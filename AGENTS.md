# AGENTS.md

## Project Overview

Rust P2P chat application with a fullscreen TUI (ratatui + crossterm). No central server — nodes connect directly via TCP with automatic peer registry persistence.

## Commands

```bash
cargo check              # fast compile check
cargo build              # dev build
cargo build --release    # release build (recommended)
cargo run --release      # run interactively
cargo run --release -- --port 9005  # bind specific port
```

No unit tests exist. `test.sh` is a manual integration script with hardcoded args — not an automated test suite.

No `rustfmt.toml`, `.clippy.toml`, CI, or pre-commit config. Use Rust defaults.

## Port Binding (Important)

Port is **not** random (`:0`). Without `--port`, the app tries `DEFAULT_PORTS = [9000, 9001, 9002, 9003, 9004]` in order, binding the first available. All five busy → error, user must pass `--port <N>`.

`connect <ip>` (IP-only form) probes all 5 ports concurrently with 200ms timeout each — first to accept wins.

## Architecture

**Dual-channel design**: UI layer and Node layer communicate via two unbounded MPSC channels (`cmd_tx/cmd_rx`, `event_tx/event_rx`).

```
main.rs → bind_listener() → run_setup() → P2PNode::start() (tokio::spawn) → run_tui() (main task)
```

**Module dependency graph (strictly unidirectional, no cycles)**:
```
main.rs ─┬─► ui.rs       ─► event.rs
         │                ─► network.rs ─► connection.rs ─► message.rs
         │                                ─► registry.rs
         └─► node.rs      ─► event.rs, network.rs, connection.rs, registry.rs
```

**Key structural facts**:
- Port binding happens in `main()` via `bind_listener()` **before** setup, so the port can be displayed. The bound listener is passed into `P2PNode::start()`.
- `run_setup()` returns the `Terminal` still in raw mode; `run_tui()` takes it over directly to avoid screen flicker.
- `current_chat` state lives in the **UI layer** (`App` struct in `ui.rs`), not in `P2PNode`.
- `local_ip()` in `ui.rs` determines LAN address by "connecting" to 8.8.8.8:80 (no packet sent).
- Write channel per connection is **bounded** (`mpsc::channel(32)`), not unbounded.
- Shutdown sends `Disconnect` to all peers, sleeps 200ms for flush, then `main` waits up to 2s for node task.

## Conventions & Gotchas

- **Never hold a `ConnectionMap` lock across an `.await`** — clone the sender, drop the lock, then await.
- **Broadcast**: snapshot all senders into a `Vec` first, release the lock, then send serially.
- Failed connections are collected into a `failed` list and cleaned up after the loop, not inline.
- Logs go to `p2p-chat.log` (file sink via `tracing-subscriber`), not stdout — avoids corrupting the TUI.
- Registry (`known_peers.json`) uses `serde_json` and writes synchronously on every `upsert()`.
- Max wire message size: 10 MB (bincode + 4-byte big-endian length prefix).

## File Locations

| File | Role |
|------|------|
| `src/main.rs` | CLI args, port binding (DEFAULT_PORTS strategy), setup → tui handoff (~150 lines) |
| `src/event.rs` | `AppEvent`, `NodeCommand`, `DisplayLine` types (~70 lines) |
| `src/ui.rs` | Setup TUI + Chat TUI, input routing, rendering (~500 lines) |
| `src/node.rs` | `P2PNode` state machine, accept loop, IP discovery probe (~660 lines) |
| `src/network.rs` | `handle_*` loops, time formatting (~170 lines) |
| `src/connection.rs` | `ConnectionMap`, `write_msg`/`read_msg` frame encode/decode (~80 lines) |
| `src/message.rs` | `P2PMessage` protocol enum (~120 lines) |
| `src/registry.rs` | `PeerRegistry`, `KnownPeer`, `parse_peer_arg` (~100 lines) |

## Docs

- `docs/design_document.md` — full architecture and protocol design
- `docs/code_analysis.md` — detailed code walkthrough
- `docs/tokio_async_guide.md` — Tokio/async primer
