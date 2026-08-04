//! Native text-input dialogs.
//!
//! There is no app window and no GUI framework, so each platform shells out
//! to the dialog facility the OS already ships. That also sidesteps macOS's
//! main-thread requirement for AppKit alerts: a subprocess has no such
//! constraint, and the tray loop can call these directly.
//!
//! Every entry point returns `None` when the user cancels *or* when no dialog
//! helper is available. Callers must treat `None` as "no change", never as an
//! empty value — on Linux with neither zenity nor kdialog installed, `None`
//! is the normal outcome and the "Edit config.toml…" menu item is the
//! documented way to change settings.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod linux;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use linux as imp;

/// Asks for a single line of text, pre-filled with `default`.
pub fn prompt_text(title: &str, message: &str, default: Option<&str>) -> Option<String> {
    imp::prompt(title, message, default.unwrap_or(""), false)
}

/// Asks for a value that must not be echoed to the screen or shell history.
pub fn prompt_secret(title: &str, message: &str) -> Option<String> {
    imp::prompt(title, message, "", true)
}

/// Whether a dialog helper is actually available. Used to decide whether to
/// offer a dialog or point the user at the config file.
pub fn available() -> bool {
    imp::available()
}

/// Trims exactly one trailing newline from helper output without touching
/// meaningful whitespace inside the value.
pub(crate) fn trim_one_newline(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_the_final_line_terminator() {
        assert_eq!(trim_one_newline("abc\n".into()), "abc");
        assert_eq!(trim_one_newline("abc\r\n".into()), "abc");
        // An interior newline and meaningful spaces are preserved.
        assert_eq!(trim_one_newline("a\nb\n".into()), "a\nb");
        assert_eq!(trim_one_newline("  spaced  \n".into()), "  spaced  ");
        assert_eq!(trim_one_newline("no-newline".into()), "no-newline");
        // Only one terminator goes: a deliberate trailing blank line stays.
        assert_eq!(trim_one_newline("abc\n\n".into()), "abc\n");
    }
}
