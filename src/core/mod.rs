//! The supervisor: owns the core subprocess, the PTY reader and the state
//! machine. Ported from the windowed app's `aether/mod.rs`, with Tauri's
//! `AppHandle`/`emit` replaced by plain channels.

pub mod orphan;
pub mod profile;
pub mod prompts;
pub mod pty;
pub mod status;

use crate::config::Config;
use anyhow::{Result, anyhow};
use log::error;
use profile::ConnectionProfile;
use pty::PtySession;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ConnectionState {
    Idle,
    Launching,
    Connecting,
    Connected {
        socks_addr: String,
        connected_at_ms: u64,
    },
    Reconnecting {
        attempt: u32,
        max_attempts: u32,
    },
    Disconnecting,
    Error {
        message: String,
        phase: String,
    },
}

impl ConnectionState {
    /// Settings may only be changed from a resting state, mirroring the
    /// windowed app's locking.
    pub fn is_resting(&self) -> bool {
        matches!(self, ConnectionState::Idle | ConnectionState::Error { .. })
    }

    pub fn label(&self) -> String {
        match self {
            ConnectionState::Idle => "Idle".into(),
            ConnectionState::Launching => "Launching…".into(),
            ConnectionState::Connecting => "Connecting…".into(),
            ConnectionState::Connected { socks_addr, .. } => format!("Connected · {socks_addr}"),
            ConnectionState::Reconnecting {
                attempt,
                max_attempts,
            } => format!("Reconnecting {attempt}/{max_attempts}…"),
            ConnectionState::Disconnecting => "Disconnecting…".into(),
            ConnectionState::Error { message, .. } => format!("Error: {message}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEvent {
    pub line: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub enum StatusEvent {
    StateChange(ConnectionState),
}

#[derive(Debug, Clone)]
pub enum Command {
    /// Boxed because `ConnectionProfile` is far larger than the other
    /// variants, and every queued command would otherwise pay for it.
    Connect {
        profile: Option<Box<ConnectionProfile>>,
    },
    Disconnect,
    SubmitCode {
        code: String,
    },
    Shutdown,
}

impl Command {
    /// Convenience for the common case of connecting with a known profile.
    pub fn connect(profile: ConnectionProfile) -> Self {
        Command::Connect {
            profile: Some(Box::new(profile)),
        }
    }
}

/// Shared mutable core of the supervisor. Threads spawned per connection
/// attempt hold an `Arc` of this; the command loop owns the other end.
struct Inner {
    session: Option<PtySession>,
    state: ConnectionState,
    user_requested_stop: bool,
    /// Consecutive auto-retry attempts for the current connection lineage.
    /// Reset on a fresh user-initiated connect, on reaching Connected (a
    /// proven-working connection earns a full retry budget for whatever
    /// drops it next), and on a user-requested disconnect.
    retry_count: u32,
}

#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Mutex<Inner>>,
    status_tx: Sender<StatusEvent>,
    log_tx: Sender<LogEvent>,
    binary: PathBuf,
    /// The core's working directory — shared with the windowed app so the
    /// provisioned WARP identity persists. We do not own files here.
    core_cwd: PathBuf,
    /// Our own state directory; the pid file lives here.
    state_dir: PathBuf,
}

/// Everything a caller needs to drive and observe a supervisor.
pub struct SupervisorHandles {
    pub commands: Sender<Command>,
    pub status: Receiver<StatusEvent>,
    pub logs: Receiver<LogEvent>,
    pub supervisor: Supervisor,
}

impl Supervisor {
    /// Resolves the core binary and data dir, reaps any orphan from a prior
    /// crash, and returns the channels plus a handle. Named `start` rather
    /// than `new` because it returns handles, not `Self`, and spawns the
    /// command loop as a side effect.
    pub fn start(config: &Config) -> Result<SupervisorHandles> {
        let binary = resolve_binary(config)?;
        let core_cwd = Config::core_cwd()?;
        let state_dir = Config::state_dir()?;
        std::fs::create_dir_all(&core_cwd)?;
        std::fs::create_dir_all(&state_dir)?;
        // Before anything can bind the SOCKS port, clear a leftover core from
        // a previous crash of *this* app. Reads only our own state dir, so a
        // core belonging to the windowed app is never touched.
        orphan::reap_orphan(&state_dir);

        let (status_tx, status_rx) = mpsc::channel();
        let (log_tx, log_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();

        let supervisor = Supervisor {
            inner: Arc::new(Mutex::new(Inner {
                session: None,
                state: ConnectionState::Idle,
                user_requested_stop: false,
                retry_count: 0,
            })),
            status_tx,
            log_tx,
            binary,
            core_cwd,
            state_dir,
        };

        let for_loop = supervisor.clone();
        std::thread::spawn(move || for_loop.command_loop(cmd_rx));

        Ok(SupervisorHandles {
            commands: cmd_tx,
            status: status_rx,
            logs: log_rx,
            supervisor,
        })
    }

    pub fn state(&self) -> ConnectionState {
        self.inner.lock().unwrap().state.clone()
    }

    /// Drains commands until `Shutdown`. Each command returns promptly —
    /// connection progress happens on per-attempt monitor threads, so a
    /// Disconnect issued mid-connect is never queued behind a scan.
    fn command_loop(self, cmd_rx: Receiver<Command>) {
        for cmd in cmd_rx {
            match cmd {
                Command::Connect { profile } => {
                    if let Err(e) = self.start_connect(profile) {
                        // Preconditions (already running, port in use, spawn
                        // failure) have already set their own state.
                        error!("connect: {e}");
                    }
                }
                Command::Disconnect => {
                    if let Err(e) = self.request_disconnect() {
                        error!("disconnect: {e}");
                    }
                }
                Command::SubmitCode { code } => {
                    if let Err(e) = self.submit_access_code(&code) {
                        error!("access code: {e}");
                    }
                }
                Command::Shutdown => {
                    self.shutdown_blocking();
                    return;
                }
            }
        }
    }

    fn set_state(&self, new_state: ConnectionState) {
        self.inner.lock().unwrap().state = new_state.clone();
        let _ = self.status_tx.send(StatusEvent::StateChange(new_state));
    }

    /// Kicks off a connection attempt and returns as soon as Aether is
    /// spawned (or a synchronous precondition fails). The
    /// Launching → Connecting → Connected transitions happen on a monitor
    /// thread and reach the caller as `StatusEvent`s.
    pub fn start_connect(&self, profile_override: Option<Box<ConnectionProfile>>) -> Result<()> {
        let profile = match profile_override {
            Some(p) => *p,
            None => Config::load().unwrap_or_default().profile,
        };

        {
            let mut inner = self.inner.lock().unwrap();
            if !inner.state.is_resting() {
                // Deliberately does NOT set Error: there is a live attempt or
                // connection behind this state, and clobbering it would
                // report a failure that did not happen.
                return Err(anyhow!("Aether is already running"));
            }
            // Defensive guard independent of the pid-file mechanism in
            // orphan.rs (covers a manually-started Aether, another app's
            // core, or a missing pid file), checked under the same lock as
            // the state check above so a rapid double-click can't race two
            // connects past this guard.
            let socks = status::parse_bind_address(&profile.bind_address);
            if status::port_is_live(&socks) {
                let message = format!("port {} is already in use by another process", socks.port());
                inner.state = ConnectionState::Error {
                    message: message.clone(),
                    phase: "launching".into(),
                };
                let new_state = inner.state.clone();
                drop(inner);
                // Must reach the UI: the user pressed Connect and it failed,
                // so leaving the tray on Idle would look like nothing
                // happened at all.
                let _ = self.status_tx.send(StatusEvent::StateChange(new_state));
                return Err(anyhow!(message));
            }
            inner.state = ConnectionState::Launching;
            // A fresh user-initiated connect always gets a full retry budget.
            inner.retry_count = 0;
        }
        let _ = self
            .status_tx
            .send(StatusEvent::StateChange(ConnectionState::Launching));

        self.spawn_and_monitor(profile)
    }

    /// Spawns the PTY session and its monitor thread. Shared by the initial
    /// user-initiated connect and by `handle_unexpected_failure`'s auto-retry
    /// — both start from a fresh PTY with `Launching` already set.
    fn spawn_and_monitor(&self, profile: ConnectionProfile) -> Result<()> {
        let session = match pty::spawn(
            &self.binary,
            &self.core_cwd,
            profile.clone(),
            self.log_tx.clone(),
        ) {
            Ok(session) => session,
            Err(e) => {
                // Must not leave the state machine stuck in Launching with no
                // process behind it. A spawn failure is an OS/environment
                // problem (not a network drop), so it is deliberately not
                // auto-retried — retrying blindly would mask a real setup
                // issue.
                self.set_state(ConnectionState::Error {
                    message: e.to_string(),
                    phase: "launching".into(),
                });
                return Err(e);
            }
        };
        orphan::write_pid(&self.state_dir, session.pid());

        {
            let mut inner = self.inner.lock().unwrap();
            inner.session = Some(session);
            inner.user_requested_stop = false;
        }

        let this = self.clone();
        std::thread::spawn(move || this.monitor_connect(profile));
        Ok(())
    }

    fn monitor_connect(&self, profile: ConnectionProfile) {
        let deadline = Instant::now() + status::connect_timeout(&profile.scan_mode);
        let socks = status::parse_bind_address(&profile.bind_address);
        let mut announced_connecting = false;

        loop {
            std::thread::sleep(Duration::from_millis(400));
            let mut inner = self.inner.lock().unwrap();
            if inner.user_requested_stop {
                return;
            }

            if let Some(exit) = inner.session.as_mut().and_then(|s| s.try_wait()) {
                inner.session = None;
                drop(inner);
                self.handle_unexpected_failure(
                    profile,
                    format!("Aether exited before connecting ({exit})"),
                    "connecting",
                );
                return;
            }

            if !announced_connecting {
                let done = inner
                    .session
                    .as_ref()
                    .map(|s| s.prompts_done())
                    .unwrap_or(false);
                if done {
                    inner.state = ConnectionState::Connecting;
                    let new_state = inner.state.clone();
                    drop(inner);
                    let _ = self.status_tx.send(StatusEvent::StateChange(new_state));
                    announced_connecting = true;
                    continue;
                }
            }

            if status::port_is_live(&socks) {
                let new_state = ConnectionState::Connected {
                    socks_addr: profile.bind_address.clone(),
                    connected_at_ms: now_millis(),
                };
                inner.state = new_state.clone();
                // Proven working — a future drop earns a fresh full retry
                // budget rather than inheriting whatever it took to get here.
                inner.retry_count = 0;
                drop(inner);
                let _ = self.status_tx.send(StatusEvent::StateChange(new_state));
                // Only persisted as "last successful" once actually proven to
                // work, never on a mere attempt.
                self.persist_successful_profile(&profile);
                self.monitor_connected(profile);
                return;
            }

            if Instant::now() >= deadline {
                if let Some(session) = inner.session.as_mut() {
                    session.kill();
                }
                inner.session = None;
                drop(inner);
                self.handle_unexpected_failure(
                    profile,
                    "Timed out waiting for Aether to find a working route".into(),
                    "connecting",
                );
                return;
            }
        }
    }

    /// Watches an established connection purely for an unexpected process
    /// exit — nothing else needs polling once Connected is reached.
    fn monitor_connected(&self, profile: ConnectionProfile) {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let mut inner = self.inner.lock().unwrap();
            if inner.user_requested_stop {
                return;
            }
            if let Some(exit) = inner.session.as_mut().and_then(|s| s.try_wait()) {
                inner.session = None;
                drop(inner);
                self.handle_unexpected_failure(
                    profile,
                    format!("Lost connection unexpectedly ({exit})"),
                    "connected",
                );
                return;
            }
        }
    }

    /// Common landing spot for every unexpected failure that was NOT a
    /// user-requested disconnect. Retries with backoff up to
    /// `MAX_AUTO_RETRIES` before giving up with a real `Error` — this turns a
    /// mid-session drop into a brief, visible "Reconnecting" instead of
    /// dumping the user back to Idle.
    fn handle_unexpected_failure(
        &self,
        profile: ConnectionProfile,
        failure_message: String,
        phase: &'static str,
    ) {
        let attempt = {
            let mut inner = self.inner.lock().unwrap();
            if inner.user_requested_stop {
                // request_disconnect is already handling this exit; don't
                // race it with a retry or an Error it didn't ask for.
                return;
            }
            inner.session = None;
            inner.retry_count += 1;
            inner.retry_count
        };
        orphan::clear_pid(&self.state_dir);

        if attempt > status::MAX_AUTO_RETRIES {
            self.set_state(ConnectionState::Error {
                message: format!(
                    "{failure_message} (gave up after {} retries)",
                    status::MAX_AUTO_RETRIES
                ),
                phase: phase.into(),
            });
            return;
        }

        self.set_state(ConnectionState::Reconnecting {
            attempt,
            max_attempts: status::MAX_AUTO_RETRIES,
        });

        let backoff = status::RETRY_BACKOFF[(attempt - 1) as usize];
        let this = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(backoff);
            if this.inner.lock().unwrap().user_requested_stop {
                return;
            }
            this.set_state(ConnectionState::Launching);
            // spawn_and_monitor lands its own failure in Error — nothing
            // further to do with its Result here.
            let _ = this.spawn_and_monitor(profile);
        });
    }

    pub fn request_disconnect(&self) -> Result<()> {
        let had_session = {
            let mut inner = self.inner.lock().unwrap();
            // Reconnecting has no live session (the old one already exited;
            // the retry's replacement hasn't spawned yet) — still a valid
            // thing to cancel, there is just nothing to send Ctrl-C to.
            let reconnecting = matches!(inner.state, ConnectionState::Reconnecting { .. });
            if inner.session.is_none() && !reconnecting {
                return Err(anyhow!("no active connection"));
            }
            inner.user_requested_stop = true;
            inner.retry_count = 0;
            if let Some(session) = inner.session.as_ref() {
                session.send_ctrl_c();
            }
            inner.session.is_some()
        };

        if !had_session {
            // Mid-backoff: the retry thread checks user_requested_stop before
            // respawning, so setting the flag is enough.
            self.set_state(ConnectionState::Idle);
            return Ok(());
        }

        self.set_state(ConnectionState::Disconnecting);

        let this = self.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + status::GRACEFUL_SHUTDOWN_GRACE;
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let mut inner = this.inner.lock().unwrap();
                let exited = inner.session.as_mut().and_then(|s| s.try_wait()).is_some();
                if exited || Instant::now() >= deadline {
                    if !exited && let Some(session) = inner.session.as_mut() {
                        session.kill();
                    }
                    inner.session = None;
                    inner.user_requested_stop = false;
                    drop(inner);
                    orphan::clear_pid(&this.state_dir);
                    this.set_state(ConnectionState::Idle);
                    return;
                }
            }
        });

        Ok(())
    }

    /// Supplies the Cloudflare Access one-time code requested during a Zero
    /// Trust email enrolment. Deliberately narrow rather than a generic PTY
    /// write, so no caller can inject arbitrary terminal input into the core.
    pub fn submit_access_code(&self, code: &str) -> Result<()> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| anyhow!("Aether state is unavailable"))?;
        let session = inner
            .session
            .as_ref()
            .ok_or_else(|| anyhow!("no active connection"))?;
        session.send_access_code(code)
    }

    fn persist_successful_profile(&self, profile: &ConnectionProfile) {
        let mut config = Config::load().unwrap_or_default();
        config.profile = profile.clone();
        if let Err(e) = config.save() {
            error!("could not persist profile: {e}");
        }
    }

    /// Blocks briefly to tear the core down; used on quit, where spawning a
    /// thread and emitting events nobody will receive is pointless.
    pub fn shutdown_blocking(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(session) = inner.session.as_mut() {
            session.send_ctrl_c();
            std::thread::sleep(Duration::from_millis(500));
            session.kill();
        }
        inner.session = None;
        inner.state = ConnectionState::Idle;
        drop(inner);
        orphan::clear_pid(&self.state_dir);
    }
}

