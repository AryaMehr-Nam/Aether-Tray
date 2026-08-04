use std::fs;
use std::path::{Path, PathBuf};

fn pid_file(data_dir: &Path) -> PathBuf {
    data_dir.join("aether.pid")
}

pub fn write_pid(data_dir: &Path, pid: u32) {
    let _ = fs::write(pid_file(data_dir), pid.to_string());
}

pub fn clear_pid(data_dir: &Path) {
    let _ = fs::remove_file(pid_file(data_dir));
}

/// On startup, if a pid file survives from a prior crash and that process is
/// still alive, kill it before the user can hit Connect — otherwise a
/// leftover Aether would just fail to bind the SOCKS port for the new one.
/// This is a defensive backstop; `start_connect`'s own port-in-use check
/// (see core/mod.rs) covers the case where this file is missing or stale.
pub fn reap_orphan(data_dir: &Path) {
    let path = pid_file(data_dir);
    let Ok(contents) = fs::read_to_string(&path) else {
        return;
    };
    if let Ok(pid) = contents.trim().parse::<u32>()
        && is_alive(pid)
    {
        kill_pid(pid);
    }
    let _ = fs::remove_file(&path);
}

#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
}

#[cfg(windows)]
fn is_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_clear_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        write_pid(dir.path(), 4242);
        assert_eq!(
            fs::read_to_string(pid_file(dir.path())).unwrap().trim(),
            "4242"
        );
        clear_pid(dir.path());
        assert!(!pid_file(dir.path()).exists());
    }

    #[test]
    fn reap_removes_pid_file_even_when_process_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        // A pid that is essentially certain not to be live.
        fs::write(pid_file(dir.path()), "999999").unwrap();
        reap_orphan(dir.path());
        assert!(!pid_file(dir.path()).exists());
    }

    #[test]
    fn reap_tolerates_a_missing_or_garbage_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        reap_orphan(dir.path()); // no file at all
        fs::write(pid_file(dir.path()), "not-a-pid").unwrap();
        reap_orphan(dir.path());
        assert!(!pid_file(dir.path()).exists());
    }
}
