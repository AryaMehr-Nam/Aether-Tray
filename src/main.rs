mod autostart;
mod config;
mod core;
mod dialog;
mod headless;
mod ipc;
mod notify;
mod tray;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::core::ConnectionState;
use ipc::{Overrides, Request, Response};

#[derive(Parser)]
#[command(name = "aether-tray", version, about = "Tray-only client for Aether")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Run without a tray icon, logging to stdout. Useful on servers and on
    /// desktops with no system tray.
    #[arg(long, global = true)]
    no_tray: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Connect, optionally overriding the saved profile for this run.
    Connect {
        #[arg(long)]
        protocol: Option<String>,
        #[arg(long)]
        scan_mode: Option<String>,
        #[arg(long)]
        ip_version: Option<String>,
        /// SOCKS5 listen address, host:port.
        #[arg(long)]
        bind: Option<String>,
    },
    /// Disconnect a running instance.
    Disconnect,
    /// Report the current state.
    Status {
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    match cli.command {
        None => {
            if cli.no_tray {
                headless::run(None)
            } else {
                tray::run()
            }
        }
        Some(Commands::Connect {
            protocol,
            scan_mode,
            ip_version,
            bind,
        }) => {
            let overrides = Overrides {
                protocol,
                scan_mode,
                ip_version,
                bind_address: bind,
            };
            connect(overrides, cli.no_tray)
        }
        Some(Commands::Disconnect) => simple(Request::Disconnect),
        Some(Commands::Status { json }) => status(json),
    }
}

/// With a tray running, this is a remote control. Without one, it becomes the
/// supervisor itself rather than failing — the plan's "connect starts a
/// headless supervisor in-process" behaviour.
fn connect(overrides: Overrides, force_headless: bool) -> Result<()> {
    if !force_headless
        && let Some(response) = ipc::client::send(&Request::Connect {
            overrides: overrides.clone(),
        })?
    {
        return report(response);
    }
    eprintln!("No tray process running — starting a headless supervisor (Ctrl-C to stop).");
    headless::run(Some(overrides))
}

fn simple(request: Request) -> Result<()> {
    match ipc::client::send(&request)? {
        Some(response) => report(response),
        None => {
            eprintln!("No Aether process is running.");
            std::process::exit(1);
        }
    }
}

fn status(json: bool) -> Result<()> {
    let (state, geo) = match ipc::client::send(&Request::Status)? {
        Some(Response::Status { state, geo }) => (Some(state), geo),
        Some(other) => return report(other),
        None => (None, None),
    };

    if json {
        // A machine-readable "not running" beats a non-zero exit with no
        // parseable body when --json was explicitly requested.
        let value = match &state {
            Some(state) => {
                let mut value = serde_json::to_value(state)?;
                // Geo only ever describes a live tunnel; never report it for
                // a disconnected (possibly raced) state.
                if matches!(state, ConnectionState::Connected { .. })
                    && let Some(geo) = &geo
                {
                    value["geo"] = serde_json::to_value(geo)?;
                }
                value
            }
            None => serde_json::json!({ "state": "not_running" }),
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    match state {
        Some(state) => {
            println!("{}", state.label());
            // The geo summary only describes a live tunnel, so it is never
            // printed for a stale or idle state.
            if matches!(state, ConnectionState::Connected { .. })
                && let Some(geo) = &geo
            {
                println!("{}", geo.summary());
            }
        }
        None => {
            println!("Not running");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn report(response: Response) -> Result<()> {
    match response {
        Response::Ok { message } => println!("{message}"),
        Response::Status { state, .. } => println!("{}", state.label()),
        Response::Error { message } => {
            eprintln!("Error: {message}");
            std::process::exit(1);
        }
    }
    Ok(())
}