/// `<exe_dir>/aether` → `settings.core_path` → `$PATH`, per the plan.
fn resolve_binary(config: &Config) -> Result<PathBuf> {
    let name = if cfg!(windows) {
        "aether.exe"
    } else {
        "aether"
    };

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Ok(ensure_executable(candidate));
        }
    }

    if let Some(configured) = &config.settings.core_path {
        if configured.exists() {
            return Ok(ensure_executable(configured.clone()));
        }
        return Err(anyhow!(
            "core_path points at {}, which does not exist",
            configured.display()
        ));
    }

    if let Ok(found) = which::which(name) {
        return Ok(found);
    }

    Err(anyhow!(
        "Aether binary not found. Place `{name}` next to this executable, set \
         `core_path` in config.toml, or put it on $PATH."
    ))
}

/// Bundlers don't reliably preserve the exec bit on resource files, and a
/// non-executable core would fail every spawn with a cryptic error.
fn ensure_executable(path: PathBuf) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_resting_states_allow_settings_changes() {
        assert!(ConnectionState::Idle.is_resting());
        assert!(
            ConnectionState::Error {
                message: "x".into(),
                phase: "connecting".into()
            }
            .is_resting()
        );
        assert!(!ConnectionState::Launching.is_resting());
        assert!(!ConnectionState::Connecting.is_resting());
        assert!(!ConnectionState::Disconnecting.is_resting());
        assert!(
            !ConnectionState::Connected {
                socks_addr: "127.0.0.1:1819".into(),
                connected_at_ms: 0
            }
            .is_resting()
        );
    }

    #[test]
    fn retry_backoff_covers_every_permitted_attempt() {
        // handle_unexpected_failure indexes RETRY_BACKOFF[attempt - 1] for
        // attempt in 1..=MAX_AUTO_RETRIES; a shorter table would panic.
        assert_eq!(
            status::RETRY_BACKOFF.len(),
            status::MAX_AUTO_RETRIES as usize
        );
    }

    #[test]
    fn missing_binary_is_reported_not_silently_defaulted() {
        let mut config = Config::default();
        config.settings.core_path = Some(PathBuf::from("/nonexistent/aether-binary"));
        let err = resolve_binary(&config).unwrap_err().to_string();
        assert!(err.contains("does not exist"), "{err}");
    }
}
