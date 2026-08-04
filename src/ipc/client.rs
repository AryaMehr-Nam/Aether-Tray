//! The CLI client half: talks to a running tray process over the local
//! socket, or falls back to running headless when there isn't one.

use anyhow::{Result, anyhow};
use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{GenericNamespaced, Stream, ToNsName};
use std::io::{BufRead, BufReader, Write};

use super::{Request, Response, SOCKET_NAME};

/// Sends one request and returns the response, or `None` when no tray
/// process is listening.
pub fn send(request: &Request) -> Result<Option<Response>> {
    let name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let stream = match Stream::connect(name) {
        Ok(stream) => stream,
        // Nothing listening: the caller decides whether that's fatal.
        Err(e) if is_not_running(&e) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let mut writer = &stream;
    writer.write_all(serde_json::to_string(request)?.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(anyhow!(
            "tray process closed the connection without replying"
        ));
    }
    Ok(Some(serde_json::from_str(line.trim())?))
}

/// Distinguishes "no server" from a real transport failure. Both socket
/// families report absence differently, so both spellings are matched.
fn is_not_running(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::AddrNotAvailable
    )
}
