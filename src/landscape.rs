// SPDX-License-Identifier: EUPL-1.2
//! Landscape narrative engine.
//!
//! Three acts driven by `total_elapsed` vs `total_budget`:
//!  * **Voyage** (0..1×) — sun rising/setting, boat sailing past N-1 islands
//!    with waiting soldiers, towards a castle on the right edge.
//!  * **Cascade** (1×..2×) — castle destroyed, boat falls through an infinite
//!    waterfall on the right while the horizon scrolls upward.
//!  * **Abyss** (>2×) — Satan's throne reveals at the bottom, world ends.
//!
//! All decor is painted onto a char canvas so the big timer overlays cleanly.
//!
//! No I/O, no terminal calls — just pure state + paint(canvas).

use crossterm::style::Color;

#[derive(Debug, Clone)]
pub struct WorldState {
    pub cols: usize,
    pub rows: usize,
    pub act: Act,
    pub sun_x: usize,
    pub boat_x: usize,
    pub boat_y: usize,
    pub boat_rotation: u8,        // 0..4
    pub horizon_y: usize,         // ground line
    pub island_xs: Vec<usize>,    // x-positions of N-1 islands
    pub current_soldier: u32,     // 1-based
    pub soldiers: u32,
    pub castle_x: usize,
    pub castle_w: usize,
    pub castle_destroyed: bool,
    pub satan_revealed: bool,
    pub cascade_x: usize,         // fixed x on the right edge
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act { Voyage, Cascade, Abyss }

impl WorldState {
    pub fn compute(
        total_elapsed: u32,
        total_budget: u32,
        soldiers: u32,
        current_soldier: u32,
        cols: usize,
        rows: usize,
        elapsed_ms: u64,
    ) -> Self {
        let budget = total_budget.max(1);
        let progress = (total_elapsed as f32 / budget as f32).min(99.0);

        let act = if progress < 1.0 { Act::Voyage }
            else if progress < 2.0 { Act::Cascade }
            else { Act::Abyss };

        // Sun: rises east (right) at t=0, sets west (left) at t=budget.
        let sun_x = if matches!(act, Act::Voyage) {
            let frac = (1.0 - progress).max(0.0);
            ((cols as f32 - 4.0) * (1.0 - frac)) as usize
        } else { 0 };

        // Castle: 10% width, min 5 max 15 cols, on the right edge.
        let castle_w = (cols / 10).clamp(5, 15);
        let castle_x = cols.saturating_sub(castle_w + 1);
        let castle_destroyed = !matches!(act, Act::Voyage);

        // Boat: voyage = travels left→right; cascade = falls; abyss = stuck at 2/3 down.
        let boat_x;
        let boat_y;
        let mut boat_rotation = 0;

        match act {
            Act::Voyage => {
                let travel = cols.saturating_sub(castle_w + 6);
                boat_x = 3 + (travel as f32 * progress.min(1.0)) as usize;
                boat_y = rows.saturating_sub(2);
            }
            Act::Cascade => {
                // boat falls along the right side
                boat_x = cols.saturating_sub(castle_w / 2 + 4);
                let fall_progress = (progress - 1.0).clamp(0.0, 1.0);
                let max_fall = (rows as f32 * 2.0 / 3.0) as usize;
                boat_y = (max_fall as f32 * fall_progress) as usize;
                boat_rotation = ((elapsed_ms / 250) % 4) as u8;
            }
            Act::Abyss => {
                boat_x = cols.saturating_sub(castle_w / 2 + 4);
                // stuck at 2/3 height, still spinning
                boat_y = (rows as f32 * 2.0 / 3.0) as usize;
                boat_rotation = ((elapsed_ms / 200) % 4) as u8;
            }
        }

        // Horizon: ground line. Voyage = rows-1. Cascade = rises by 1/sec.
        let horizon_y = match act {
            Act::Voyage => rows.saturating_sub(1),
            Act::Cascade => {
                let secs_in_cascade = ((progress - 1.0) * budget as f32) as usize;
                rows.saturating_sub(1).saturating_sub(secs_in_cascade).max(rows / 4)
            }
            Act::Abyss => rows / 4,
        };

        // Islands: N-1 evenly spaced between boat-start and castle.
        let n_islands = soldiers.saturating_sub(1) as usize;
        let island_xs: Vec<usize> = if n_islands == 0 {
            vec![]
        } else {
            let span = cols.saturating_sub(castle_w + 10);
            (0..n_islands)
                .map(|i| 6 + (span as f32 * (i as f32 + 1.0) / (n_islands as f32 + 1.0)) as usize)
                .collect()
        };

        let satan_revealed = matches!(act, Act::Abyss);
        let cascade_x = cols.saturating_sub(3);

        WorldState {
            cols, rows, act, sun_x, boat_x, boat_y, boat_rotation, horizon_y,
            island_xs, current_soldier, soldiers, castle_x, castle_w,
            castle_destroyed, satan_revealed, cascade_x,
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
        let y = 2;
        place(canvas, colors, w.sun_x, y, '☼', Color::Yellow);
        if w.sun_x + 1 < w.cols { place(canvas, colors, w.sun_x + 1, y, ' ', None); }
    }

    // Horizon line (ground / sea level)
    if w.horizon_y < w.rows {
        for x in 0..w.cols {
            place(canvas, colors, x, w.horizon_y, '~', Color::Blue);
        }
    }

    // Islands + waiting soldiers (only during Voyage)
    if matches!(w.act, Act::Voyage) {
        for (idx, &ix) in w.island_xs.iter().enumerate() {
            let soldier_idx = (idx + 2) as u32; // soldier 2, 3, …, N waits on islands
            let already_spoke = soldier_idx < w.current_soldier;
            let on_boat_now = soldier_idx == w.current_soldier;
            // palmier
            if w.horizon_y >= 2 {
                place(canvas, colors, ix, w.horizon_y.saturating_sub(2), 'Y', Color::DarkGreen);
            }
            // sand
            if w.horizon_y >= 1 {
                let y = w.horizon_y.saturating_sub(1);
                for dx in 0..3 {
                    place(canvas, colors, ix.saturating_sub(1) + dx, y, '~', Color::DarkYellow);
                }
            }
            // waiting soldier
            if !already_spoke && !on_boat_now && w.horizon_y >= 3 {
                let y = w.horizon_y.saturating_sub(3);
                place(canvas, colors, ix, y, 'o', Color::White);
            }
        }
    }

    // Castle (right edge) — Voyage = intact, Cascade/Abyss = destroyed pile
    if !w.castle_destroyed && w.horizon_y >= 4 {
        let cw = w.castle_w;
        let cx = w.castle_x;
        let base_y = w.horizon_y;
        // crenelations
        if base_y >= 4 {
            for i in 0..cw {
                let ch = if i % 2 == 0 { '█' } else { ' ' };
                place(canvas, colors, cx + i, base_y - 4, ch, Color::Grey);
            }
        }
        // walls
        for dy in 1..4 {
            for i in 0..cw {
                place(canvas, colors, cx + i, base_y - dy, '█', Color::Grey);
            }
        }
        // gate
        let gate_x = cx + cw / 2;
        if base_y >= 1 {
            place(canvas, colors, gate_x, base_y - 1, '║', Color::Yellow);
        }
    } else if w.castle_destroyed && w.horizon_y >= 1 {
        let cx = w.castle_x;
        for i in 0..w.castle_w {
            place(canvas, colors, cx + i, w.horizon_y.saturating_sub(1), '▒', Color::DarkGrey);
        }
    }

    // Cascade: vertical waterfall on the right side, only in Cascade/Abyss
    if !matches!(w.act, Act::Voyage) {
        for y in 0..w.rows {
            place(canvas, colors, w.cascade_x, y, '║', Color::Cyan);
            if w.cascade_x >= 1 {
                place(canvas, colors, w.cascade_x - 1, y, '~', Color::Cyan);
            }
        }
    }

    // Satan throne (Abyss only) at bottom
    if w.satan_revealed && w.rows >= 5 {
        let sx = w.cols / 3;
        let sy = w.rows.saturating_sub(4);
        // fire
        for dx in 0..7 {
            place(canvas, colors, sx + dx, sy, '\\', Color::Red);
            place(canvas, colors, sx + dx + 1, sy, '/', Color::Red);
        }
        place(canvas, colors, sx + 2, sy + 1, '(', Color::Red);
        place(canvas, colors, sx + 3, sy + 1, 'o', Color::Yellow);
        place(canvas, colors, sx + 4, sy + 1, '_', Color::Red);
        place(canvas, colors, sx + 5, sy + 1, 'o', Color::Yellow);
        place(canvas, colors, sx + 6, sy + 1, ')', Color::Red);
        place(canvas, colors, sx + 1, sy + 2, '~', Color::DarkRed);
        place(canvas, colors, sx + 2, sy + 2, '|', Color::DarkRed);
        place(canvas, colors, sx + 3, sy + 2, 'Y', Color::Yellow);
        place(canvas, colors, sx + 4, sy + 2, '|', Color::DarkRed);
        place(canvas, colors, sx + 5, sy + 2, '~', Color::DarkRed);
        // throne base
        for dx in 0..9 {
            place(canvas, colors, sx + dx, sy + 3, '█', Color::DarkRed);
        }
    }

    // Boat (always visible, drawn last so it sits on top of decor)
    paint_boat(w, canvas, colors);
}

fn paint_boat(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    // Boat ASCII art per rotation (4 frames).
    // Each frame is 2 rows tall × 5 cols wide.
    let frames: [[&str; 2]; 4] = [
        ["\\_/_/", "~~~~~"],   // upright
        ["\\_/__", "~~~~/"],   // tilting
        ["__/_/", "/~~~~"],    // upside-ish
        ["_/_/_", "_~~~~"],    // tilting back
    ];
    let frame = &frames[w.boat_rotation as usize % 4];
    for (dy, line) in frame.iter().enumerate() {
        let y = w.boat_y + dy;
        for (dx, ch) in line.chars().enumerate() {
            let x = w.boat_x + dx;
            if ch != ' ' {
                place(canvas, colors, x, y, ch, Color::DarkYellow);
            }
        }
    }
    // Boarded soldier on top of boat (during Voyage, soldier 1 is always on board;
    // others get teleported when their turn starts)
    if matches!(w.act, Act::Voyage) && w.boat_y >= 1 {
        place(canvas, colors, w.boat_x + 2, w.boat_y - 1, 'o', Color::White);
    }
}

fn place(
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

    #[test]
    fn voyage_then_cascade_then_abyss() {
        let w1 = WorldState::compute(0, 60, 3, 1, 80, 24, 0);
        assert_eq!(w1.act, Act::Voyage);
        let w2 = WorldState::compute(80, 60, 3, 3, 80, 24, 0);
        assert_eq!(w2.act, Act::Cascade);
        let w3 = WorldState::compute(140, 60, 3, 3, 80, 24, 0);
        assert_eq!(w3.act, Act::Abyss);
    }

    #[test]
    fn n_minus_1_islands() {
        let w = WorldState::compute(10, 60, 5, 2, 80, 24, 0);
        assert_eq!(w.island_xs.len(), 4);
    }

    #[test]
    fn castle_width_clamped() {
        let w_small = WorldState::compute(0, 60, 3, 1, 30, 24, 0);
        assert_eq!(w_small.castle_w, 5);
        let w_big = WorldState::compute(0, 60, 3, 1, 200, 24, 0);
        assert_eq!(w_big.castle_w, 15);
    }
}
