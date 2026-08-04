//! Launch-at-login, per platform.

use anyhow::{Result, anyhow};
use std::path::PathBuf;

const LABEL: &str = "com.cluvexstudio.aether-tray";

fn exe() -> Result<PathBuf> {
    std::env::current_exe().map_err(|e| anyhow!("cannot locate this executable: {e}"))
}

#[cfg(target_os = "macos")]
fn plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

#[cfg(target_os = "macos")]
pub fn set_enabled(enabled: bool) -> Result<()> {
    let path = plist_path()?;
    if !enabled {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array><string>{}</string></array>
    <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        exe()?.display()
    );
    std::fs::write(&path, plist)?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn is_enabled() -> bool {
    plist_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(target_os = "windows")]
pub fn set_enabled(enabled: bool) -> Result<()> {
    // Shelling out to reg.exe avoids a winreg dependency for two calls.
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let status = if enabled {
        std::process::Command::new("reg")
            .args(["add", KEY, "/v", LABEL, "/t", "REG_SZ", "/d"])
            .arg(exe()?)
            .arg("/f")
            .status()?
    } else {
        std::process::Command::new("reg")
            .args(["delete", KEY, "/v", LABEL, "/f"])
            .status()?
    };
    if !status.success() && enabled {
        return Err(anyhow!("reg.exe failed to set the Run key"));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn is_enabled() -> bool {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    std::process::Command::new("reg")
        .args(["query", KEY, "/v", LABEL])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn desktop_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().ok_or_else(|| anyhow!("no config directory"))?;
    Ok(dir.join("autostart").join(format!("{LABEL}.desktop")))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn set_enabled(enabled: bool) -> Result<()> {
    let path = desktop_path()?;
    if !enabled {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Aether\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exe()?.display()
    );
    std::fs::write(&path, entry)?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn is_enabled() -> bool {
    desktop_path().map(|p| p.exists()).unwrap_or(false)
}
