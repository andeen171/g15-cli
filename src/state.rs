//! Last-applied settings, persisted so `g15 restore` (autostart) and the
//! waybar module can see them. Plain KEY=VALUE lines in ~/.config/g15/state.
//! When running under sudo, resolves the invoking user's home and chowns back.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

fn state_path() -> PathBuf {
    let home = match std::env::var("SUDO_USER") {
        Ok(user) if !user.is_empty() => format!("/home/{user}"),
        _ => std::env::var("HOME").unwrap_or_else(|_| "/root".into()),
    };
    PathBuf::from(home).join(".config/g15/state")
}

pub fn load() -> HashMap<String, String> {
    fs::read_to_string(state_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub fn set(key: &str, value: &str) -> io::Result<()> {
    let mut map = load();
    map.insert(key.to_string(), value.to_string());
    let path = state_path();
    fs::create_dir_all(path.parent().unwrap())?;
    let mut out: Vec<_> = map.into_iter().collect();
    out.sort();
    let body: String = out.into_iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    fs::write(&path, body)?;
    // running under sudo: give the file back to the real user
    if let (Ok(uid), Ok(gid)) = (std::env::var("SUDO_UID"), std::env::var("SUDO_GID")) {
        if let (Ok(uid), Ok(gid)) = (uid.parse::<u32>(), gid.parse::<u32>()) {
            for p in [path.as_path(), path.parent().unwrap()] {
                let c = std::ffi::CString::new(p.to_str().unwrap()).unwrap();
                // Safety: valid C string, best-effort chown
                unsafe { libc::chown(c.as_ptr(), uid, gid) };
            }
        }
    }
    Ok(())
}
