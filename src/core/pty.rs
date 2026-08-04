use super::profile::{ConnectionProfile, ZeroTrustAuth};
use super::prompts::{PROMPT_TABLE, looks_like_choice_prompt};
use crate::core::{LogEvent, now_millis};
use anyhow::{Result, anyhow};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub struct PtySession {
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    prompts_done: Arc<AtomicBool>,
    _master: Box<dyn MasterPty + Send>,
}

impl PtySession {
    pub fn pid(&self) -> u32 {
        self.child.process_id().unwrap_or(0)
    }

    pub fn prompts_done(&self) -> bool {
        self.prompts_done.load(Ordering::Relaxed)
    }

    pub fn try_wait(&mut self) -> Option<portable_pty::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Ctrl-C (ETX) — the same byte a real terminal sends for SIGINT. See
    /// core/status.rs::GRACEFUL_SHUTDOWN_GRACE for why callers should follow
    /// this with only a short wait before `kill()`, not a long one.
    pub fn send_ctrl_c(&self) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(&[0x03]);
            let _ = w.flush();
        }
    }

    /// Feeds the one-time code requested by Cloudflare Access during a Zero
    /// Trust email enrolment. The code never enters the log stream.
    pub fn send_access_code(&self, code: &str) -> Result<()> {
        let code = code.trim();
        if code.is_empty() || code.len() > 512 || code.contains(['\r', '\n']) {
            return Err(anyhow!("invalid Zero Trust access code"));
        }
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| anyhow!("Aether input is unavailable"))?;
        writer
            .write_all(code.as_bytes())
            .and_then(|_| writer.write_all(b"\r\n"))
            .and_then(|_| writer.flush())
            .map_err(|e| anyhow!("sending Zero Trust access code: {e}"))?;
        Ok(())
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

/// Spawns Aether in a real PTY (not a plain piped subprocess) and answers its
/// known interactive prompts as they appear. A PTY is required because
/// interactive-prompt libraries typically check `isatty()` and behave
/// differently — or refuse to prompt at all — over a plain pipe.
///
/// `cwd` should be a stable, dedicated directory (the app's data dir): Aether
/// writes its provisioned identity (`aether-masque.toml` / `aether.toml`)
/// into its working directory, so this must stay consistent across launches
/// for that identity to persist rather than being silently re-provisioned
/// every run.
pub fn spawn(
    binary: &Path,
    cwd: &Path,
    profile: ConnectionProfile,
    log_tx: Sender<LogEvent>,
) -> Result<PtySession> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| anyhow!("Failed to open PTY: {}", e))?;

    let mut cmd = CommandBuilder::new(binary);
    cmd.cwd(cwd);
    for arg in profile.as_args() {
        cmd.arg(arg);
    }
    // Env var, not a flag (see ConnectionProfile::masque_http2's doc-comment):
    // any value suppresses Aether 1.2.0's interactive "MASQUE transport"
    // prompt, and only a truthy one selects HTTP/2.
    cmd.env(
        "AETHER_MASQUE_HTTP2",
        if profile.masque_http2 { "1" } else { "0" },
    );
    // Keep Access credentials out of the process command line. Aether's
    // flags and environment variables are equivalent, but command arguments
    // are trivially visible to other local processes on several platforms.
    //
    // Service tokens need TWO variables, which `zero_trust_env` cannot
    // express in one return value, so that pair is handled here directly.
    match profile.zero_trust_auth {
        ZeroTrustAuth::Service
            if !profile.access_client_id.trim().is_empty()
                && !profile.access_client_secret.trim().is_empty() =>
        {
            cmd.env("AETHER_ACCESS_CLIENT_ID", profile.access_client_id.trim());
            cmd.env(
                "AETHER_ACCESS_CLIENT_SECRET",
                profile.access_client_secret.trim(),
            );
        }
        _ => {
            if let Some((key, value)) = profile.zero_trust_env() {
                cmd.env(key, value);
            }
        }
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| anyhow!("Failed to spawn Aether: {}", e))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow!("Failed to clone PTY reader: {}", e))?;

    let raw_writer = pair
        .master
        .take_writer()
        .map_err(|e| anyhow!("Failed to take PTY writer: {}", e))?;
    let writer = Arc::new(Mutex::new(raw_writer));
    let writer_for_thread = Arc::clone(&writer);

    let prompts_done = Arc::new(AtomicBool::new(false));
    let prompts_done_for_thread = Arc::clone(&prompts_done);

    std::thread::spawn(move || {
        read_loop(
            reader.as_mut(),
            writer_for_thread,
            profile,
            log_tx,
            prompts_done_for_thread,
        );
    });

    Ok(PtySession {
        child,
        writer,
        prompts_done,
        _master: pair.master,
    })
}

