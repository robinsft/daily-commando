# daily-commando

A small daily-standup timer, commando-style — all in your terminal.

Built in Rust as a **headless core** (`Session`) wrapped by two interchangeable
renderers, so the same engine can later drive a multi-user web backend without
touching the timer logic.

## Usage

```sh
cargo run --release -- -n 5 -t 15            # ASCII-art TUI (default)
cargo run --release -- -n 5 -t 15 -m json    # one JSON tick per second on stdout
```

Flags:
- `-n, --soldiers N`  participants (1..=20, default 5)
- `-t, --minutes M`   total daily duration in minutes (default 15)
- `-m, --mode tui|json`  output mode (default `tui`)

TUI controls: `SPACE` pause/resume · `n` or `→` next soldier · `q`/`ESC` abort.

## Architecture

```
src/
├── lib.rs          re-exports the domain
├── session.rs      pure state machine, no I/O — future web backend reuses this as-is
├── main.rs         CLI parsing
├── render_tui.rs   crossterm ASCII-art renderer
└── render_json.rs  newline-delimited JSON renderer (pipe-friendly)
```

No names, no personal data: soldiers are identified by 1-based index only.

## License

EUPL-1.2 — see [LICENSE](LICENSE).
