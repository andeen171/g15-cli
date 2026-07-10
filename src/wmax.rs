//! Fan/power control via the WMAX WMI method through /proc/acpi/call.
//! Needs root and the acpi_call kernel module (modprobe acpi_call).
//! Codes ported from dell-g-series-controller main.py (verified for G15 5520).

use std::fs;
use std::io;

// ponytail: AMWW is the Intel-model path (this 5520); AMW3 fallback covers AMD models.
const METHODS: [&str; 2] = ["\\_SB.AMWW.WMAX", "\\_SB.AMW3.WMAX"];

pub const POWER_MODES: [(&str, u8); 5] = [
    ("balanced", 0xA0),
    ("performance", 0xA1),
    ("quiet", 0xA3),
    ("battery", 0xA5),
    ("gmode", 0xAB),
];

fn call(op: u8, args: [u8; 3]) -> io::Result<u32> {
    let mut last = io::Error::other("no WMAX method worked");
    for method in METHODS {
        let cmd = format!(
            "{} 0 {:#x} {{{:#x}, {:#x}, {:#x}, 0x00}}",
            method, op, args[0], args[1], args[2]
        );
        if let Err(e) = fs::write("/proc/acpi/call", &cmd) {
            last = match e.kind() {
                io::ErrorKind::NotFound => {
                    io::Error::other("/proc/acpi/call missing — run: sudo modprobe acpi_call")
                }
                io::ErrorKind::PermissionDenied => io::Error::other("needs root (sudo)"),
                _ => e,
            };
            continue;
        }
        let raw = fs::read("/proc/acpi/call")?;
        let text = String::from_utf8_lossy(&raw);
        let text = text.trim_end_matches('\0').trim();
        if text.starts_with("Error") || text.contains("not found") {
            last = io::Error::other(format!("WMAX call failed: {text}"));
            continue;
        }
        let hex = text.trim_start_matches("0x");
        return u32::from_str_radix(hex, 16)
            .map_err(|_| io::Error::other(format!("unparseable acpi_call result: {text}")));
    }
    Err(last)
}

pub fn get_power_mode() -> io::Result<u32> {
    call(0x14, [0x0B, 0x00, 0x00])
}

pub fn set_power_mode(mode: u8) -> io::Result<()> {
    call(0x15, [0x01, mode, 0x00])?;
    // G-Mode has an extra enable flag AWCC sets alongside the mode
    call(0x25, [0x01, (mode == 0xAB) as u8, 0x00])?;
    Ok(())
}

pub fn fan_boost(fan: u8) -> io::Result<u32> {
    call(0x14, [0x0C, 0x32 + fan, 0x00])
}

pub fn set_fan_boost(fan: u8, boost: u8) -> io::Result<()> {
    call(0x15, [0x02, 0x32 + fan, boost]).map(|_| ())
}

pub fn fan_rpm(fan: u8) -> io::Result<u32> {
    call(0x14, [0x05, 0x32 + fan, 0x00])
}

pub fn cpu_temp() -> io::Result<u32> {
    call(0x14, [0x04, 0x01, 0x00])
}

pub fn gpu_temp() -> io::Result<u32> {
    call(0x14, [0x04, 0x06, 0x00])
}

pub fn laptop_model() -> io::Result<u32> {
    call(0x1A, [0x02, 0x02, 0x00])
}
