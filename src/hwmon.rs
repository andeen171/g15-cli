//! Unprivileged sensor reads via hwmon (alienware_wmi, fallback dell_smm).

use std::fs;
use std::io;

pub struct Stats {
    pub cpu: u32,   // °C
    pub gpu: u32,   // °C
    pub fan1: u32,  // rpm
    pub fan2: u32,  // rpm
    pub boost: u32, // % — alienware_wmi only, 0 on the dell_smm fallback
}

fn find() -> io::Result<std::path::PathBuf> {
    let mut fallback = None;
    for entry in fs::read_dir("/sys/class/hwmon")? {
        let path = entry?.path();
        match fs::read_to_string(path.join("name")).unwrap_or_default().trim() {
            "alienware_wmi" => return Ok(path),
            "dell_smm" => fallback = Some(path),
            _ => {}
        }
    }
    fallback.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no dell/alienware hwmon"))
}

fn read_u32(path: &std::path::Path) -> u32 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

pub fn read() -> io::Result<Stats> {
    let dir = find()?;
    Ok(Stats {
        cpu: read_u32(&dir.join("temp1_input")) / 1000,
        gpu: read_u32(&dir.join("temp2_input")) / 1000,
        fan1: read_u32(&dir.join("fan1_input")),
        fan2: read_u32(&dir.join("fan2_input")),
        // The same value `g15 fan boost` writes through WMAX, readable without
        // root — no need to guess from what was last set.
        boost: read_u32(&dir.join("fan1_boost")),
    })
}

/// The alienware-wmi driver's platform profile, e.g. "quiet" or "performance".
/// World-readable, unlike the WMAX power-mode read, and it also catches mode
/// changes made outside g15 — omarchy's own power menu writes this node.
pub fn platform_profile() -> Option<String> {
    fs::read_dir("/sys/class/platform-profile")
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| {
            fs::read_to_string(e.path().join("name")).unwrap_or_default().trim() == "alienware-wmi"
        })
        .and_then(|e| fs::read_to_string(e.path().join("profile")).ok())
        .map(|p| p.trim().to_string())
}

/// The g15 mode name for a platform profile, where one exists. The driver has
/// profiles g15 does not name (`balanced-performance`, `custom`) and g15 has a
/// mode the driver cannot report (G-Mode goes through WMAX behind its back).
pub fn mode_for_profile(profile: &str) -> Option<&'static str> {
    match profile {
        "quiet" => Some("quiet"),
        "balanced" => Some("balanced"),
        "performance" | "balanced-performance" => Some("performance"),
        "low-power" => Some("battery"),
        _ => None,
    }
}
