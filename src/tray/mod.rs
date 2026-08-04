//! The tray event loop. Owns the main thread, because `tray-icon` requires
//! that on macOS and it is the safest assumption on all three platforms.
//!
//! Dialogs are opened from here rather than from the supervisor: on macOS
//! they must run on the main thread, so the access-code prompt arrives as a
//! marker log line and the reply goes back over the command channel.

pub mod icons;
pub mod menu;

use anyhow::Result;
use log::{error, warn};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use tray_icon::{TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

use crate::config::Config;
use crate::core::profile::ZeroTrustAuth;
use crate::core::{Command, ConnectionState, LogEvent, StatusEvent, Supervisor};
use crate::dialog;
use crate::notify;
use menu::MenuAction;

/// Emitted by the PTY reader when Cloudflare Access wants a one-time code.
const ACCESS_CODE_MARKER: &str = "Zero Trust access code required";

/// How often the loop wakes to drain the supervisor channels.
const TICK: Duration = Duration::from_millis(200);

pub fn run() -> Result<()> {
    let config = Config::load().unwrap_or_else(|e| {
        warn!("using default config: {e}");
        Config::default()
    });
    let handles = Supervisor::start(&config)?;

    // The CLI talks to this process over a local socket. A bind failure is
    // not fatal: the tray is still fully usable, only `aether-tray status`
    // and friends stop working.
    if let Err(e) = crate::ipc::server::run(handles.supervisor.clone(), handles.commands.clone()) {
        warn!("CLI socket unavailable (another instance may be running): {e}");
    }

    let mut app = TrayApp {
        tray: None,
        commands: handles.commands,
        status: handles.status,
        logs: handles.logs,
        supervisor: handles.supervisor,
        state: ConnectionState::Idle,
        log_file: Config::log_path()
            .ok()
            .and_then(|p| OpenOptions::new().create(true).append(true).open(p).ok()),
        code_prompt_open: false,
        interrupt: Arc::new(AtomicBool::new(false)),
        config,
    };

    // Launch-at-login lives in the OS, not in config.toml — if the user
    // removed the LaunchAgent/registry entry behind our back, believing our
    // own stale copy would show a wrong checkmark.
    let actual = crate::autostart::is_enabled();
    if actual != app.config.settings.launch_at_login {
        app.config.settings.launch_at_login = actual;
        let _ = app.config.save();
    }

    if !dialog::available() {
        warn!(
            "no dialog helper found (install zenity or kdialog); free-text \
             settings must be edited via \"Edit config.toml…\""
        );
    }

    // A tray app is normally quit from its menu, but it can still be started
    // from a shell or stopped by a service manager. Without this, Ctrl-C would
    // kill us while the core kept running and holding the SOCKS port.
    {
        let interrupt = Arc::clone(&app.interrupt);
        if let Err(e) = ctrlc::set_handler(move || interrupt.store(true, Ordering::Relaxed)) {
            warn!("could not install the Ctrl-C handler: {e}");
        }
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + TICK));
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct TrayApp {
    tray: Option<TrayIcon>,
    commands: Sender<Command>,
    status: Receiver<StatusEvent>,
    logs: Receiver<LogEvent>,
    supervisor: Supervisor,
    config: Config,
    state: ConnectionState,
    log_file: Option<File>,
    code_prompt_open: bool,
    /// Set from the Ctrl-C handler; polled by the event loop because a signal
    /// handler must not touch the supervisor or the tray directly.
    interrupt: Arc<AtomicBool>,
}

impl TrayApp {
    fn build_tray(&self) -> Result<TrayIcon> {
        let menu = menu::build(&self.state, &self.config)?;
        let tray = TrayIconBuilder::new()
            .with_icon(icons::for_state(&self.state))
            .with_tooltip(format!("Aether — {}", self.state.label()))
            .with_menu(Box::new(menu))
            .build()?;
        Ok(tray)
    }

    fn rebuild_menu(&self) {
        let Some(tray) = &self.tray else { return };
        match menu::build(&self.state, &self.config) {
            Ok(m) => tray.set_menu(Some(Box::new(m))),
            Err(e) => error!("could not rebuild menu: {e}"),
        }
    }

    fn apply_state(&mut self, new_state: ConnectionState) {
        let previous = std::mem::replace(&mut self.state, new_state);
        if let Some(tray) = &self.tray {
            let _ = tray.set_icon(Some(icons::for_state(&self.state)));
            let _ = tray.set_tooltip(Some(format!("Aether — {}", self.state.label())));
        }
        self.rebuild_menu();

        if self.config.settings.notifications {
            self.notify_transition(&previous);
        }
    }

    /// Only the transitions a user cares about, and only on *entering* them —
    /// the retry loop re-enters Launching repeatedly and must not spam.
    fn notify_transition(&self, previous: &ConnectionState) {
        let message = match (&previous, &self.state) {
            (_, ConnectionState::Connected { socks_addr, .. })
                if !matches!(previous, ConnectionState::Connected { .. }) =>
            {
                Some(("Connected".to_string(), format!("SOCKS5 on {socks_addr}")))
            }
            (ConnectionState::Connected { .. }, ConnectionState::Reconnecting { .. }) => {
                Some(("Connection lost".to_string(), "Retrying…".to_string()))
            }
            (_, ConnectionState::Error { message, .. })
                if !matches!(previous, ConnectionState::Error { .. }) =>
            {
                Some(("Aether error".to_string(), message.clone()))
            }
            _ => None,
        };
        if let Some((title, body)) = message
            && let Err(e) = notify::send(&title, &body)
        {
            warn!("notification failed: {e}");
        }
    }

    fn drain_channels(&mut self) {
        while let Ok(StatusEvent::StateChange(state)) = self.status.try_recv() {
            self.apply_state(state);
        }
        while let Ok(log) = self.logs.try_recv() {
            if let Some(file) = self.log_file.as_mut() {
                let _ = writeln!(file, "{} {}", log.timestamp, log.line);
            }
            // The core asks for the Access one-time code without a trailing
            // newline, so the reader emits this marker instead. Prompt once.
            if log.line.contains(ACCESS_CODE_MARKER) && !self.code_prompt_open {
                self.code_prompt_open = true;
                self.prompt_for_access_code();
                self.code_prompt_open = false;
            }
        }
    }

    fn prompt_for_access_code(&self) {
        match dialog::prompt_secret(
            "Aether — Zero Trust",
            "Enter the one-time code Cloudflare Access emailed you:",
        ) {
            Some(code) if !code.trim().is_empty() => {
                let _ = self.commands.send(Command::SubmitCode { code });
            }
            _ => {
                // Declining leaves the core waiting; a disconnect is the only
                // clean way out of a half-finished enrolment.
                let _ = self.commands.send(Command::Disconnect);
            }
        }
    }

    fn save_config(&mut self) {
        if let Err(e) = self.config.save() {
            error!("could not save config: {e}");
        }
        self.rebuild_menu();
    }

    /// Profile edits are refused unless resting: the core reads its whole
    /// profile once at spawn, so a mid-connection change would appear to
    /// apply but silently not take effect until the next connect.
    fn edit_profile(&mut self, f: impl FnOnce(&mut crate::core::profile::ConnectionProfile)) {
        if !self.state.is_resting() {
            return;
        }
        f(&mut self.config.profile);
        self.save_config();
    }

    fn handle(&mut self, action: MenuAction, event_loop: &ActiveEventLoop) {
        match action {
            MenuAction::Toggle => match self.state {
                ConnectionState::Idle | ConnectionState::Error { .. } => {
                    let _ = self
                        .commands
                        .send(Command::connect(self.config.profile.clone()));
                }
                _ => {
                    let _ = self.commands.send(Command::Disconnect);
                }
            },
            MenuAction::CopyProxy => self.copy_proxy_address(),
            MenuAction::ShowLogs => {
                if let Ok(path) = Config::log_path() {
                    open_path(&path);
                }
            }
            MenuAction::OpenConfig => {
                // The documented fallback when no dialog helper exists.
                if let Ok(path) = Config::config_path() {
                    let _ = self.config.save();
                    open_path(&path);
                }
            }
            MenuAction::Quit => {
                let _ = self.commands.send(Command::Shutdown);
                self.supervisor.shutdown_blocking();
                event_loop.exit();
            }

            MenuAction::SetProtocol(v) => self.edit_profile(|p| p.protocol = v),
            MenuAction::SetScanMode(v) => self.edit_profile(|p| p.scan_mode = v),
            MenuAction::SetIpVersion(v) => self.edit_profile(|p| p.ip_version = v),
            MenuAction::SetMasqueNoize(v) => self.edit_profile(|p| p.masque_noize = v),
            MenuAction::SetWgNoize(v) => self.edit_profile(|p| p.wg_noize = v),
            MenuAction::SetZeroTrustAuth(v) => self.edit_profile(|p| p.zero_trust_auth = v),
            MenuAction::ToggleQuickReconnect => {
                self.edit_profile(|p| p.quick_reconnect = !p.quick_reconnect)
            }
            MenuAction::ToggleMasqueHttp2 => {
                self.edit_profile(|p| p.masque_http2 = !p.masque_http2)
            }
            MenuAction::ToggleZeroTrustGateway => {
                self.edit_profile(|p| p.zero_trust_gateway = !p.zero_trust_gateway)
            }

            MenuAction::ToggleNotifications => {
                self.config.settings.notifications = !self.config.settings.notifications;
                self.save_config();
            }
            MenuAction::ToggleConnectOnLaunch => {
                self.config.settings.connect_on_launch = !self.config.settings.connect_on_launch;
                self.save_config();
            }
            MenuAction::ToggleLaunchAtLogin => {
                let enable = !self.config.settings.launch_at_login;
                match crate::autostart::set_enabled(enable) {
                    Ok(()) => {
                        self.config.settings.launch_at_login = enable;
                        self.save_config();
                    }
                    Err(e) => error!("could not change launch-at-login: {e}"),
                }
            }

            MenuAction::EditBindAddress => self.edit_text(
                "Listen address",
                "SOCKS5 listen address (host:port). Use 0.0.0.0 to allow LAN clients.",
                |p| p.bind_address.clone(),
                |p, v| p.bind_address = v,
            ),
            MenuAction::EditDns => self.edit_text(
                "DNS",
                "Resolvers used inside the tunnel, comma separated. Blank for default.",
                |p| p.dns.clone(),
                |p, v| p.dns = v,
            ),
            MenuAction::EditRouteBlock => self.edit_text(
                "Block list",
                "Domains/CIDRs to block, comma separated.",
                |p| p.route_block.clone(),
                |p, v| p.route_block = v,
            ),
            MenuAction::EditRouteDirect => self.edit_text(
                "Direct list",
                "Domains/CIDRs to route outside the tunnel, comma separated.",
                |p| p.route_direct.clone(),
                |p, v| p.route_direct = v,
            ),
            MenuAction::EditZeroTrustTeam => self.edit_text(
                "Zero Trust team",
                "Cloudflare Zero Trust organization name. Blank for consumer WARP.",
                |p| p.zero_trust_team.clone(),
                |p, v| p.zero_trust_team = v,
            ),
            MenuAction::PickRoutesFile => {
                if !self.state.is_resting() {
                    return;
                }
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.edit_profile(|p| p.routes_file = path.display().to_string());
                }
            }
            MenuAction::EditZeroTrustCredentials => self.edit_credentials(),
        }
    }

    fn edit_text(
        &mut self,
        title: &str,
        message: &str,
        get: impl Fn(&crate::core::profile::ConnectionProfile) -> String,
        set: impl FnOnce(&mut crate::core::profile::ConnectionProfile, String),
    ) {
        if !self.state.is_resting() {
            return;
        }
        let current = get(&self.config.profile);
        if let Some(value) = dialog::prompt_text(title, message, Some(&current)) {
            self.edit_profile(|p| set(p, value.trim().to_string()));
        }
    }

    /// Credentials are never persisted (see `ConnectionProfile::scrubbed`), so
    /// these live in memory for this run only.
    fn edit_credentials(&mut self) {
        if !self.state.is_resting() {
            return;
        }
        match self.config.profile.zero_trust_auth {
            ZeroTrustAuth::Email => {
                let current = self.config.profile.access_email.clone();
                if let Some(v) = dialog::prompt_text(
                    "Zero Trust email",
                    "Email address for Cloudflare Access sign-in:",
                    Some(&current),
                ) {
                    self.config.profile.access_email = v.trim().to_string();
                }
            }
            ZeroTrustAuth::Service => {
                let id = self.config.profile.access_client_id.clone();
                if let Some(v) = dialog::prompt_text(
                    "Service token",
                    "Access service token client id:",
                    Some(&id),
                ) {
                    self.config.profile.access_client_id = v.trim().to_string();
                }
                if let Some(v) =
                    dialog::prompt_secret("Service token", "Access service token client secret:")
                {
                    self.config.profile.access_client_secret = v.trim().to_string();
                }
            }
            ZeroTrustAuth::Token => {
                if let Some(v) =
                    dialog::prompt_secret("Access token", "Pre-obtained Access enrolment JWT:")
                {
                    self.config.profile.access_token = v.trim().to_string();
                }
            }
        }
        // Deliberately not saved to disk — only the menu summary refreshes.
        self.rebuild_menu();
    }

    /// One iteration of periodic work: drain the supervisor, service menu
    /// clicks, honour an interrupt, then arm the next wakeup.
    fn pump(&mut self, event_loop: &ActiveEventLoop) {
        self.drain_channels();

        // muda delivers menu clicks on its own channel, not through winit.
        while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            if let Some(action) = menu::action_for(&event.id) {
                self.handle(action, event_loop);
            }
        }

        if self.interrupt.load(Ordering::Relaxed) {
            // Same path as the Quit menu item, so the core is torn down
            // rather than left holding the SOCKS port.
            let _ = self.commands.send(Command::Shutdown);
            self.supervisor.shutdown_blocking();
            event_loop.exit();
            return;
        }

        // Re-arm on every pass: WaitUntil is a one-shot deadline, so without
        // this the loop would park indefinitely after the first wakeup.
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + TICK));
    }

    fn copy_proxy_address(&self) {
        let addr = self.config.profile.bind_address.clone();
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(addr)) {
            Ok(()) => {}
            Err(e) => warn!("clipboard unavailable: {e}"),
        }
    }
}

