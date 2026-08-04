//! Persistent configuration: the last-successful connection profile plus
//! app-level settings that are not core flags.
//!
//! Stored as TOML in the platform config dir, replacing the two
//! `tauri-plugin-store` JSON files the windowed app used.

use crate::core::profile::ConnectionProfile;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The old Tauri bundle identifier. Migration reads from here; the originals
/// are left untouched so the windowed app keeps working.
const LEGACY_IDENTIFIER: &str = "com.cluvexstudio.aethergui";
const LEGACY_PROFILE_STORE: &str = "profile.json";
const LEGACY_SETTINGS_STORE: &str = "settings.json";
const LEGACY_PROFILE_KEY: &str = "last_successful_profile";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub profile: ConnectionProfile,
    #[serde(default)]
    pub settings: Settings,
}

/// App-level settings. These are this app's own behaviour, not core flags —
/// note `quick_reconnect` is deliberately absent: it is a *core* flag living
/// on `ConnectionProfile`, and conflating it with auto-retry policy would
/// silently disable reconnection when a user turned off gateway reuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_true")]
    pub notifications: bool,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default)]
    pub connect_on_launch: bool,
    /// Optional explicit path to the core binary — step 2 of the lookup
    /// order (`<exe_dir>/aether` → `core_path` → `$PATH`).
    #[serde(default)]
    pub core_path: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notifications: true,
            launch_at_login: false,
            connect_on_launch: false,
            core_path: None,
        }
    }
}

impl Config {
    /// Loads `config.toml`, falling back to a one-time migration from the
    /// Tauri stores, then to defaults. A malformed file is reported rather
    /// than silently replaced — overwriting a user's settings because of a
    /// stray edit would be worse than refusing to start.
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::config_path()?, legacy_data_dir().as_deref())
    }

    /// `legacy_dir` is the Tauri store directory to migrate from on first
    /// run, passed explicitly so tests never touch the real home directory.
    fn load_from(path: &Path, legacy_dir: Option<&Path>) -> Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            return toml::from_str(&content)
                .map_err(|e| anyhow!("{} is not valid config TOML: {e}", path.display()));
        }
        if let Some(migrated) = legacy_dir
            .and_then(|d| migrate_from_dir(d).transpose())
            .transpose()?
        {
            migrated.save_to(path)?;
            return Ok(migrated);
        }
        Ok(Config::default())
    }

    /// Persists the config. Every Access credential is erased first: a
    /// successful profile is worth remembering, but a JWT, service secret or
    /// email address must never reach disk (see `ConnectionProfile::scrubbed`).
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path()?)
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let scrubbed = Config {
            profile: self.profile.scrubbed(),
            settings: self.settings.clone(),
        };
        std::fs::write(path, toml::to_string_pretty(&scrubbed)?)?;
        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(config_dir()?.join("config.toml"))
    }

    /// Where forwarded core output is appended, for the "Show logs…" item.
    pub fn log_path() -> Result<PathBuf> {
        Ok(config_dir()?.join("aether-tray.log"))
    }

    /// Where Aether itself runs. Must stay stable across launches: the core
    /// writes its provisioned identity (`aether-masque.toml` / `aether.toml`)
    /// into its working directory, so a changing cwd silently re-provisions
    /// the WARP identity every run.
    pub fn data_dir() -> Result<PathBuf> {
        match legacy_data_dir().filter(|d| d.exists()) {
            Some(dir) => Ok(dir),
            None => config_dir(),
        }
    }
}

fn config_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
        Ok(home.join("Library/Application Support/aether-tray"))
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = dirs::data_dir().ok_or_else(|| anyhow!("no AppData directory"))?;
        Ok(appdata.join("aether-tray"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let config = dirs::config_dir().ok_or_else(|| anyhow!("no config directory"))?;
        Ok(config.join("aether-tray"))
    }
}

/// The windowed app's data dir. Kept as the core's cwd when it exists, so an
/// upgrading user's provisioned WARP identity is not orphaned.
fn legacy_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| {
            h.join("Library/Application Support")
                .join(LEGACY_IDENTIFIER)
        })
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().map(|d| d.join(LEGACY_IDENTIFIER))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir().map(|d| d.join(LEGACY_IDENTIFIER))
    }
}

