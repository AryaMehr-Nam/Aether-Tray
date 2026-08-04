//! The IPC server, run inside the tray process.

use anyhow::{Result, anyhow};
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, Stream, ToNsName};
use log::{debug, error, warn};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::Sender;

use super::{Request, Response, SOCKET_NAME};
use crate::core::{Command, Supervisor};

/// `overwrite` displaces whatever holds the name. Only ever set once a probe
/// has established that nothing is listening — see `run`.
fn bind_named(name: &str, overwrite: bool) -> std::io::Result<Listener> {
    let name = name.to_ns_name::<GenericNamespaced>()?;
    ListenerOptions::new()
        .name(name)
        .try_overwrite(overwrite)
        .create_sync()
}

fn bind() -> std::io::Result<Listener> {
    bind_named(SOCKET_NAME, false)
}

fn bind_reclaiming() -> std::io::Result<Listener> {
    bind_named(SOCKET_NAME, true)
}

/// Binds the socket and serves requests on a background thread.
///
/// A crashed process (SIGKILL, panic, power loss) leaves the socket file
/// behind on Unix, and every later bind then fails with `AddrInUse` — the CLI
/// would stay permanently broken until someone deleted the file by hand. But
/// `AddrInUse` is also what a genuinely running second instance produces, and
/// displacing that one would leave two supervisors fighting over the SOCKS
/// port. The two cases are only distinguishable by asking: a live server
/// answers a `Status` request, a stale file does not.
pub fn run(supervisor: Supervisor, commands: Sender<Command>) -> Result<()> {
    let listener = match bind() {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if super::client::send(&Request::Status)
                .ok()
                .flatten()
                .is_some()
            {
                return Err(anyhow!(
                    "another aether-tray process is already running and serving the CLI socket"
                ));
            }
            // Nothing answered, so the name is a corpse — reclaim it.
            warn!("reclaiming a stale CLI socket left by a previous crash");
            bind_reclaiming()?
        }
        Err(e) => return Err(e.into()),
    };

    std::thread::spawn(move || {
        for incoming in listener {
            match incoming {
                Ok(stream) => {
                    if let Err(e) = handle(stream, &supervisor, &commands) {
                        debug!("ipc client: {e}");
                    }
                }
                Err(e) => error!("ipc accept failed: {e}"),
            }
        }
    });
    Ok(())
}

fn handle(stream: Stream, supervisor: &Supervisor, commands: &Sender<Command>) -> Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(request) => dispatch(request, supervisor, commands),
        Err(e) => Response::Error {
            message: format!("malformed request: {e}"),
        },
    };

    let mut stream = &stream;
    stream.write_all(serde_json::to_string(&response)?.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn dispatch(request: Request, supervisor: &Supervisor, commands: &Sender<Command>) -> Response {
    match request {
        Request::Status => Response::Status {
            state: supervisor.state(),
            geo: supervisor.geo_state().info.map(Box::new),
        },
        Request::Disconnect => match commands.send(Command::Disconnect) {
            Ok(()) => Response::Ok {
                message: "Disconnecting".into(),
            },
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        Request::Connect { overrides } => {
            let mut profile = crate::config::Config::load().unwrap_or_default().profile;
            if let Err(message) = overrides.apply(&mut profile) {
                return Response::Error { message };
            }
            match commands.send(Command::connect(profile)) {
                Ok(()) => Response::Ok {
                    message: "Connecting".into(),
                },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distinct per test so concurrent runs cannot collide on one name.
    fn unique(tag: &str) -> String {
        format!("aether-tray-test-{tag}-{}.sock", std::process::id())
    }

    #[test]
    fn a_second_bind_to_a_live_name_is_refused() {
        let name = unique("live");
        let _held = bind_named(&name, false).expect("first bind");
        let err = bind_named(&name, false).expect_err("second bind must fail");
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    }

    #[test]
    fn overwrite_reclaims_a_name_that_is_only_a_corpse() {
        let name = unique("stale");
        // Dropping the listener without unlinking is what a SIGKILL leaves
        // behind on Unix: the name persists with nothing serving it.
        let held = bind_named(&name, false).expect("first bind");
        std::mem::forget(held);

        // Plain bind stays blocked...
        if let Err(e) = bind_named(&name, false) {
            assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);
            // ...but reclaiming succeeds, which is what stops the CLI from
            // being permanently broken after a crash.
            bind_named(&name, true).expect("reclaim a stale name");
        }
    }
}
