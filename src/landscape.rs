// SPDX-License-Identifier: EUPL-1.2
//! Landscape narrative engine — round 3.
//!
//! Three acts:
//!   * **Voyage** — boat sails left → right across the ocean past N-1 islands
//!     (each holding a waiting soldier under a tall palm tree), heading for
//!     the castle on the right edge. The boat moves PER SEGMENT: while
//!     soldier `i` is speaking, the boat travels from the previous stop to
//!     the destination of soldier `i`. When `i` overshoots their allowance,
//!     the boat is parked at the destination and visibly *pushes* the
//!     palm — the palm tilts more and more.
//!   * **Cascade** — the team failed to finish in time. Boat goes past the
//!     destroyed castle and falls into an infinite waterfall.
//!   * **Abyss** — at 2× total budget, Satan's throne reveals at the bottom.
//!
//! All decor is painted onto a char canvas; the big timer overlays on top.
//! No I/O, no terminal calls — pure state + paint(canvas).

use crossterm::style::Color;

#[derive(Debug, Clone)]
pub struct WorldState {
    pub cols: usize,
    pub rows: usize,
    pub act: Act,
    pub sun_x: usize,
    pub boat_x: usize,
    pub boat_y: usize,           // top row of the (4-row) boat
    pub boat_rotation: u8,       // 0..4, used in Cascade/Abyss
    pub horizon_y: usize,
    pub stops: Vec<usize>,       // N+1 x-positions: [start, island_1, …, island_{N-1}, castle_dock]
    pub palm_tilts: Vec<u8>,     // tilt 0..=3 for each of the N-1 islands
    pub current_soldier: u32,    // 1-based
    pub soldiers: u32,
    pub castle_x: usize,
    pub castle_w: usize,
    pub castle_state: CastleState,
    pub satan_revealed: bool,
    pub cascade_x: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act { Voyage, Cascade, Abyss }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastleState { Intact, Destroyed }

/// Inputs the TUI feeds us each frame.
pub struct Inputs {
    pub total_elapsed: u32,
    pub total_budget: u32,
    pub soldiers: u32,
    pub current_soldier: u32,    // 1-based
    pub elapsed_current: u32,
    pub current_allowance: i32,
    pub cols: usize,
    pub rows: usize,
    pub elapsed_ms: u64,
    /// True when the last soldier has gone past the global budget; triggers
    /// the boat to leave the castle dock and fall into the cascade.
    pub cascade_active: bool,
}

impl WorldState {
    pub fn compute(inp: &Inputs) -> Self {
        let cols = inp.cols.max(20);
        let rows = inp.rows.max(10);
        let budget = inp.total_budget.max(1);

        // ── Castle geometry ─────────────────────────────────────────
        let castle_w = (cols / 10).clamp(7, 15);
        let castle_x = cols.saturating_sub(castle_w + 2);

        // ── Stops: N+1 anchor x positions ──────────────────────────
        // [0]=start dock (left), [1..N-1]=N-1 islands, [N]=castle dock.
        let n = inp.soldiers.max(1) as usize;
        let start_x: usize = 4;
        let castle_dock_x: usize = castle_x.saturating_sub(2);
        let stops: Vec<usize> = if n == 1 {
            vec![start_x, castle_dock_x]
        } else {
            let span = castle_dock_x.saturating_sub(start_x);
            (0..=n).map(|k| start_x + (span as f32 * (k as f32 / n as f32)) as usize).collect()
        };

        // ── Boat segment progress ──────────────────────────────────
        let cur = inp.current_soldier.clamp(1, inp.soldiers.max(1)) as usize;
        let seg_start = stops[cur - 1];
        let seg_end = stops[cur];
        let alw = inp.current_allowance.max(1) as f32;
        let seg_progress = (inp.elapsed_current as f32 / alw).clamp(0.0, 1.0);

        // Waterline boat (Voyage)
        let voyage_boat_x = seg_start + ((seg_end as i32 - seg_start as i32) as f32 * seg_progress) as i32 as usize;
        let voyage_boat_y = rows.saturating_sub(6);   // 4-tall boat sitting on horizon (rows-2)

        // ── Acts: cascade only when explicitly signaled by TUI ─────
        let progress = inp.total_elapsed as f32 / budget as f32;
        let act = if !inp.cascade_active {
            Act::Voyage
        } else if progress < 2.0 {
            Act::Cascade
        } else {
            Act::Abyss
        };

        // ── Boat per act ───────────────────────────────────────────
        let (boat_x, boat_y, boat_rotation);
        match act {
            Act::Voyage => {
                boat_x = voyage_boat_x;
                boat_y = voyage_boat_y;
                boat_rotation = 0;
            }
            Act::Cascade => {
                let fall = (progress - 1.0).clamp(0.0, 1.0);
                // First 30% of fall: continue right past castle off-screen-ish.
                // Then start falling vertically on the right edge.
                let horiz = fall.min(0.3) / 0.3;
                let vert = ((fall - 0.3) / 0.7).clamp(0.0, 1.0);
                let bx = castle_dock_x + ((cols.saturating_sub(castle_dock_x + 4)) as f32 * horiz) as usize;
                boat_x = bx.min(cols.saturating_sub(9));
                let max_fall = rows.saturating_sub(3);
                boat_y = voyage_boat_y + ((max_fall.saturating_sub(voyage_boat_y)) as f32 * vert) as usize;
                boat_rotation = ((inp.elapsed_ms / 250) % 4) as u8;
            }
            Act::Abyss => {
                boat_x = cols.saturating_sub(12);
                boat_y = rows.saturating_sub(5);
                boat_rotation = ((inp.elapsed_ms / 200) % 4) as u8;
            }
        }

        // ── Horizon ────────────────────────────────────────────────
        let horizon_y = match act {
            Act::Voyage => rows.saturating_sub(2),
            Act::Cascade => {
                // horizon rises 1 row per second of overrun
                let secs = (inp.total_elapsed as i32 - budget as i32).max(0) as usize;
                rows.saturating_sub(2).saturating_sub(secs).max(rows / 4)
            }
            Act::Abyss => rows / 4,
        };

        // ── Sun ────────────────────────────────────────────────────
        let sun_x = if matches!(act, Act::Voyage) {
            ((cols as f32 - 4.0) * progress.min(1.0)) as usize
        } else { 0 };

        // ── Palm tilts ─────────────────────────────────────────────
        // Only the destination island of the *current* speaker can tilt
        // (because the boat is pushing it). Tilt grows with overshoot.
        let mut palm_tilts = vec![0u8; n.saturating_sub(1)];
        if matches!(act, Act::Voyage) {
            let overshoot = inp.elapsed_current as i32 - inp.current_allowance;
            if overshoot > 0 && cur >= 1 && cur <= palm_tilts.len() {
                let denom = inp.current_allowance.max(1) as f32;
                let ratio = (overshoot as f32 / denom).clamp(0.0, 1.0);
                let tilt = (ratio * 3.0).round() as u8;
                palm_tilts[cur - 1] = tilt.min(3);
            }
        }

        let castle_state = if matches!(act, Act::Voyage) { CastleState::Intact } else { CastleState::Destroyed };
        let satan_revealed = matches!(act, Act::Abyss);
        let cascade_x = cols.saturating_sub(4);

        WorldState {
            cols, rows, act, sun_x, boat_x, boat_y, boat_rotation, horizon_y,
            stops, palm_tilts, current_soldier: inp.current_soldier, soldiers: inp.soldiers,
            castle_x, castle_w, castle_state, satan_revealed, cascade_x,
        }
    }
}

/// Paint the entire decor onto the (canvas, color_map). Call BEFORE the timer.
pub fn paint(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    // Sky: sun
    if matches!(w.act, Act::Voyage) && w.sun_x < w.cols {
        let y = 1;
        place(canvas, colors, w.sun_x, y, '*', Color::Yellow);
        if w.sun_x + 1 < w.cols { place(canvas, colors, w.sun_x + 1, y, ' ', None); }
    }

    // Horizon (sea / ground line)
    if w.horizon_y < w.rows {
        for x in 0..w.cols {
            place(canvas, colors, x, w.horizon_y, '~', Color::Blue);
        }
    }

    // Islands (Voyage only)
    if matches!(w.act, Act::Voyage) {
        for (idx, &ix) in w.stops.iter().enumerate() {
            // skip start dock (idx 0) and castle dock (idx N) — only paint islands in 1..N-1
            if idx == 0 || idx + 1 == w.stops.len() { continue; }
            let island_idx = idx - 1;          // 0..N-2
            let soldier_idx = (idx + 1) as u32; // soldier 2 stands on islands[0], etc.
            let already_spoke = soldier_idx < w.current_soldier;
            let on_boat_now = soldier_idx <= w.current_soldier;
            let tilt = w.palm_tilts.get(island_idx).copied().unwrap_or(0);
            paint_island(canvas, colors, ix, w.horizon_y, tilt);
            // Waiting soldier (visible only if they haven't spoken AND aren't currently aboard)
            if !already_spoke && !on_boat_now && w.horizon_y >= 5 {
                let y = w.horizon_y.saturating_sub(5);
                place(canvas, colors, ix, y, 'o', Color::White);
            }
        }
    }

    // Castle
    paint_castle(w, canvas, colors);

    // Cascade waterfall (Cascade / Abyss)
    if !matches!(w.act, Act::Voyage) {
        for y in 0..w.rows {
            place(canvas, colors, w.cascade_x, y, '|', Color::Cyan);
            if w.cascade_x + 1 < w.cols {
                place(canvas, colors, w.cascade_x + 1, y, '~', Color::Cyan);
            }
        }
    }

    // Satan throne (Abyss)
    if w.satan_revealed && w.rows >= 6 {
        paint_satan(w, canvas, colors);
    }

    // Boat (drawn LAST so it overlays decor)
    paint_boat(w, canvas, colors);
}

// ─── Boat ────────────────────────────────────────────────────────────
//
// Voyage frame (4 rows × 9 cols). Hull bottom sits on horizon - 1.
//
//      o          row 0 : soldier head
//     /|\         row 1 : soldier with arms
//   __|||__       row 2 : hull top + mast
//   \_____/       row 3 : hull bottom
//
fn paint_boat(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    if matches!(w.act, Act::Voyage) {
        let bx = w.boat_x;
        let by = w.boat_y;
        // Soldier head
        place(canvas, colors, bx + 4, by, 'o', Color::White);
        // Soldier arms
        place(canvas, colors, bx + 3, by + 1, '/', Color::White);
        place(canvas, colors, bx + 4, by + 1, '|', Color::White);
        place(canvas, colors, bx + 5, by + 1, '\\', Color::White);
        // Hull top with mast through
        place(canvas, colors, bx + 1, by + 2, '_', Color::DarkYellow);
        place(canvas, colors, bx + 2, by + 2, '_', Color::DarkYellow);
        place(canvas, colors, bx + 3, by + 2, '|', Color::DarkYellow);
        place(canvas, colors, bx + 4, by + 2, '|', Color::DarkYellow);
        place(canvas, colors, bx + 5, by + 2, '|', Color::DarkYellow);
        place(canvas, colors, bx + 6, by + 2, '_', Color::DarkYellow);
        place(canvas, colors, bx + 7, by + 2, '_', Color::DarkYellow);
        // Hull bottom
        place(canvas, colors, bx + 1, by + 3, '\\', Color::DarkYellow);
        for dx in 2..7 {
            place(canvas, colors, bx + dx, by + 3, '_', Color::DarkYellow);
        }
        place(canvas, colors, bx + 7, by + 3, '/', Color::DarkYellow);
        return;
    }

    // Cascade / Abyss: rotating boat, 4 frames (compact 7×3 to spin nicely)
    let frames: [[&str; 3]; 4] = [
        ["  o    ", "__|||__", "\\_____/"],     // upright
        ["    o  ", "/|||___", "/_____\\"],     // tilt right
        ["\\_____/", "__|||__", "  o    "],     // upside-down
        ["\\_____\\", "___|||\\", "  o    "],   // tilt left
    ];
    let frame = &frames[w.boat_rotation as usize % 4];
    for (dy, line) in frame.iter().enumerate() {
        let y = w.boat_y + dy;
        for (dx, ch) in line.chars().enumerate() {
            if ch != ' ' {
                place(canvas, colors, w.boat_x + dx, y, ch, Color::DarkYellow);
            }
        }
    }
}

// ─── Island & palm ───────────────────────────────────────────────────
//
// Upright (tilt=0):
//
//     ▓▓▓        row -5 : canopy top
//    ▓▓▓▓▓       row -4 : canopy bottom
//      |         row -3 : trunk top
//      |         row -2 : trunk bottom
//   ~~~~~~~      row -1 : sand (5 wide centered)
//
// `cx` is the trunk centre x; horizon_y is the row WHERE the sea line is —
// the sand replaces the sea wave on horizon_y.
fn paint_island(
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
    cx: usize,
    horizon_y: usize,
    tilt: u8,
) {
    // Sand on horizon line (5 wide)
    if horizon_y < canvas.len() {
        for dx in 0..5 {
            let x = cx.saturating_sub(2) + dx;
            place(canvas, colors, x, horizon_y, '~', Color::DarkYellow);
        }
    }
    // Trunk (2 rows). Tilt: 0 upright, 1 lean by 1, 2 lean by 1.5, 3 by 2.
    let (trunk_offsets, trunk_chars): ([i32; 2], [char; 2]) = match tilt {
        0 => ([0, 0], ['|', '|']),
        1 => ([1, 0], ['/', '|']),
        2 => ([1, 1], ['/', '/']),
        _ => ([2, 1], ['_', '/']),
    };
    if horizon_y >= 1 {
        let y = horizon_y - 1;          // trunk bottom
        let x = (cx as i32 + trunk_offsets[1]) as usize;
        place(canvas, colors, x, y, trunk_chars[1], Color::Rgb { r: 139, g: 69, b: 19 });
    }
    if horizon_y >= 2 {
        let y = horizon_y - 2;          // trunk top
        let x = (cx as i32 + trunk_offsets[0]) as usize;
        place(canvas, colors, x, y, trunk_chars[0], Color::Rgb { r: 139, g: 69, b: 19 });
    }
    // Canopy: 5-wide bottom, 3-wide top, shifted by tilt
    let canopy_dx: i32 = match tilt { 0 => 0, 1 => 1, 2 => 2, _ => 3 };
    if horizon_y >= 3 {
        let y = horizon_y - 3;
        for dx in 0..5 {
            let x = (cx as i32 - 2 + dx + canopy_dx) as usize;
            place(canvas, colors, x, y, '#', Color::Green);
        }
    }
    if horizon_y >= 4 {
        let y = horizon_y - 4;
        for dx in 0..3 {
            let x = (cx as i32 - 1 + dx + canopy_dx) as usize;
            place(canvas, colors, x, y, '#', Color::DarkGreen);
        }
    }
}

// ─── Castle ──────────────────────────────────────────────────────────
fn paint_castle(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    if matches!(w.castle_state, CastleState::Intact) && w.horizon_y >= 4 {
        let cw = w.castle_w;
        let cx = w.castle_x;
        let base_y = w.horizon_y;
        for i in 0..cw {
            let ch = if i % 2 == 0 { '#' } else { ' ' };
            if base_y >= 4 {
                place(canvas, colors, cx + i, base_y - 4, ch, Color::Grey);
            }
        }
        for dy in 1..4 {
            for i in 0..cw {
                place(canvas, colors, cx + i, base_y - dy, '#', Color::Grey);
            }
        }
        let gate_x = cx + cw / 2;
        if base_y >= 1 {
            place(canvas, colors, gate_x, base_y - 1, '|', Color::Yellow);
        }
    } else if matches!(w.castle_state, CastleState::Destroyed) && w.horizon_y >= 1 {
        let cx = w.castle_x;
        for i in 0..w.castle_w {
            place(canvas, colors, cx + i, w.horizon_y.saturating_sub(1), ',', Color::DarkGrey);
        }
    }
}

// ─── Satan throne ────────────────────────────────────────────────────
fn paint_satan(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    let sx = w.cols / 3;
    let sy = w.rows.saturating_sub(5);
    if sy + 4 >= w.rows { return; }
    // Flames row
    for dx in 0..9 {
        let ch = if dx % 2 == 0 { '\\' } else { '/' };
        place(canvas, colors, sx + dx, sy, ch, Color::Red);
    }
    // Face row
    place(canvas, colors, sx + 2, sy + 1, '(', Color::Red);
    place(canvas, colors, sx + 3, sy + 1, 'o', Color::Yellow);
    place(canvas, colors, sx + 4, sy + 1, '_', Color::Red);
    place(canvas, colors, sx + 5, sy + 1, 'o', Color::Yellow);
    place(canvas, colors, sx + 6, sy + 1, ')', Color::Red);
    // Trident row
    place(canvas, colors, sx + 1, sy + 2, '~', Color::DarkRed);
    place(canvas, colors, sx + 3, sy + 2, '|', Color::DarkRed);
    place(canvas, colors, sx + 4, sy + 2, 'Y', Color::Yellow);
    place(canvas, colors, sx + 5, sy + 2, '|', Color::DarkRed);
    place(canvas, colors, sx + 7, sy + 2, '~', Color::DarkRed);
    // Throne base
    for dx in 0..9 {
        place(canvas, colors, sx + dx, sy + 3, '#', Color::DarkRed);
    }
}

pub fn place(
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
    x: usize,
    y: usize,
    ch: char,
    color: impl Into<Option<Color>>,
) {
    if y >= canvas.len() { return; }
    let row = &mut canvas[y];
    if x >= row.len() { return; }
    row[x] = ch;
    colors[y][x] = color.into();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(total_elapsed: u32, total_budget: u32, soldiers: u32, current: u32,
            elapsed: u32, allowance: i32, cascade: bool) -> WorldState {
        WorldState::compute(&Inputs {
            total_elapsed, total_budget, soldiers, current_soldier: current,
            elapsed_current: elapsed, current_allowance: allowance,
            cols: 80, rows: 24, elapsed_ms: 0, cascade_active: cascade,
        })
    }

    #[test]
    fn voyage_unless_cascade_active() {
        // total far exceeds budget but cascade not signaled -> still voyage
        assert_eq!(make(120, 60, 3, 1, 10, 20, false).act, Act::Voyage);
        assert_eq!(make(70, 60, 3, 3, 10, 20, true).act, Act::Cascade);
        assert_eq!(make(140, 60, 3, 3, 10, 20, true).act, Act::Abyss);
    }

    #[test]
    fn n_plus_1_stops_for_n_soldiers() {
        let w = make(0, 60, 5, 1, 0, 12, false);
        assert_eq!(w.stops.len(), 6);          // start + 4 islands + castle dock
        assert_eq!(w.palm_tilts.len(), 4);     // N-1 palms
    }

    #[test]
    fn palm_tilts_only_when_overshooting() {
        let no_overshoot = make(5, 60, 3, 1, 5, 20, false);
        assert_eq!(no_overshoot.palm_tilts[0], 0);
        let overshooting = make(40, 60, 3, 1, 40, 20, false);
        assert!(overshooting.palm_tilts[0] >= 1);
    }
}