/// Imports `last_successful_profile` and `close_to_tray` from the Tauri
/// stores on first run. The originals are read-only here — the windowed app
/// must keep working after this.
fn migrate_from_dir(dir: &Path) -> Result<Option<Config>> {
    let profile_store = dir.join(LEGACY_PROFILE_STORE);
    let settings_store = dir.join(LEGACY_SETTINGS_STORE);
    if !profile_store.exists() && !settings_store.exists() {
        return Ok(None);
    }

    let mut config = Config::default();

    // A store that exists but won't parse is skipped rather than fatal: a
    // corrupt legacy file should not block first launch.
    if let Ok(raw) = std::fs::read_to_string(&profile_store)
        && let Ok(store) = serde_json::from_str::<serde_json::Value>(&raw)
        && let Some(value) = store.get(LEGACY_PROFILE_KEY)
        && let Ok(profile) = serde_json::from_value::<ConnectionProfile>(value.clone())
    {
        // Credentials were already scrubbed on the way in, but a
        // hand-edited store could still carry them.
        config.profile = profile.scrubbed();
    }

    if let Ok(raw) = std::fs::read_to_string(&settings_store)
        && let Ok(store) = serde_json::from_str::<serde_json::Value>(&raw)
    {
        // `close_to_tray` meant "keep running without a window", which is
        // this app's only mode — it survives as connect-on-launch intent
        // only if explicitly set; otherwise defaults apply.
        if let Some(v) = store.get("connect_on_launch").and_then(|v| v.as_bool()) {
            config.settings.connect_on_launch = v;
        }
        if let Some(v) = store.get("notifications").and_then(|v| v.as_bool()) {
            config.settings.notifications = v;
        }
    }

    Ok(Some(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::{MasqueNoize, Protocol, ScanMode};

    #[test]
    fn round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.profile.protocol = Protocol::Gool;
        cfg.profile.scan_mode = ScanMode::Ironclad;
        cfg.profile.bind_address = "0.0.0.0:1919".into();
        cfg.settings.notifications = false;

        cfg.save_to(&path).unwrap();
        let loaded = Config::load_from(&path, None).unwrap();

        assert_eq!(loaded.profile.protocol, Protocol::Gool);
        assert_eq!(loaded.profile.scan_mode, ScanMode::Ironclad);
        assert_eq!(loaded.profile.bind_address, "0.0.0.0:1919");
        assert!(!loaded.settings.notifications);
    }

    #[test]
    fn save_scrubs_every_access_credential() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.profile.zero_trust_team = "acme".into();
        cfg.profile.access_email = "me@example.com".into();
        cfg.profile.access_client_id = "client-id".into();
        cfg.profile.access_client_secret = "super-secret".into();
        cfg.profile.access_token = "a.jwt.value".into();

        cfg.save_to(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();

        for secret in ["me@example.com", "client-id", "super-secret", "a.jwt.value"] {
            assert!(!raw.contains(secret), "{secret} leaked into {raw}");
        }
        // The non-credential part of the Zero Trust config survives.
        assert!(raw.contains("acme"));
    }

    #[test]
    fn missing_config_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nope.toml"), None).unwrap();
        assert_eq!(cfg.profile, ConnectionProfile::default());
    }

    #[test]
    fn first_run_migrates_then_writes_config_toml() {
        let legacy = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(
            legacy.path().join(LEGACY_PROFILE_STORE),
            r#"{"last_successful_profile":{"protocol":"masque","scan_mode":"turbo"}}"#,
        )
        .unwrap();

        let path = target.path().join("config.toml");
        let cfg = Config::load_from(&path, Some(legacy.path())).unwrap();
        assert_eq!(cfg.profile.protocol, Protocol::Masque);
        // The migrated result is persisted, so this only happens once.
        assert!(path.exists());
        let second = Config::load_from(&path, Some(legacy.path())).unwrap();
        assert_eq!(second.profile.scan_mode, ScanMode::Turbo);
    }

    #[test]
    fn malformed_config_is_an_error_not_a_silent_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is not toml {{{").unwrap();
        assert!(Config::load_from(&path, None).is_err());
    }

    #[test]
    fn migrates_last_successful_profile_from_the_tauri_store() {
        let dir = tempfile::tempdir().unwrap();
        // Shape written by tauri-plugin-store: a flat key/value JSON object.
        std::fs::write(
            dir.path().join(LEGACY_PROFILE_STORE),
            r#"{"last_successful_profile":{
                "protocol":"gool","scan_mode":"thorough","ip_version":"both",
                "quick_reconnect":false,"masque_http2":true,
                "bind_address":"0.0.0.0:1919","dns":"1.1.1.1"
            }}"#,
        )
        .unwrap();

        let cfg = migrate_from_dir(dir.path()).unwrap().expect("migrated");
        assert_eq!(cfg.profile.protocol, Protocol::Gool);
        assert_eq!(cfg.profile.scan_mode, ScanMode::Thorough);
        assert_eq!(cfg.profile.bind_address, "0.0.0.0:1919");
        assert_eq!(cfg.profile.dns, "1.1.1.1");
        assert!(!cfg.profile.quick_reconnect);
        assert!(cfg.profile.masque_http2);
        // Fields absent from the old store take their defaults.
        assert_eq!(cfg.profile.masque_noize, MasqueNoize::Firewall);
    }

    #[test]
    fn migration_leaves_the_originals_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join(LEGACY_PROFILE_STORE);
        let original = r#"{"last_successful_profile":{"protocol":"masque"}}"#;
        std::fs::write(&store, original).unwrap();

        migrate_from_dir(dir.path()).unwrap().expect("migrated");

        assert_eq!(std::fs::read_to_string(&store).unwrap(), original);
    }

    #[test]
    fn migration_scrubs_credentials_from_a_hand_edited_store() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(LEGACY_PROFILE_STORE),
            r#"{"last_successful_profile":{
                "protocol":"auto","scan_mode":"balanced","ip_version":"v4",
                "zero_trust_team":"acme","access_token":"leaked.jwt"
            }}"#,
        )
        .unwrap();

        let cfg = migrate_from_dir(dir.path()).unwrap().expect("migrated");
        assert_eq!(cfg.profile.zero_trust_team, "acme");
        assert!(cfg.profile.access_token.is_empty());
    }

    #[test]
    fn no_legacy_stores_means_no_migration() {
        let dir = tempfile::tempdir().unwrap();
        assert!(migrate_from_dir(dir.path()).unwrap().is_none());
    }

    #[test]
    fn corrupt_legacy_store_does_not_block_startup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LEGACY_PROFILE_STORE), "{not json").unwrap();
        let cfg = migrate_from_dir(dir.path()).unwrap().expect("migrated");
        assert_eq!(cfg.profile, ConnectionProfile::default());
    }
}
