//! AW-ELC keyboard LED controller (187c:0550) over hidraw.
//!
//! Transport MUST be the USB control pipe (HIDIOCSOUTPUT/HIDIOCGINPUT):
//! the firmware ignores the interrupt endpoints. See protocol.md.
//! NEVER write /sys/class/leds/dell::kbd_backlight — it wedges this chip.

use std::fs::{self, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;

const ZONES: [u8; 4] = [0x10, 0x11, 0x12, 0x13];
const DIM_ZONE_COUNT: u8 = 0x14; // dimming addresses zones 0x00..0x13

/// (effect, duration, tempo, r, g, b)
type Action = (u8, u16, u16, u8, u8, u8);

/// The 7 colors AWCC's spectrum/wave effects use, straight from the capture.
pub const SPECTRUM: [(u8, u8, u8); 7] = [
    (0xFF, 0x00, 0x00),
    (0xFF, 0xA5, 0x00),
    (0xFF, 0xFF, 0x00),
    (0x00, 0x80, 0x00),
    (0x00, 0xBF, 0xFF),
    (0x00, 0x00, 0xFF),
    (0x80, 0x00, 0x80),
];

/// speed 1 (slow) .. 10 (fast) -> (duration, tempo). Capture reference points:
/// pulse 2000/100, morph 1500/100, spectrum 642/15, wave 428/15.
fn timing(speed: u8) -> (u16, u16) {
    let s = speed.clamp(1, 10) as u32;
    let dur = 3200 - 300 * s;
    (dur as u16, (dur / 20).max(15) as u16)
}

fn morph_actions(colors: &[(u8, u8, u8)], speed: u8, offset: usize) -> Vec<Action> {
    let (dur, tempo) = timing(speed);
    (0..colors.len())
        .map(|i| {
            let (r, g, b) = colors[(i + offset) % colors.len()];
            (0x02, dur, tempo, r, g, b)
        })
        .collect()
}

// _IOC(read|write, 'H', nr, size): buffer = report-number byte + 33 data bytes
const fn ioc(nr: u32) -> u64 {
    (3 << 30) | (34 << 16) | (('H' as u64) << 8) | nr as u64
}
const HIDIOCGINPUT: u64 = ioc(0x0A);
const HIDIOCSOUTPUT: u64 = ioc(0x0B);

pub struct Led {
    file: std::fs::File,
}

impl Led {
    pub fn open() -> io::Result<Self> {
        for entry in fs::read_dir("/sys/class/hidraw")? {
            let entry = entry?;
            let uevent = entry.path().join("device/uevent");
            let Ok(content) = fs::read_to_string(&uevent) else { continue };
            // uevent contains HID_ID=0003:0000187C:00000550
            if content.to_uppercase().contains("0000187C:00000550") {
                let dev = format!("/dev/{}", entry.file_name().to_string_lossy());
                let file = OpenOptions::new().read(true).write(true).open(&dev)?;
                // serialize g15 instances: a concurrent command's ack landing
                // between another's send and read fakes the wedge signature
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } < 0 {
                    return Err(io::Error::last_os_error());
                }
                return Ok(Led { file });
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no 187c:0550 hidraw device found (is this a Dell G15 with the AW-ELC controller?)",
        ))
    }

    /// Send one 33-byte report, read the ack back. Both over the control pipe.
    fn cmd(&self, payload: &[u8]) -> io::Result<[u8; 33]> {
        let mut buf = [0u8; 34]; // buf[0] = report number 0
        buf[1] = 0x03;
        buf[2..2 + payload.len()].copy_from_slice(payload);
        let fd = self.file.as_raw_fd();
        // Safety: fd is a valid open hidraw fd; buffers are 34 bytes as encoded in the ioctl number.
        unsafe {
            if libc::ioctl(fd, HIDIOCSOUTPUT as _, buf.as_mut_ptr()) < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut ack = [0u8; 34];
            if libc::ioctl(fd, HIDIOCGINPUT as _, ack.as_mut_ptr()) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(ack[1..34].try_into().unwrap())
        }
    }

    /// Wedge check: a healthy controller acks 0x26 with a short status
    /// (03 26 <val> 00 00...); a wedged one echoes the whole command back.
    fn dim(&self, dimming: u8) -> io::Result<()> {
        let mut p = vec![0x26, dimming, 0x00, DIM_ZONE_COUNT];
        p.extend(0..DIM_ZONE_COUNT);
        let ack = self.cmd(&p)?;
        if ack[4] != 0 {
            return Err(io::Error::other(
                "controller is wedged (echo-mode acks). Recover: USBDEVFS_RESET, or power \
                 button 30s with AC+battery removed. See protocol.md",
            ));
        }
        Ok(())
    }

    fn anim(&self, sub: u8, id: u16) -> io::Result<()> {
        self.cmd(&[0x21, 0x00, sub, (id >> 8) as u8, id as u8])?;
        Ok(())
    }

    /// One animation with per-zone-group action lists (what AWCC does).
    /// Each action: (effect, duration, tempo, r, g, b). Max 3 per report.
    fn play(&self, groups: &[(&[u8], Vec<Action>)]) -> io::Result<()> {
        self.anim(0x01, 0xFFFF)?; // start new RUNNING animation
        for (zones, actions) in groups {
            let mut series = vec![0x23, 0x01, 0x00, zones.len() as u8];
            series.extend_from_slice(zones);
            self.cmd(&series)?;
            for chunk in actions.chunks(3) {
                let mut p = vec![0x24];
                for &(effect, dur, tempo, r, g, b) in chunk {
                    p.extend_from_slice(&[
                        effect,
                        (dur >> 8) as u8,
                        dur as u8,
                        (tempo >> 8) as u8,
                        tempo as u8,
                        r,
                        g,
                        b,
                    ]);
                }
                self.cmd(&p)?;
            }
        }
        self.anim(0x03, 0x00FF) // finish-play RUNNING
    }

    pub fn color(&self, r: u8, g: u8, b: u8) -> io::Result<()> {
        self.play(&[(&ZONES, vec![(0x00, 0x07D0, 0x00FA, r, g, b)])])
    }

    /// Smooth breathe: firmware effect 0x01 is a hard blink, so pulse is a
    /// morph pair color -> black -> color; dimming still caps the max level.
    pub fn pulse(&self, r: u8, g: u8, b: u8, speed: u8) -> io::Result<()> {
        let (dur, tempo) = timing(speed);
        self.play(&[(
            &ZONES,
            vec![(0x02, dur, tempo, r, g, b), (0x02, dur, tempo, 0, 0, 0)],
        )])
    }

    pub fn morph(&self, c1: (u8, u8, u8), c2: (u8, u8, u8), speed: u8) -> io::Result<()> {
        let (dur, tempo) = timing(speed);
        self.play(&[(
            &ZONES,
            vec![
                (0x02, dur, tempo, c1.0, c1.1, c1.2),
                (0x02, dur, tempo, c2.0, c2.1, c2.2),
            ],
        )])
    }

    /// Morph through a color list on all zones together
    /// (AWCC's "spectrum" — captured as chained 0x02 actions).
    pub fn cycle(&self, colors: &[(u8, u8, u8)], speed: u8) -> io::Result<()> {
        self.play(&[(&ZONES, morph_actions(colors, speed, 0))])
    }

    /// Same cycle but each zone offset in the list — a moving rainbow
    /// across the 4 zones (AWCC's "wave": per-zone series, rotated colors).
    pub fn rainbow(&self, colors: &[(u8, u8, u8)], speed: u8) -> io::Result<()> {
        let groups: Vec<(&[u8], Vec<Action>)> = ZONES
            .iter()
            .enumerate()
            // spread the color list evenly across the 4 zones
            .map(|(i, z)| {
                (
                    std::slice::from_ref(z),
                    morph_actions(colors, speed, i * colors.len() / ZONES.len()),
                )
            })
            .collect();
        self.play(&groups)
    }

    /// brightness 0-100; the 0x26 command is DIMMING (inverted).
    pub fn brightness(&self, percent: u8) -> io::Result<()> {
        self.dim(100 - percent.min(100))
    }

    pub fn fw_version(&self) -> io::Result<String> {
        let ack = self.cmd(&[0x20, 0x00])?;
        Ok(format!("{}.{}.{}", ack[3], ack[4], ack[5]))
    }
}

