// SPDX-License-Identifier: EUPL-1.2
//! ASCII-art commando TUI.
//!
//! Three screens: Welcome → Running → Closing (debrief).
//! Each is driven by the same `Session`; preparation & closing wall-clock
//! seconds are accumulated by the session itself.

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use daily_commando::{ascii_fonts, landscape, telemetry, Phase, Session, SessionConfig};
use std::io::{self, Write};
use std::time::{Duration, Instant};
use tracing::{info, info_span};

pub fn run(session: Session, auto_advance: bool, skip_welcome: bool) -> Result<()> {
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    let result = run_all(&mut stdout, session, auto_advance, skip_welcome);

    execute!(stdout, cursor::Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    if let Ok(ref s) = result {
        print_final_summary(s);
    }
    result.map(|_| ())
}

fn run_all<W: Write>(
    out: &mut W,
    mut session: Session,
    auto_advance: bool,
    skip_welcome: bool,
) -> Result<Session> {
    if !skip_welcome {
        let span = info_span!("session.preparation").entered();
        info!(phase = "preparation", "welcome screen opened");
        welcome_loop(out, &mut session)?;
        drop(span);
    }
    if matches!(session.phase(), Phase::Aborted) {
        return Ok(session);
    }
    session.commence();
    info!(
        soldiers = session.config().soldiers,
        per_soldier_seconds = session.config().per_soldier_seconds(),
        prep_seconds = session.prep_seconds(),
        "daily commenced"
    );

    {
        let span = info_span!("session.running").entered();
        running_loop(out, &mut session, auto_advance)?;
        drop(span);
    }

    if matches!(session.phase(), Phase::Aborted) {
        return Ok(session);
    }

    {
        let span = info_span!("session.closing").entered();
        info!(
            total_elapsed = session.snapshot().total_elapsed,
            "daily finished — entering closing"
        );
        closing_loop(out, &mut session)?;
        drop(span);
    }

    Ok(session)
}

// ─── Welcome screen ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq)]
enum Field { Soldiers, Minutes, Name(usize) }

fn welcome_loop<W: Write>(out: &mut W, session: &mut Session) -> Result<()> {
    let cfg0 = session.config().clone();
    let mut soldiers_buf = cfg0.soldiers.to_string();
    let mut minutes_buf = (cfg0.total_seconds / 60).to_string();
    let mut names: Vec<String> = (0..cfg0.soldiers as usize)
        .map(|i| cfg0.names.get(i).cloned().unwrap_or_else(|| format!("Soldier {}", i + 1)))
        .collect();
    let mut original_names = names.clone();
    let mut field = Field::Soldiers;
    let mut last_tick = Instant::now();

    loop {
        if last_tick.elapsed() >= Duration::from_secs(1) {
            session.tick_one_second();
            telemetry::count_phase_second(session.phase());
            last_tick = Instant::now();
        }

        let n = soldiers_buf.parse::<u32>().unwrap_or(0).clamp(1, 20);
        // keep names + original_names sized to N
        while names.len() < n as usize {
            let i = names.len();
            names.push(format!("Soldier {}", i + 1));
        }
        while original_names.len() < n as usize {
            let i = original_names.len();
            original_names.push(format!("Soldier {}", i + 1));
        }
        names.truncate(n as usize);
        original_names.truncate(n as usize);

        let mins = minutes_buf.parse::<u32>().unwrap_or(0).max(1);
        session.update_config(SessionConfig {
            soldiers: n,
            total_seconds: mins * 60,
            names: names.clone(),
        });
        let tick = session.snapshot();
        telemetry::record(&tick, &session.stats());

        draw_welcome(out, &soldiers_buf, &minutes_buf, &names, field, tick.prep_seconds)?;

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Press { continue; }
                let editing_name = matches!(field, Field::Name(_));
                let alt_held = k.modifiers.contains(KeyModifiers::ALT);

                // Alt+Up/Down: manual reorder of the focused soldier
                if alt_held && matches!(k.code, KeyCode::Up | KeyCode::Down) {
                    if let Field::Name(i) = field {
                        let new_i = match k.code {
                            KeyCode::Up if i > 0 => Some(i - 1),
                            KeyCode::Down if i + 1 < names.len() => Some(i + 1),
                            _ => None,
                        };
                        if let Some(j) = new_i {
                            names.swap(i, j);
                            field = Field::Name(j);
                            info!(event = "roster_reorder", action = "move",
                                from = i, to = j, "soldier moved");
                        }
                    }
                    continue;
                }

                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') if !editing_name => {
                        session.abort(); return Ok(());
                    }
                    KeyCode::Esc => { session.abort(); return Ok(()); }
                    // F2: commence; F5: shuffle (always); F6: reset (always)
                    KeyCode::F(2) => return Ok(()),
                    KeyCode::F(5) => {
                        shuffle_in_place(&mut names);
                        info!(event = "roster_reorder", action = "shuffle",
                            via = "F5", "roster shuffled");
                    }
                    KeyCode::F(6) => {
                        names.clone_from(&original_names);
                        info!(event = "roster_reorder", action = "reset",
                            via = "F6", "roster reset");
                    }
                    // s/r only when not editing a name (collide with typing)
                    KeyCode::Char('s') if !editing_name => {
                        shuffle_in_place(&mut names);
                        info!(event = "roster_reorder", action = "shuffle",
                            via = "s", "roster shuffled");
                    }
                    KeyCode::Char('r') if !editing_name => {
                        names.clone_from(&original_names);
                        info!(event = "roster_reorder", action = "reset",
                            via = "r", "roster reset");
                    }
                    KeyCode::Tab | KeyCode::Down => field = next_field(field, names.len()),
                    KeyCode::BackTab | KeyCode::Up => field = prev_field(field, names.len()),
                    KeyCode::Enter => {
                        if let Field::Name(i) = field {
                            if i + 1 == names.len() { return Ok(()); }
                        }
                        field = next_field(field, names.len());
                    }
                    KeyCode::Backspace => match field {
                        Field::Soldiers => { soldiers_buf.pop(); }
                        Field::Minutes => { minutes_buf.pop(); }
                        Field::Name(i) => { names[i].pop(); }
                    },
                    KeyCode::Char(c) => match field {
                        Field::Soldiers if c.is_ascii_digit() && soldiers_buf.len() < 2 => {
                            soldiers_buf.push(c);
                        }
                        Field::Minutes if c.is_ascii_digit() && minutes_buf.len() < 3 => {
                            minutes_buf.push(c);
                        }
                        Field::Name(i) if !c.is_control() && names[i].len() < 24 => {
                            names[i].push(c);
                            // user is editing → record edit as the new "original"
                            // for this slot so reset reflects the typed name
                            if let Some(slot) = original_names.get_mut(i) {
                                *slot = names[i].clone();
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }
}

fn shuffle_in_place(v: &mut [String]) {
    // Fisher-Yates with a tiny LCG seeded from SystemTime.
    let mut seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xdead_beef);
    for i in (1..v.len()).rev() {
        // LCG step (Numerical Recipes constants)
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = (seed >> 33) as usize % (i + 1);
        v.swap(i, j);
    }
}

fn next_field(f: Field, n: usize) -> Field {
    match f {
        Field::Soldiers => Field::Minutes,
        Field::Minutes => Field::Name(0),
        Field::Name(i) if i + 1 < n => Field::Name(i + 1),
        Field::Name(_) => Field::Soldiers,
    }
}
fn prev_field(f: Field, n: usize) -> Field {
    match f {
        Field::Soldiers => Field::Name(n.saturating_sub(1)),
        Field::Minutes => Field::Soldiers,
        Field::Name(0) => Field::Minutes,
        Field::Name(i) => Field::Name(i - 1),
    }
}

fn draw_welcome<W: Write>(
    out: &mut W,
    soldiers: &str,
    minutes: &str,
    names: &[String],
    field: Field,
    prep_seconds: u32,
) -> Result<()> {
    queue!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    queue!(
        out,
        SetForegroundColor(Color::DarkYellow),
        Print("\r\n  ╔══════════════════════════════════════════════════════════╗\r\n"),
        Print("  ║          D A I L Y   C O M M A N D O   —   BRIEFING      ║\r\n"),
        Print("  ╚══════════════════════════════════════════════════════════╝\r\n\r\n"),
        ResetColor
    )?;

    field_line(out, "  Soldiers in the boat (N)  : ", soldiers, field == Field::Soldiers, 4)?;
    field_line(out, "  Total daily duration (min): ", minutes, field == Field::Minutes, 4)?;
    queue!(out, Print("\r\n"))?;
    queue!(
        out,
        SetForegroundColor(Color::DarkYellow),
        Print("  Order of fire (1 = first to speak):\r\n"),
        ResetColor
    )?;
    for (i, name) in names.iter().enumerate() {
        let label = format!("    {:>2}.  ", i + 1);
        field_line(out, &label, name, field == Field::Name(i), 26)?;
    }

    queue!(
        out,
        Print("\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print(format!("    Preparation time : {}\r\n", fmt(prep_seconds))),
        ResetColor,
        Print("\r\n"),
        SetForegroundColor(Color::DarkYellow),
        Print("    [Tab/↑↓] move  [Enter] next  [F2] COMMENCE  [Esc] abort\r\n"),
        Print("    [s / F5] shuffle order  [r / F6] reset  [Alt+↑/↓] move soldier\r\n"),
        ResetColor
    )?;
    out.flush()?;
    Ok(())
}

fn field_line<W: Write>(out: &mut W, label: &str, value: &str, selected: bool, width: usize) -> Result<()> {
    let (fg, marker) = if selected { (Color::Yellow, "▶ ") } else { (Color::DarkGrey, "  ") };
    queue!(
        out,
        SetForegroundColor(fg),
        Print(marker),
        Print(label),
        Print(format!("{:width$}", value, width = width)),
        Print(if selected { "_" } else { " " }),
        Print("\r\n"),
        ResetColor
    )?;
    Ok(())
}

// ─── Running screen: big centered timer + landscape ────────────────────

/// In-progress overlay animation. Frames render on top of the static landscape.
#[derive(Debug, Clone)]
enum Anim {
    /// Anchor flying from boat to next stop, then dragging it to the boat.
    /// Plays 5 frames at ~25 fps. On completion, calls `next_soldier()`.
    AnchorPull { frame: u8, started: Instant, target_kind: AnchorTarget },
    /// Mario-style victory: castle pulled in, flag rises, 4 fireworks. On
    /// completion, calls `session.close()`.
    MarioVictory { frame: u8, started: Instant },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AnchorTarget { Island, Castle }

const ANIM_FRAME_MS: u64 = 40;          // 25 fps
const ANCHOR_FRAMES: u8 = 5;
const VICTORY_FRAMES: u8 = 24;          // ~1 s

fn running_loop<W: Write>(out: &mut W, session: &mut Session, _auto_advance: bool) -> Result<()> {
    let mut last_sec_tick = Instant::now();
    let start = Instant::now();
    let mut frame: u64 = 0;
    let mut anim: Option<Anim> = None;
    // Latched once the last speaker pushes total beyond budget (no recovery).
    let mut cascade_active = false;

    loop {
        // ── Input ─────────────────────────────────────────────────────
        if event::poll(Duration::from_millis(20))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    let in_anim = anim.is_some();
                    match k.code {
                        // Pause works any time
                        KeyCode::Char(' ') if !cascade_active && !in_anim => session.toggle_pause(),

                        // Cascade-fail: q/Esc/Enter/Space close the session.
                        KeyCode::Char(' ') | KeyCode::Enter
                        | KeyCode::Char('q') | KeyCode::Esc if cascade_active => {
                            session.close();
                            break;
                        }

                        // Manual abort
                        KeyCode::Char('q') | KeyCode::Esc if !in_anim => { session.abort(); break; }

                        // Manual next
                        KeyCode::Char('n') | KeyCode::Right if !in_anim && !cascade_active => {
                            let snap = session.snapshot();
                            let last = snap.current_soldier == snap.soldiers;
                            let elapsed = snap.elapsed_current as i32;
                            let alw = snap.current_allowance.max(1);
                            let early = elapsed < alw;       // boat hasn't reached destination
                            let within_budget = snap.total_elapsed <= snap.total_budget;

                            if last && early && within_budget {
                                anim = Some(Anim::MarioVictory { frame: 0, started: Instant::now() });
                            } else if early {
                                let kind = if last { AnchorTarget::Castle } else { AnchorTarget::Island };
                                anim = Some(Anim::AnchorPull {
                                    frame: 0, started: Instant::now(),
                                    target_kind: kind,
                                });
                            } else {
                                // Already at (or past) the destination — just advance.
                                session.next_soldier();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // ── 1-sec session tick ────────────────────────────────────────
        if last_sec_tick.elapsed() >= Duration::from_secs(1) {
            session.tick_one_second();
            telemetry::count_phase_second(session.phase());
            last_sec_tick = Instant::now();
            let s = session.snapshot();
            telemetry::record(&s, &session.stats());
            if s.total_elapsed > s.total_budget {
                telemetry::count_cascade_second();
            }
            // Latch cascade if the LAST speaker has overrun the global budget.
            if !cascade_active
                && s.current_soldier == s.soldiers
                && s.total_elapsed > s.total_budget
            {
                cascade_active = true;
                info!(event = "cascade_started", total = s.total_elapsed,
                    budget = s.total_budget, "team failed to dock — falling into cascade");
            }
        }

        // ── Animation transitions ─────────────────────────────────────
        let mut next_anim = anim.clone();
        if let Some(a) = anim.as_ref() {
            match a {
                Anim::AnchorPull { started, .. } => {
                    let f = (started.elapsed().as_millis() as u64 / ANIM_FRAME_MS) as u8;
                    if f >= ANCHOR_FRAMES {
                        session.next_soldier();
                        next_anim = None;
                    } else if let Some(Anim::AnchorPull { frame, .. }) = next_anim.as_mut() {
                        *frame = f;
                    }
                }
                Anim::MarioVictory { started, .. } => {
                    let f = (started.elapsed().as_millis() as u64 / ANIM_FRAME_MS) as u8;
                    if f >= VICTORY_FRAMES {
                        session.close();
                        let _ = anim.take();
                        break;
                    } else if let Some(Anim::MarioVictory { frame, .. }) = next_anim.as_mut() {
                        *frame = f;
                    }
                }
            }
        }
        anim = next_anim;

        // ── Render ────────────────────────────────────────────────────
        frame += 1;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let blink_500 = ((elapsed_ms / 500) % 2) as u8;
        let blink_1000 = ((elapsed_ms / 1000) % 2) as u8;
        draw_running(out, session, frame, blink_500, blink_1000, elapsed_ms,
                     cascade_active, anim.as_ref())?;

        // Exit when session left the running family (Closing/Aborted) — but
        // ONLY if we are not in cascade-fail mode, where Closing is reached
        // only via explicit user keypress (handled above).
        if matches!(session.phase(), Phase::Closing | Phase::Aborted) && !cascade_active { break; }

        std::thread::sleep(Duration::from_millis(ANIM_FRAME_MS));
    }
    Ok(())
}

fn draw_running<W: Write>(
    out: &mut W,
    session: &Session,
    frame: u64,
    blink_500: u8,
    blink_1000: u8,
    elapsed_ms: u64,
    cascade_active: bool,
    anim: Option<&Anim>,
) -> Result<()> {
    let snap = session.snapshot();
    let (cols_u16, rows_u16) = terminal::size().unwrap_or((100, 30));
    let cols = cols_u16 as usize;
    let rows = rows_u16 as usize;

    // ── Decide which font & color the timer uses ──────────────────────
    let alw = snap.current_allowance.max(1);
    let elapsed_cur = snap.elapsed_current as i32;
    let two_thirds = (alw * 2) / 3;
    let remaining = alw - elapsed_cur;
    let (font, color) = if remaining < 0 {
        // overtime → blink every 0.5 s, ALWAYS RED
        if blink_500 == 0 { (ascii_fonts::Font::Ko, Color::Red) } else { (ascii_fonts::Font::Ok, Color::Red) }
    } else if remaining <= 5 {
        if blink_1000 == 0 { (ascii_fonts::Font::Ok, Color::Red) } else { (ascii_fonts::Font::Ko, Color::Red) }
    } else if elapsed_cur >= two_thirds {
        (ascii_fonts::Font::Ko, Color::Yellow)
    } else {
        (ascii_fonts::Font::Ok, Color::Green)
    };

    // ── Build canvas ─────────────────────────────────────────────────
    let mut canvas: Vec<Vec<char>> = vec![vec![' '; cols]; rows];
    let mut color_map: Vec<Vec<Option<Color>>> = vec![vec![None; cols]; rows];

    let world = landscape::WorldState::compute(&landscape::Inputs {
        total_elapsed: snap.total_elapsed,
        total_budget: snap.total_budget,
        soldiers: snap.soldiers,
        current_soldier: snap.current_soldier,
        elapsed_current: snap.elapsed_current,
        current_allowance: snap.current_allowance,
        cols, rows, elapsed_ms,
        cascade_active,
    });
    landscape::paint(&world, &mut canvas, &mut color_map);

    // ── Animations overlay (between landscape and timer) ──────────────
    if let Some(a) = anim {
        match a {
            Anim::AnchorPull { frame, target_kind, .. } => {
                paint_anchor(&mut canvas, &mut color_map, &world, *frame, *target_kind);
            }
            Anim::MarioVictory { frame, .. } => {
                paint_victory(&mut canvas, &mut color_map, &world, *frame);
            }
        }
    }

    // ── Big timer ─────────────────────────────────────────────────────
    let (mins, secs) = if remaining >= 0 {
        ((remaining / 60) as u32, (remaining % 60) as u32)
    } else {
        let v = (-remaining) as u32;
        (v / 60, v % 60)
    };
    let timer_str = format!("{}:{:02}", mins, secs);
    let timer_lines = ascii_fonts::render_big(&timer_str, font);
    let prefix = if remaining < 0 { Some('-') } else { None };

    let timer_w = timer_lines.iter().map(|l| l.chars().count()).max().unwrap_or(0)
        + if prefix.is_some() { 4 } else { 0 };
    let timer_h = timer_lines.len();
    let start_x = cols.saturating_sub(timer_w) / 2;
    let start_y = rows.saturating_sub(timer_h) / 2;
    for (dy, line) in timer_lines.iter().enumerate() {
        let y = start_y + dy;
        if y >= rows { break; }
        let mut x = start_x;
        if let Some(c) = prefix {
            if dy == timer_h / 2 {
                if x < cols { canvas[y][x] = c; color_map[y][x] = Some(color); }
            }
            x += 4;
        }
        for ch in line.chars() {
            if x >= cols { break; }
            if ch != ' ' {
                canvas[y][x] = ch;
                color_map[y][x] = Some(color);
            }
            x += 1;
        }
    }

    // ── Header / footer ──────────────────────────────────────────────
    let bonus = snap.current_allowance - snap.per_soldier_seconds as i32;
    let bonus_str = if bonus > 0 { format!(" (+{}s pool)", bonus) }
        else if bonus < 0 { format!(" ({}s pool)", bonus) } else { String::new() };
    let header = format!(
        " D A I L Y   C O M M A N D O   —   {} ({}/{}){}   prep:{}",
        snap.current_name, snap.current_soldier, snap.soldiers, bonus_str, fmt(snap.prep_seconds)
    );
    paint_string(&mut canvas, &mut color_map, 1, 0, &header, Color::DarkYellow);

    let phase_str = match snap.phase {
        Phase::Preparation => "PREP", Phase::Running => "RUNNING", Phase::Paused => "PAUSED",
        Phase::Closing => "CLOSING", Phase::Aborted => "ABORTED", Phase::Finished => "FINISHED",
    };
    let footer = if cascade_active {
        format!(" 🔥 CASCADE  total {} / {}   [Q/Enter/Space] end mission",
            fmt(snap.total_elapsed), fmt(snap.total_budget))
    } else {
        format!(" phase:{}  total {} / {}   [SPACE] pause  [N/→] next  [Q/ESC] abort",
            phase_str, fmt(snap.total_elapsed), fmt(snap.total_budget))
    };
    if rows >= 1 {
        paint_string(&mut canvas, &mut color_map, 1, rows - 1, &footer,
            if cascade_active { Color::Red } else { Color::DarkYellow });
    }
    let _ = frame;

    // ── Flush canvas ─────────────────────────────────────────────────
    queue!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    for y in 0..rows {
        let mut current_color: Option<Color> = None;
        let mut buf = String::new();
        for x in 0..cols {
            let ch = canvas[y][x];
            let c = color_map[y][x];
            if c != current_color {
                if !buf.is_empty() {
                    write_colored(out, &buf, current_color)?;
                    buf.clear();
                }
                current_color = c;
            }
            buf.push(ch);
        }
        if !buf.is_empty() {
            write_colored(out, &buf, current_color)?;
        }
        if y + 1 < rows { queue!(out, Print("\r\n"))?; }
    }
    out.flush()?;
    Ok(())
}

// ─── Animation overlays ─────────────────────────────────────────────────

fn paint_anchor(
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
    w: &landscape::WorldState,
    frame: u8,
    target_kind: AnchorTarget,
) {
    // Boat anchor origin (right side of hull)
    let bx_origin = w.boat_x + 8;
    let by = w.boat_y + 2;
    // Target x: next stop (= w.stops[current_soldier])
    let cur = w.current_soldier as usize;
    let target_x = if target_kind == AnchorTarget::Castle {
        w.castle_x.saturating_sub(2)
    } else {
        *w.stops.get(cur).unwrap_or(&bx_origin)
    };

    // Frames 0..ANCHOR_FRAMES-1: anchor flies; we also visually drag the
    // island's *trunk* progressively closer to the boat.
    let f = frame.min(ANCHOR_FRAMES - 1) as f32 / (ANCHOR_FRAMES - 1) as f32;
    let anchor_x = bx_origin + ((target_x as i32 - bx_origin as i32) as f32 * (1.0 - f)) as i32 as usize;
    landscape::place(canvas, colors, anchor_x, by, '⌬', Color::DarkGrey);
    // Rope
    let (lo, hi) = if anchor_x < bx_origin { (anchor_x, bx_origin) } else { (bx_origin, anchor_x) };
    for x in lo..=hi {
        if x != anchor_x { landscape::place(canvas, colors, x, by, '-', Color::DarkGrey); }
    }
}

fn paint_victory(
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
    w: &landscape::WorldState,
    frame: u8,
) {
    // Flag rising on top of the castle. Frame 0..VICTORY_FRAMES, height grows.
    let cw = w.castle_w;
    let cx = w.castle_x;
    let top_y = w.horizon_y.saturating_sub(4);
    if w.horizon_y < 5 { return; }
    let pole_x = cx + cw / 2;
    let max_h = (top_y).min(8);
    let h = ((frame as usize) * max_h / VICTORY_FRAMES as usize).min(max_h);
    for dy in 0..=h {
        let y = top_y.saturating_sub(dy);
        landscape::place(canvas, colors, pole_x, y, '|', Color::White);
    }
    let flag_y = top_y.saturating_sub(h);
    landscape::place(canvas, colors, pole_x + 1, flag_y, '#', Color::Red);
    landscape::place(canvas, colors, pole_x + 2, flag_y, '#', Color::Red);
    landscape::place(canvas, colors, pole_x + 3, flag_y, '>', Color::Red);

    // 4 fireworks at frames 6,10,14,18 — each lasts 4 frames.
    let bursts = [(6u8, cx.saturating_sub(8), top_y.saturating_sub(4)),
                  (10, cx + cw + 4, top_y.saturating_sub(6)),
                  (14, cx.saturating_sub(4), top_y.saturating_sub(8).max(2)),
                  (18, cx + cw / 2 + 6, top_y.saturating_sub(5))];
    for (start, fx, fy) in bursts {
        if frame >= start && frame < start + 4 {
            let radius = (frame - start) as i32 + 1;
            let palette = [Color::Yellow, Color::Magenta, Color::Cyan, Color::Red];
            let c = palette[((frame as usize) ^ (start as usize)) % 4];
            for (dx, dy, ch) in [(-radius, 0, '*'), (radius, 0, '*'),
                                 (0, -radius, '*'), (0, radius, '*'),
                                 (-radius, -radius, '+'), (radius, radius, '+'),
                                 (-radius, radius, '+'), (radius, -radius, '+')] {
                let x = (fx as i32 + dx) as usize;
                let y = (fy as i32 + dy) as usize;
                landscape::place(canvas, colors, x, y, ch, c);
            }
        }
    }
}

fn write_colored<W: Write>(out: &mut W, s: &str, c: Option<Color>) -> Result<()> {
    if let Some(c) = c {
        queue!(out, SetForegroundColor(c), Print(s), ResetColor)?;
    } else {
        queue!(out, Print(s))?;
    }
    Ok(())
}

fn paint_string(
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
    x: usize,
    y: usize,
    s: &str,
    color: Color,
) {
    if y >= canvas.len() { return; }
    let row = &mut canvas[y];
    let crow = &mut colors[y];
    let mut xi = x;
    for ch in s.chars() {
        if xi >= row.len() { break; }
        row[xi] = ch;
        crow[xi] = Some(color);
        xi += 1;
    }
}

// ─── Closing screen ─────────────────────────────────────────────────────
fn closing_loop<W: Write>(out: &mut W, session: &mut Session) -> Result<()> {
    let mut last_tick = Instant::now();
    loop {
        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter) {
                        break;
                    }
                }
            }
        }
        if last_tick.elapsed() >= Duration::from_secs(1) {
            session.tick_one_second();
            telemetry::count_phase_second(session.phase());
            last_tick = Instant::now();
            let snap = session.snapshot();
            telemetry::record(&snap, &session.stats());
        }
        draw_closing(out, session)?;
    }
    Ok(())
}

fn draw_closing<W: Write>(out: &mut W, session: &Session) -> Result<()> {
    let snap = session.snapshot();
    let stats = session.stats();
    queue!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    queue!(out,
        SetForegroundColor(Color::DarkYellow),
        Print("\r\n  ╔══════════════════════════════════════════════════════════╗\r\n"),
        Print("  ║              M I S S I O N   D E B R I E F               ║\r\n"),
        Print("  ╚══════════════════════════════════════════════════════════╝\r\n\r\n"),
        ResetColor
    )?;
    for s in &stats {
        let over_color = if s.overtime_seconds > 0 { Color::Red } else { Color::Green };
        let over = if s.overtime_seconds > 0 {
            format!("  (+{} over)", fmt(s.overtime_seconds))
        } else { String::new() };
        queue!(out,
            Print(format!("    {:>2}.  {:<24}  ", s.index, s.name)),
            SetForegroundColor(over_color),
            Print(format!("{}{}\r\n", fmt(s.elapsed_seconds), over)),
            ResetColor
        )?;
    }
    queue!(out, Print("\r\n"),
        SetForegroundColor(Color::DarkYellow),
        Print("    ─────────────────────────────────\r\n"),
        ResetColor,
        Print(format!("    Preparation : {}\r\n", fmt(snap.prep_seconds))),
        Print(format!("    Daily total : {}\r\n", fmt(snap.total_elapsed))),
        Print(format!("    Closing     : {} (still counting)\r\n", fmt(snap.closing_seconds))),
        Print("\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print("    [Q/ESC/Enter] dismiss & exit\r\n"),
        ResetColor
    )?;
    out.flush()?;
    Ok(())
}

// ─── Helpers ────────────────────────────────────────────────────────────
fn fmt(s: u32) -> String { format!("{}:{:02}", s / 60, s % 60) }

fn print_final_summary(session: &Session) {
    let snap = session.snapshot();
    let stats = session.stats();
    let total: u32 = stats.iter().map(|s| s.elapsed_seconds).sum();
    println!();
    println!("════════ FINAL DEBRIEF ════════");
    for s in &stats {
        let over = if s.overtime_seconds > 0 { format!("  (+{} over)", fmt(s.overtime_seconds)) } else { String::new() };
        println!("  {:>2}. {:<24} {}{}", s.index, s.name, fmt(s.elapsed_seconds), over);
    }
    println!("──────────────────────────────");
    println!("  Preparation     : {}", fmt(snap.prep_seconds));
    println!("  Daily total     : {}", fmt(total));
    println!("  Closing         : {}", fmt(snap.closing_seconds));
    println!("  Phase           : {:?}", session.phase());
    println!();
    info!(
        prep_seconds = snap.prep_seconds,
        daily_total_seconds = total,
        closing_seconds = snap.closing_seconds,
        phase = ?session.phase(),
        "session terminated"
    );
}
