//! macOS dialogs via AppleScript.
//!
//! `osascript` renders a real Aqua dialog and, being a subprocess, has none
//! of AppKit's main-thread constraints — so this is safe to call from the
//! tray loop without marshalling.

use std::process::Command;

/// Escapes a value for embedding in an AppleScript string literal. Without
/// this, a quote or backslash in a prompt or default value would terminate
/// the literal and the rest would be interpreted as script.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn available() -> bool {
    true
}

pub fn prompt(title: &str, message: &str, default: &str, secret: bool) -> Option<String> {
    let hidden = if secret { " with hidden answer" } else { "" };
    let script = format!(
        r#"display dialog "{}" with title "{}" default answer "{}"{} buttons {{"Cancel", "OK"}} default button "OK"
set r to text returned of result"#,
        escape(message),
        escape(title),
        escape(default),
        hidden
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .ok()?;
    // Cancel exits non-zero; treat that as "no change" rather than empty.
    if !output.status.success() {
        return None;
    }
    Some(super::trim_one_newline(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_neutralises_quotes_and_backslashes() {
        assert_eq!(escape(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(escape(r"back\slash"), r"back\\slash");
        assert_eq!(escape("plain"), "plain");
    }
}