fn read_loop(
    reader: &mut dyn Read,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    profile: ConnectionProfile,
    log_tx: Sender<LogEvent>,
    prompts_done: Arc<AtomicBool>,
) {
    let mut answered: HashSet<&'static str> = HashSet::new();
    let mut current_section: Option<&'static str> = None;
    let mut line_buf = String::new();
    let mut byte_buf = [0u8; 4096];
    let mut code_prompt_visible = false;

    loop {
        let n = match reader.read(&mut byte_buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        line_buf.push_str(&String::from_utf8_lossy(&byte_buf[..n]));

        // Emit every complete line, tracking which known prompt "section"
        // we're currently in (the last recognized header line wins — plain
        // log lines in between don't reset it).
        for raw_line in drain_lines(&mut line_buf) {
            let line = strip_ansi(&raw_line);
            if line.is_empty() {
                continue;
            }
            for rule in PROMPT_TABLE {
                if (rule.header_matches)(&line) {
                    current_section = Some(rule.id);
                    // Seeing a header again means Aether restarted its prompt
                    // sequence (its own stdin read timed out — see
                    // prompts.rs) — allow re-answering, or it blocks forever.
                    answered.remove(rule.id);
                }
            }
            let _ = log_tx.send(LogEvent {
                line,
                timestamp: now_millis(),
            });
        }

        // Whatever remains (no newline yet) is either more output still
        // arriving, or Aether blocking on stdin for the current section's
        // answer. A bare header as the partial ("Scan mode:") is output still
        // in flight — Aether always prints the menu + "Choose…: " before
        // blocking — so answering there would double-feed the next menu once
        // the header completes as a line and gets un-answered above.
        let partial = strip_ansi(&line_buf);
        // Aether 1.5.0 asks for the Cloudflare Access one-time code without a
        // terminating newline, so normal line forwarding cannot expose it.
        // Emit a single neutral signal line instead; the tray shows a code
        // dialog and calls `send_access_code` to reply.
        let access_code_prompt = partial.contains("Enter the code:");
        if access_code_prompt && !code_prompt_visible {
            let _ = log_tx.send(LogEvent {
                line: "[gui] Zero Trust access code required".into(),
                timestamp: now_millis(),
            });
        }
        code_prompt_visible = access_code_prompt;
        if looks_like_choice_prompt(&partial)
            && !PROMPT_TABLE.iter().any(|r| (r.header_matches)(&partial))
            && let Some(section) = current_section
            && !answered.contains(section)
            && let Some(rule) = PROMPT_TABLE.iter().find(|r| r.id == section)
        {
            let answer = (rule.answer)(&profile);
            if let Ok(mut w) = writer.lock() {
                let _ = w.write_all(answer.as_bytes());
                let _ = w.write_all(b"\r\n");
                let _ = w.flush();
            }
            let _ = log_tx.send(LogEvent {
                line: format!("[gui] answered {section} \u{2192} {answer}"),
                timestamp: now_millis(),
            });
            answered.insert(section);
            if answered.len() == PROMPT_TABLE.len() {
                prompts_done.store(true, Ordering::Relaxed);
            }
        }
    }
}

/// Longest the unterminated tail may grow before the front is discarded.
/// `strip_ansi` rescans the whole tail on every read, so an unbounded tail
/// (e.g. output that never emits a terminator) would be O(n²) CPU.
const MAX_PARTIAL: usize = 16 * 1024;

/// Drains and returns every terminated line in `buf`, leaving the
/// unterminated tail in place. Terminal semantics, not plain `\n`-splitting:
/// a `\r` (or ONLCR-style `\r\r`) run followed by `\n` ends a line, while a
/// `\r` run followed by anything else is a carriage-return overwrite — a
/// spinner/progress frame a terminal would repaint in place — so the
/// overwritten prefix is dead output and is dropped without being emitted.
/// A `\r` run touching the end of the buffer is kept: the `\n` half of a
/// `\r\n` may still be in flight.
fn drain_lines(buf: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(pos) = buf.find(['\r', '\n']) {
        let end = if buf.as_bytes()[pos] == b'\n' {
            pos
        } else {
            let mut run_end = pos;
            while run_end < buf.len() && buf.as_bytes()[run_end] == b'\r' {
                run_end += 1;
            }
            if run_end == buf.len() {
                break; // "\r" at buffer end: might be a split "\r\n"
            }
            if buf.as_bytes()[run_end] != b'\n' {
                buf.drain(..run_end); // overwritten frame: discard silently
                continue;
            }
            run_end
        };
        let line: String = buf.drain(..=end).collect();
        lines.push(line.trim_end_matches(['\r', '\n']).to_string());
    }
    if buf.len() > MAX_PARTIAL {
        let mut cut = buf.len() - MAX_PARTIAL;
        while !buf.is_char_boundary(cut) {
            cut += 1;
        }
        buf.drain(..cut);
    }
    lines
}

/// Aether's output includes ANSI color codes (e.g. `\x1b[32m`) around log
/// level names — stripped so header-line matching and the log panel both see
/// plain text. Minimal hand-rolled CSI-sequence stripper: no regex needed for
/// a single well-known pattern (`ESC [ ... letter`).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c2 in chars.by_ref() {
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(buf: &mut String, chunk: &str) -> Vec<String> {
        buf.push_str(chunk);
        drain_lines(buf)
    }

    #[test]
    fn plain_newlines() {
        let mut buf = String::new();
        assert_eq!(feed(&mut buf, "a\nb\nc"), ["a", "b"]);
        assert_eq!(buf, "c");
    }

    #[test]
    fn crlf_and_onlcr_double_cr() {
        let mut buf = String::new();
        assert_eq!(feed(&mut buf, "a\r\nb\r\r\n"), ["a", "b"]);
        assert_eq!(buf, "");
    }

    #[test]
    fn cr_overwrite_drops_spinner_frames() {
        let mut buf = String::new();
        assert_eq!(
            feed(&mut buf, "scan 1%\rscan 2%\rscan 3%"),
            Vec::<String>::new()
        );
        assert_eq!(buf, "scan 3%"); // only the live frame survives
        assert_eq!(feed(&mut buf, "\rscan done\n"), ["scan done"]);
        assert_eq!(buf, "");
    }

    #[test]
    fn lone_cr_at_end_waits_for_possible_lf() {
        let mut buf = String::new();
        assert_eq!(feed(&mut buf, "abc\r"), Vec::<String>::new());
        assert_eq!(buf, "abc\r");
        assert_eq!(feed(&mut buf, "\n"), ["abc"]); // the \r\n was split across reads
        assert_eq!(buf, "");
    }

    #[test]
    fn unterminated_tail_is_capped() {
        let mut buf = String::new();
        // Multibyte chars so the cap must respect char boundaries.
        let big = "é".repeat(MAX_PARTIAL); // 2 bytes each → 32 KiB, no terminators
        assert_eq!(feed(&mut buf, &big), Vec::<String>::new());
        assert!(buf.len() <= MAX_PARTIAL + 1);
        assert!(buf.chars().all(|c| c == 'é'));
    }
}
