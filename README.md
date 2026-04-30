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
- `-n, --soldiers N`     participants (1..=20, default 5)
- `-t, --minutes M`      total daily duration in minutes (default 15)
- `-m, --mode tui|json`  output mode (default `tui`)
- `--names a,b,c`        pre-fill soldier names (skips name prompts)
- `--skip-welcome`       jump straight into the run (TUI)
- `--metrics-port PORT`  Prometheus `/metrics` endpoint (default `9464`)
- `--no-metrics`         disable the metrics HTTP server

## TUI controls

### Welcome / preparation screen
| Key             | Action                                                |
|-----------------|-------------------------------------------------------|
| `Tab` / `↑` `↓` | Move focus between fields                             |
| `Enter`         | Validate field / start the daily when on the button   |
| `s` or `F5`     | **Shuffle** the running order randomly                |
| `r` or `F6`     | **Reset** to the order initially typed                |
| `Alt+↑` / `Alt+↓` | **Move** the focused soldier up / down              |
| `q` / `Esc`     | Abort                                                 |

The 1-based index in front of each soldier **is** the speaking order.

### Running screen
| Key                | Action                            |
|--------------------|-----------------------------------|
| `Space`            | Pause / resume                    |
| `n` or `→`         | Manually hand over to next soldier|
| `q` / `Esc`        | Abort the daily                   |

> **There is no auto-advance.** When a soldier's allowance hits zero the timer
> goes negative (red) and the **boat keeps cruising at constant speed** —
> the next island is visibly **towed two characters behind the boat** by an
> anchor + rope until the user manually advances. The speaker who runs over
> is literally dragging the next person across the ocean. Pressing `n` early
> earns *bonus time* for the next speaker (saved seconds pool forward) and
> plays a 5-frame anchor animation that **drags the next island TO the boat**
> (the boat itself does not slow down).

The display is a side-scroller:

* a **big centered ASCII timer** counts down the current speaker's allowance,
  going **red and negative** on overshoot,
* a **boat** (with a tiny soldier on deck) sails at **constant cruise speed**
  driven by `total_elapsed / total_budget`,
* each island has a **brown trunk + green canopy** palm and the next waiting
  soldier; islands sit still until either the user hands over early (anchor
  pulls the island TO the boat) or the boat passes without pickup (a rope
  tows the island 2 chars behind the boat),
* the **font switches** to corrupt `ko` glyphs at 66 % of the allowance,
  alternates every tick in the last 5 s, and blinks every 0.5 s in overtime,
* the renderer is **diff-based**: only changed cells are repainted, so the
  big timer no longer flickers at 25 fps,
* if the team **dock the castle in time**, a **Mario-style flag rises** on
  top of the castle with **4 fireworks** above it (easter egg) before the
  debrief screen,
* if the team **fails** to dock before the global budget runs out, the boat
  passes through the destroyed castle and **falls into the cascade**:
    * `1× → 2×` total budget — boat falls through the void,
    * `2× → 2.5×` — **Satan's face** appears at the bottom,
    * `2.5× → 3×` — Satan reveals his **trident**,
    * `≥ 3×` — **the boat crashes in hell**: screen turns red, flames
      everywhere, debris on the floor.
  The mission report **does not** appear automatically — press
  `q` / `Enter` / `Space` at any time to end the mission and reveal the debrief.

## Architecture

```
src/
├── lib.rs          re-exports the domain
├── session.rs      pure state machine, no I/O — future web backend reuses this as-is
├── main.rs         CLI parsing
├── telemetry.rs    OpenTelemetry-style logs/metrics + Prometheus /metrics HTTP
├── ascii_fonts.rs  big 7×7 digit glyphs (Ok / Ko)
├── landscape.rs    sun, boat, islands, castle, cascade, Satan
├── render_tui.rs   crossterm ASCII-art renderer (welcome + run + closing)
└── render_json.rs  newline-delimited JSON renderer (pipe-friendly)
```

The CLI is the future **backend**; both renderers consume the same `Tick`
snapshots so a web UI can be plugged on top of `Session` later without
touching the timer logic.

## Observability

The binary is instrumented with `tracing` (structured logs) and `prometheus`
metrics. A ready-to-use stack is provided under `observability/`:

```sh
docker compose -f observability/docker-compose.yml up -d
cargo run --release -- -n 3 -t 5            # leave it running
open http://localhost:3000                  # Grafana, anonymous viewer enabled
```

Stack:
* **VictoriaMetrics** (`:8428`) — TSDB + PromQL
* **vmagent** — scrapes `host.docker.internal:9464/metrics` every 5 s
* **Grafana** (`:3000`, `admin` / `admin`) — pre-provisioned dashboard
  *Daily Commando — Mission Control* (phase rates, current soldier, overtime,
  cascade seconds, Satan-reached).

Exposed metrics (prefix `daily_commando_`):
`sessions_total`, `phase_seconds_total{phase=…}`, `current_soldier_index`,
`soldier_seconds{index,name}`, `overtime_seconds`, `soldiers_boarded`,
`cascade_seconds_total`, `satan_reached`.

No personal data leaves the host: soldier names are kept locally; only their
1-based index travels in metric labels by default.

## License

EUPL-1.2 — see [LICENSE](LICENSE).
