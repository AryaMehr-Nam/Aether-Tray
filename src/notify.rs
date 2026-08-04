//! Desktop notifications for state changes.
//!
//! `notify-rust` covers all three platforms — macOS through
//! `mac-notification-sys`, Linux through D-Bus, Windows through WinRT toasts
//! — so no per-platform code is needed here.

use anyhow::Result;

pub fn send(title: &str, body: &str) -> Result<()> {
    notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .appname("Aether")
        .show()?;
    Ok(())
}
