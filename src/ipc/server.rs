//! The IPC server, run inside the tray process.

use anyhow::Result;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};
use log::{debug, error};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::Sender;

use super::{Request, Response, SOCKET_NAME};
use crate::core::{Command, Supervisor};

/// Binds the socket and serves requests on a background thread.
///
/// `try_overwrite` is deliberately NOT set: if another tray process is
/// already listening, this must fail loudly rather than silently displace the
/// running instance and leave two supervisors fighting over the SOCKS port.
pub fn run(supervisor: Supervisor, commands: Sender<Command>) -> Result<()> {
    let name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_sync()?;

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