/// Build the wire bytes for one report (for tests).
#[cfg(test)]
fn report(payload: &[u8]) -> [u8; 33] {
    let mut r = [0u8; 33];
    r[0] = 0x03;
    r[1..1 + payload.len()].copy_from_slice(payload);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    // Known-good bytes straight from the AWCC Wireshark capture.
    #[test]
    fn packets_match_capture() {
        let series: Vec<u8> = {
            let mut p = vec![0x23, 0x01, 0x00, 4];
            p.extend_from_slice(&ZONES);
            p
        };
        assert_eq!(&report(&series)[..9], &[0x03, 0x23, 0x01, 0x00, 0x04, 0x10, 0x11, 0x12, 0x13]);

        // static red action: 03 24 00 07d0 00fa ff 00 00
        let action = [0x24, 0x00, 0x07, 0xD0, 0x00, 0xFA, 0xFF, 0x00, 0x00];
        assert_eq!(&report(&action)[..10], &[0x03, 0x24, 0x00, 0x07, 0xD0, 0x00, 0xFA, 0xFF, 0x00, 0x00]);

        // dim 100 (off): 03 26 64 00 14 00 01 .. 13
        let mut dim = vec![0x26, 100, 0x00, DIM_ZONE_COUNT];
        dim.extend(0..DIM_ZONE_COUNT);
        let r = report(&dim);
        assert_eq!(&r[..5], &[0x03, 0x26, 0x64, 0x00, 0x14]);
        assert_eq!(r[5 + 0x13], 0x13);
    }

    #[test]
    fn ioctl_numbers() {
        assert_eq!(HIDIOCGINPUT, 0xC022480A);
        assert_eq!(HIDIOCSOUTPUT, 0xC022480B);
    }
}
