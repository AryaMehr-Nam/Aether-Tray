//! Headless supervisor: no tray, no dialogs, logs to stdout.
//!
//! Used by `--no-tray` and as the fallback when `connect` finds no tray
//! process listening.

use anyhow::{Result, anyhow};
use std::io::IsTerminal;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use crate::config::Config;
use crate::core::profile::ZeroTrustAuth;
use crate::core::{Command, ConnectionState, StatusEvent, Supervisor};
use crate::ipc::Overrides;

/// Emitted by the PTY reader when Cloudflare Access wants a one-time code.
const ACCESS_CODE_MARKER: &str = "Zero Trust access code required";

pub fn run(overrides: Option<Overrides>) -> Result<()> {
    let mut config = Config::load().unwrap_or_default();

    if let Some(overrides) = &overrides {
        overrides
            .apply(&mut config.profile)
            .map_err(|e| anyhow!(e))?;
    }

    // There is no dialog without a tray, so an email enrolment would need to
    // read the code from the terminal. Refuse up front when that is
    // impossible, naming the two auth methods that need no round-trip.
    let needs_code = !config.profile.zero_trust_team.trim().is_empty()
        && config.profile.zero_trust_auth == ZeroTrustAuth::Email;
    if needs_code && !std::io::stdin().is_terminal() {
        return Err(anyhow!(
            "Zero Trust email sign-in needs an interactive terminal to enter the \
             one-time code. Use a service token or an access token instead, both \
             of which authenticate without a prompt."
        ));
    }

    let handles = Supervisor::start(&config)?;
    let commands = handles.commands.clone();
    ctrlc::set_handler(move || {
        let _ = commands.send(Command::Shutdown);
    })?;

    handles
        .commands
        .send(Command::connect(config.profile.clone()))?;

    let stdin_is_tty = std::io::stdin().is_terminal();
    // Geo resolves asynchronously after Connected; print it once per tunnel.
    let mut geo_printed = false;
    loop {
        // Logs and status arrive on separate channels; poll both rather than
        // blocking on either, so neither can starve the other.
        match handles.logs.recv_timeout(Duration::from_millis(100)) {
            Ok(log) => {
                println!("{}", log.line);
                if stdin_is_tty
                    && log.line.contains(ACCESS_CODE_MARKER)
                    && let Some(code) = read_code_from_terminal()?
                {
                    handles.commands.send(Command::SubmitCode { code })?;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The supervisor is gone; nothing further will arrive.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }

        while let Ok(StatusEvent::StateChange(state)) = handles.status.try_recv() {
            eprintln!("[state] {}", state.label());
            match state {
                ConnectionState::Connected { .. } => geo_printed = false,
                ConnectionState::Error { .. } => {
                    // Retries are exhausted by the time Error is reached.
                    return Err(anyhow!("{}", state.label()));
                }
                _ => {}
            }
        }
        if !geo_printed && let Some(info) = handles.supervisor.geo_state().info {
            println!("[connection] {}", info.summary());
            geo_printed = true;
        }
    }
}

fn read_code_from_terminal() -> Result<Option<String>> {
    eprint!("Enter the Cloudflare Access code: ");
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let code = line.trim().to_string();
    Ok(if code.is_empty() { None } else { Some(code) })
}
