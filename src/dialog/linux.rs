//! Linux dialogs: zenity, then kdialog, then nothing.
//!
//! When neither helper is installed `prompt` returns `None` and the caller
//! falls back to the "Edit config.toml…" menu item. This is a real
//! configuration on minimal desktops, not a theoretical one.

use std::process::Command;

fn has(binary: &str) -> bool {
    which::which(binary).is_ok()
}

pub fn available() -> bool {
    has("zenity") || has("kdialog")
}

pub fn prompt(title: &str, message: &str, default: &str, secret: bool) -> Option<String> {
    if has("zenity") {
        return zenity(title, message, default, secret);
    }
    if has("kdialog") {
        return kdialog(title, message, default, secret);
    }
    None
}

fn zenity(title: &str, message: &str, default: &str, secret: bool) -> Option<String> {
    let mut cmd = Command::new("zenity");
    cmd.arg("--entry")
        .arg(format!("--title={title}"))
        .arg(format!("--text={message}"));
    if secret {
        cmd.arg("--hide-text");
    } else if !default.is_empty() {
        cmd.arg(format!("--entry-text={default}"));
    }
    run(cmd)
}

fn kdialog(title: &str, message: &str, default: &str, secret: bool) -> Option<String> {
    let mut cmd = Command::new("kdialog");
    cmd.arg("--title").arg(title);
    if secret {
        cmd.arg("--password").arg(message);
    } else {
        cmd.arg("--inputbox").arg(message).arg(default);
    }
    run(cmd)
}

fn run(mut cmd: Command) -> Option<String> {
    let output = cmd.output().ok()?;
    // Both helpers exit non-zero on Cancel — that must read as "no change",
    // not as an empty value that would wipe the current setting.
    if !output.status.success() {
        return None;
    }
    Some(super::trim_one_newline(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}
