# Contributing to Aether Tray

Thanks for wanting to help! This document covers how to build, test, and — most
importantly — *not break* this project.

## How to contribute

- **Report bugs** as issues with the steps to reproduce, platform, and the log
  (`Show logs…` in the tray, or `aether-tray.log` in the config dir).
- **Suggest features** as issues before writing code — some ideas need design
  discussion (e.g. anything that touches the network stack or privacy).
- **Open PRs** for bug fixes and small, focused improvements. Large changes
  should start with an issue so we agree on the approach first.

## Getting started

```sh
git clone https://github.com/rendinjast/aether-tray
cd aether-tray
./binaries/fetch-aether.sh     # downloads the `aether` core binary
cargo build
```

The `aether` core is a separate executable, looked up in this order:
next to the tray binary → `core_path` in `config.toml` → `$PATH`. You only
need it to run the app, not to compile or test.

### Platform dependencies

| Platform | Requirements |
|---|---|
| Linux | GTK3, libayatana-appindicator, `libxdo-dev` (dev-time); a StatusNotifierItem host at runtime |
| macOS / Windows | None beyond a stable Rust toolchain |

## Development workflow

Every change must pass the project's three gates:

```sh
cargo test                          # all tests
cargo fmt --check                   # formatting
cargo clippy --all-targets -- -D warnings
```

Run all three before pushing. If you changed anything that touches the menu,
the IPC protocol, or the state machine, also run a manual smoke test
(`aether-tray connect` → `aether-tray status --json` → `disconnect`).

## The most important rule: `src/core/` is a near-verbatim port

`src/core/` was ported from the windowed Aether-GUI app, and several behaviours
there **encode bugs that hit someone in production**. They look like mistakes.
They are not. They are marked with comments explaining why, and they must not
be "cleaned up" without a reproduction that proves the cleanup is safe:

1. **Prompt headers match by suffix, not equality.** Aether sometimes glues a
   header onto the preceding log line with no newline (`...balancedScan mode:`).
   Exact matching starved that prompt of an answer and caused an *infinite
   Protocol/Scan-mode loop* in production.
2. **Re-seeing a header clears the answered flag.** When Aether's own stdin
   read times out, it restarts its prompt sequence; without the re-answer,
   the process blocks forever.
3. **A bare header as the partial buffer must not be answered.** The menu is
   still in flight; answering early double-feeds the next menu.
4. **"Connected" means the SOCKS5 port accepts a TCP connect** — never a log
   line. The core prints optimistic text well before the listener accepts.
5. **The core's working directory and our state directory are different by
   design.** The core cwd is shared with the windowed app (WARP identity
   persistence); our pid file lives in our own dir so orphan reaping can never
   kill a process that isn't ours. Do not "unify" them.

## Code conventions

### Comments explain *why*

This codebase comments decisions, not syntax. If you write something subtle,
leave the reasoning: the failure mode it prevents, the production bug it
encodes, the alternative you rejected. `///` doc comments on public items,
inline comments on tricky logic. If a comment would just restate the code,
omit it.

### Structure and naming

- Modules mirror the windowed app's layout where they overlap (`core/`).
- Menu items carry **stable string ids** and events are routed **by id, never
  by label** — labels are localised, decorated with state, and duplicated
  across radio groups.
- Cross-module helpers are `pub(crate)` and reused, not re-implemented
  (`status::probe_addr` serves both liveness checks and the geo lookup).
- Large enum payloads are boxed (`Command::Connect`'s profile,
  `Response::Status`'s geo) so every queued/transported value doesn't pay for
  the biggest variant.

### Concurrency

- The supervisor is driven by channels; connection progress happens on
  per-attempt monitor threads so a `Disconnect` is never queued behind a scan.
- **Never block the tray event loop.** Anything slow (notifications, HTTP
  lookups) runs on its own thread — name the threads (`notify`,
  `geo-lookup`).
- macOS dialogs must run on the main thread; the tray opens them and replies
  over the command channel.
- Long-running work needs timeouts. The geo lookup is fire-and-forget with
  its own budgets and must never disturb the connection it describes.
- Shared state goes under the supervisor's `Mutex`; result threads re-check
  the current state (keyed on `connected_at_ms`) before landing, so a slow
  thread can't clobber a newer tunnel's state.

### Secrets and privacy

- **Access credentials never reach disk.** `Config::save` scrubs them;
  profile persistence happens only on a proven `Connected`, never an attempt.
- Credentials go to the core as **env vars, never CLI flags** — arguments are
  visible to other local processes on several platforms.
- Never log a credential, an access code, or anything that might contain one.
- Network requests to third parties (the connection-info lookup) must be
  optional and off by a documented setting.

### Serialization

- Every profile field carries `#[serde(default)]` so a store written by an
  older version — or missing a key — migrates what it does have instead of
  collapsing to defaults.
- IPC is newline-delimited JSON; new fields get `#[serde(default)]` so old
  clients can still parse new responses.

### Dependencies

The dependency list is deliberately small. Before adding a crate, consider the
standard-library or hand-rolled alternative (see `pty.rs`'s ANSI stripper and
`status.rs`'s plain TCP probes). If a dependency is genuinely warranted, prefer
a focused one over a framework, and keep feature flags minimal.

### Tests

- Unit tests live next to the code they cover.
- Bugs get **regression tests**, not just fixes — see the mock SOCKS5 server in
  `geo.rs` and the prompt-header tests in `prompts.rs`.
- Network tests must stay on loopback and never depend on the outside world.

## Pull request checklist

- [ ] `cargo test` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] Bug fix includes a regression test
- [ ] No behaviour in the "do not clean up" list was touched without a
      reproduction proving it's safe
- [ ] No credential, access code, or private data is logged or persisted
- [ ] Behaviour changes are reflected in the README
- [ ] User-visible changes are recorded in `CHANGELOG.md` under `[Unreleased]`
- [ ] Cross-platform impact considered (macOS main-thread dialogs, Linux tray
      requirements, Windows named pipes)
- [ ] PR description explains *what* changed and *why*

## Licensing

By contributing, you agree that your contributions are licensed under the
[AGPL-3.0-or-later](LICENSE) license of this project.