impl ApplicationHandler for TrayApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        _event: winit::event::WindowEvent,
    ) {
        // No windows: this app is tray-only.
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        // The tray must be created after the event loop is running, or macOS
        // drops it silently.
        if self.tray.is_none() && matches!(cause, winit::event::StartCause::Init) {
            match self.build_tray() {
                Ok(tray) => self.tray = Some(tray),
                Err(e) => {
                    // Nothing to fall back to: on Linux this is the "no
                    // StatusNotifierItem host" case, which needs a GNOME
                    // AppIndicator extension. Point at --no-tray.
                    error!("could not create tray icon: {e}");
                    error!("if this desktop has no system tray, run with --no-tray");
                    event_loop.exit();
                    return;
                }
            }
            if self.config.settings.connect_on_launch {
                let _ = self
                    .commands
                    .send(Command::connect(self.config.profile.clone()));
            }
        }

        self.pump(event_loop);
    }

    /// Runs after every batch of events, including the timer wakeups from
    /// `ControlFlow::WaitUntil`. All periodic work lives here rather than in
    /// `new_events` so it still happens on an idle desktop: this app owns no
    /// window, so without a scheduled wakeup the loop would park forever and
    /// never poll the supervisor channels or the interrupt flag.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.pump(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.supervisor.shutdown_blocking();
    }
}

fn open_path(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let opener = "xdg-open";

    if let Err(e) = std::process::Command::new(opener).arg(path).spawn() {
        warn!("could not open {}: {e}", path.display());
    }
}
