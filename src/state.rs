//! Last-applied settings, persisted so `g15 restore` (autostart) and the bar
//! module can see them. Plain KEY=VALUE lines in ~/.config/g15/state.
//! When running privileged, resolves the invoking user's home and chowns back.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

/// (uid, gid, home) of the first /etc/passwd entry matching a uid or a name.
fn passwd_lookup(uid: Option<u32>, name: Option<&str>) -> Option<(u32, u32, String)> {
    fs::read_to_string("/etc/passwd").ok()?.lines().find_map(|line| {
        let f: Vec<&str> = line.split(':').collect();
        let (u, g) = (f.get(2)?.parse().ok()?, f.get(3)?.parse().ok()?);
        let hit = uid == Some(u) || (name.is_some() && name == f.first().copied());
        hit.then(|| (u, g, f.get(5).copied().unwrap_or_default().to_string()))
    })
}

/// The user behind a privileged run. sudo leaves SUDO_USER but keeps HOME;
/// pkexec (what the bar plugin's power and fan controls go through, so polkit
/// can ask for a password instead of a sudoers whitelist) leaves PKEXEC_UID and
/// resets HOME to root's — so writing to $HOME there would bury the state file
/// in /root and the user's own `g15 restore` would never see it.
fn invoker() -> Option<(u32, u32, String)> {
    match std::env::var("PKEXEC_UID").ok().and_then(|u| u.parse().ok()) {
        Some(uid) => passwd_lookup(Some(uid), None),
        None => match std::env::var("SUDO_USER") {
            Ok(user) if !user.is_empty() => passwd_lookup(None, Some(&user)),
            _ => None,
        },
    }
}

fn state_path() -> PathBuf {
    let home = invoker()
        .map(|(_, _, home)| home)
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| "/root".into());
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
    // running privileged: give the file back to the real user
    if let Some((uid, gid, _)) = invoker() {
        for p in [path.as_path(), path.parent().unwrap()] {
            let c = std::ffi::CString::new(p.to_str().unwrap()).unwrap();
            // Safety: valid C string, best-effort chown
            unsafe { libc::chown(c.as_ptr(), uid, gid) };
        }
    }
    Ok(())
}
