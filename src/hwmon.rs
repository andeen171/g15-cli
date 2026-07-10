//! Unprivileged sensor reads via hwmon (alienware_wmi, fallback dell_smm).

use std::fs;
use std::io;

pub struct Stats {
    pub cpu: u32,  // °C
    pub gpu: u32,  // °C
    pub fan1: u32, // rpm
    pub fan2: u32, // rpm
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
    })
}
