// SPDX-License-Identifier: EUPL-1.2
//! ASCII-art commando TUI.
//!
//! Three screens: Welcome → Running → Closing (debrief).
//! Each is driven by the same `Session`; preparation & closing wall-clock
//! seconds are accumulated by the session itself.

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use daily_commando::{telemetry, Phase, Session, SessionConfig};
use std::io::{self, Write};
use std::time::{Duration, Instant};
use tracing::{info, info_span};

const FRAME_A: [&str; 5] = [r"   ___ ", r"  [o_o]", r" /|___|\", r"  |   | ", r" / \ / \"];
const FRAME_B: [&str; 5] = [r"   ___ ", r"  [o_o]", r" \|___|/", r"  |   | ", r"  | X | "];

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
    // ── Preparation ──────────────────────────────────────────────────
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

    // ── Running ──────────────────────────────────────────────────────
    {
        let span = info_span!("session.running").entered();
        running_loop(out, &mut session, auto_advance)?;
        drop(span);
    }

    if matches!(session.phase(), Phase::Aborted) {
        return Ok(session);
    }

    // ── Closing ──────────────────────────────────────────────────────
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
    let mut field = Field::Soldiers;
    let mut last_tick = Instant::now();

    loop {
        // 1-second clock for preparation counter
        if last_tick.elapsed() >= Duration::from_secs(1) {
            session.tick_one_second();
            telemetry::count_phase_second(session.phase());
            last_tick = Instant::now();
        }

        // sync working buffers into session config so prep snapshot stays fresh
        let n = soldiers_buf.parse::<u32>().unwrap_or(0).clamp(1, 20);
        if names.len() < n as usize {
            for i in names.len()..n as usize {
                names.push(format!("Soldier {}", i + 1));
            }
        }
        names.truncate(n as usize);
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
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') => { session.abort(); return Ok(()); }
                    KeyCode::Tab | KeyCode::Down => field = next_field(field, names.len()),
                    KeyCode::BackTab | KeyCode::Up => field = prev_field(field, names.len()),
                    KeyCode::Enter => {
                        // ENTER on a field advances; ENTER on last name commences
                        if let Field::Name(i) = field {
                            if i + 1 == names.len() { return Ok(()); }
                        }
                        field = next_field(field, names.len());
                    }
                    KeyCode::F(2) => return Ok(()), // shortcut: commence now
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
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
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
    queue!(out, SetForegroundColor(Color::DarkYellow), Print("  Roster:\r\n"), ResetColor)?;
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
        Print("    [Tab/↑↓] move  [Enter] next  [Enter on last name / F2] COMMENCE  [Esc/Q] abort\r\n"),
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

// ─── Running screen ─────────────────────────────────────────────────────
fn running_loop<W: Write>(out: &mut W, session: &mut Session, auto_advance: bool) -> Result<()> {
    let mut last_tick = Instant::now();
    let mut frame: u32 = 0;

    loop {
        if event::poll(Duration::from_millis(120))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Char(' ') => session.toggle_pause(),
                        KeyCode::Char('n') | KeyCode::Right => session.next_soldier(),
                        KeyCode::Char('q') | KeyCode::Esc => { session.abort(); break; }
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= Duration::from_secs(1) {
            session.tick_one_second();
            telemetry::count_phase_second(session.phase());
            last_tick = Instant::now();
            let s = session.snapshot();
            telemetry::record(&s, &session.stats());
            if auto_advance && s.elapsed_current >= s.per_soldier_seconds {
                session.next_soldier();
            }
        }

        frame = frame.wrapping_add(1);
        draw_running(out, session, frame)?;

        if matches!(session.phase(), Phase::Closing | Phase::Aborted) { break; }
    }
    Ok(())
}

fn draw_running<W: Write>(out: &mut W, session: &Session, frame: u32) -> Result<()> {
    let snap = session.snapshot();
    let (cols, _) = terminal::size().unwrap_or((100, 30));

    queue!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    queue!(
        out,
        SetForegroundColor(Color::DarkYellow),
        Print(format!(
            "  ╔══════════════════════════════════════════════════════════╗\r\n  ║   D A I L Y   C O M M A N D O   —   {:>3}/{:<3} soldiers       ║\r\n  ╚══════════════════════════════════════════════════════════╝\r\n",
            snap.current_soldier, snap.soldiers
        )),
        ResetColor
    )?;

    let n = snap.soldiers as usize;
    let spacing = ((cols as usize).saturating_sub(4) / n.max(1)).max(10);
    let phase_step = ((frame / 4) % 2) as usize;
    for line_idx in 0..5 {
        queue!(out, Print("  "))?;
        for i in 0..n {
            let is_current = (i as u32) + 1 == snap.current_soldier;
            let frame_lines = if (i + phase_step) % 2 == 0 { &FRAME_A } else { &FRAME_B };
            let color = if is_current { Color::Yellow }
                else if i < (snap.current_soldier as usize).saturating_sub(1) { Color::DarkGreen }
                else { Color::DarkGrey };
            queue!(out, SetForegroundColor(color),
                Print(format!("{:width$}", frame_lines[line_idx], width = spacing)),
                ResetColor)?;
        }
        queue!(out, Print("\r\n"))?;
    }
    queue!(out, Print("  "))?;
    for i in 0..n {
        let is_current = (i as u32) + 1 == snap.current_soldier;
        let color = if is_current { Color::Yellow } else { Color::DarkGrey };
        let name = session.config().name_of(i as u32);
        let truncated: String = name.chars().take(spacing.saturating_sub(2)).collect();
        queue!(out, SetForegroundColor(color),
            Print(format!("{:^width$}", truncated, width = spacing)),
            ResetColor)?;
    }
    queue!(out, Print("\r\n\r\n"))?;

    let rem = snap.remaining_current;
    let (label, color) = if rem < 0 { ("OVERTIME", Color::Red) }
        else if rem <= 10 { ("CRITICAL", Color::Red) }
        else if rem <= 30 { ("WARNING ", Color::Yellow) }
        else { ("ON TRACK", Color::Green) };
    queue!(out, SetForegroundColor(color),
        Print(format!("    ▶  {}  ({})   {}   [{}]\r\n",
            snap.current_name, snap.current_soldier, fmt_signed(rem), label)),
        ResetColor)?;

    let bar_w: usize = 50;
    let frac = if snap.per_soldier_seconds == 0 { 0.0 }
        else { (snap.elapsed_current as f32 / snap.per_soldier_seconds as f32).min(1.0) };
    let filled = (bar_w as f32 * frac) as usize;
    queue!(out,
        Print("    ["),
        SetForegroundColor(color),
        Print("█".repeat(filled)),
        ResetColor,
        Print("·".repeat(bar_w - filled)),
        Print(format!("]  total {} / {}\r\n\r\n", fmt(snap.total_elapsed), fmt(snap.total_budget)))
    )?;

    let phase_str = match snap.phase {
        Phase::Preparation => "PREP", Phase::Running => "RUNNING", Phase::Paused => "PAUSED",
        Phase::Closing => "CLOSING", Phase::Aborted => "ABORTED", Phase::Finished => "FINISHED",
    };
    queue!(out,
        SetForegroundColor(Color::DarkYellow),
        Print(format!("    Phase: {}  prep:{}  closing:{}\r\n",
            phase_str, fmt(snap.prep_seconds), fmt(snap.closing_seconds))),
        Print("    [SPACE] pause   [N/→] next   [Q/ESC] abort\r\n"),
        ResetColor
    )?;
    out.flush()?;
    Ok(())
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
fn fmt_signed(s: i32) -> String {
    let sign = if s < 0 { "-" } else { " " };
    let v = s.unsigned_abs();
    format!("{}{}:{:02}", sign, v / 60, v % 60)
}

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
