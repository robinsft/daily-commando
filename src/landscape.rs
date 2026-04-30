// SPDX-License-Identifier: EUPL-1.2
//! Landscape narrative engine — round 4.
//!
//! Boat motion: **constant cruise** driven by `total_elapsed / total_budget`.
//! The boat's speed is fixed; what changes is what happens to islands.
//!
//! - When the current speaker hands over EARLY (anchor pull animation), the
//!   next island is dragged toward the boat. The boat does not slow down.
//! - When the current speaker OVERRUNS, the boat sails right past the next
//!   island. An anchor + rope tow that island to **2 cols behind the boat**
//!   and drags it along until the user manually advances. The waiting
//!   soldier sits on the towed island.
//!
//! Hell staging (driven by `total_elapsed / total_budget`):
//!   * `< 1.0`           — Voyage (sun + sea).
//!   * `1.0..2.0`        — Cascade: boat falls past castle into the void.
//!   * `2.0..2.5`        — Satan's face appears at the bottom.
//!   * `2.5..3.0`        — Satan's trident appears too.
//!   * `>= 3.0`          — Full hell: boat crashes (debris), screen turns
//!                          red, flames everywhere.

use crossterm::style::Color;

#[derive(Debug, Clone)]
pub struct WorldState {
    pub cols: usize,
    pub rows: usize,
    pub act: Act,
    pub satan_stage: SatanStage,
    pub sun_x: usize,
    pub boat_x: usize,
    pub boat_y: usize,
    pub boat_rotation: u8,
    pub boat_wrecked: bool,
    pub horizon_y: usize,
    /// `N+1` original anchor x-positions: `[start, island_1, …, island_{N-1}, castle_dock]`.
    pub stops: Vec<usize>,
    /// Per-island state (length `N - 1`).
    pub islands: Vec<IslandState>,
    /// Islands that are done — permanent landmarks with colored flags.
    pub settled: Vec<SettledIsland>,
    pub current_soldier: u32,
    pub soldiers: u32,
    pub castle_x: usize,
    pub castle_w: usize,
    pub castle_state: CastleState,
    pub cascade_x: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act { Voyage, Cascade, Hell }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatanStage { None, Face, FaceTrident, FullHell }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastleState { Intact, Destroyed }

/// Per-island visual state.
#[derive(Debug, Clone, Copy)]
pub struct IslandState {
    /// Current x position (== original_x unless towed/pulled).
    pub x: usize,
    /// True when the boat has passed the island without picking up its
    /// soldier — island is being towed by an anchor at `boat_x - 2`.
    pub towed: bool,
    /// True when the soldier has already boarded (island consumed → invisible).
    pub consumed: bool,
    /// True when an anchor-pull animation is running on this island; we draw
    /// the rope but freeze it at `pull_x`.
    pub pulling: bool,
    /// Slight tilt 0..3 (only used while towed/pulling for visual feedback).
    pub tilt: u8,
}

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
    /// Override: current speaker has hit "next" early; the destination
    /// island is being pulled toward the boat over `pull_progress` 0..1.
    pub anchor_pull: Option<AnchorPull>,
    /// Islands that are done (soldier boarded) — kept in the landscape with
    /// a colored flag showing the soldier's name.
    pub settled: Vec<SettledIsland>,
}

#[derive(Debug, Clone, Copy)]
pub struct AnchorPull {
    /// Stop index being pulled (1-based: 1..=N). `N` = castle.
    pub stop_index: usize,
    /// 0.0 = at original position, 1.0 = at boat.
    pub progress: f32,
}

/// An island that has been "consumed" (its soldier boarded) and now sits
/// permanently in the decor as a landmark with a colored flag.
#[derive(Debug, Clone)]
pub struct SettledIsland {
    /// Final x position where the island stopped.
    pub x: usize,
    /// True if the soldier handed over early (flag = green), false if late (red).
    pub was_early: bool,
    /// Soldier name displayed on the flag.
    pub name: String,
}

impl WorldState {
    pub fn compute(inp: &Inputs) -> Self {
        let cols = inp.cols.max(40);
        let rows = inp.rows.max(12);
        let budget = inp.total_budget.max(1);

        // ── Castle geometry ─────────────────────────────────────────
        let castle_w = (cols / 10).clamp(7, 15);
        let castle_x = cols.saturating_sub(castle_w + 2);

        // ── Stops: N+1 anchor x positions ──────────────────────────
        let n = inp.soldiers.max(1) as usize;
        let start_x: usize = 4;
        let castle_dock_x: usize = castle_x.saturating_sub(2);
        let stops: Vec<usize> = if n == 1 {
            vec![start_x, castle_dock_x]
        } else {
            let span = castle_dock_x.saturating_sub(start_x);
            (0..=n).map(|k| start_x + (span as f32 * (k as f32 / n as f32)) as usize).collect()
        };

        // ── Constant-cruise boat ───────────────────────────────────
        // Boat goes from start_x to castle_dock_x linearly with
        // total_elapsed/total_budget. After 1.0 it falls (Cascade/Hell).
        let progress = inp.total_elapsed as f32 / budget as f32;
        let cruise = progress.min(1.0);
        let voyage_boat_x = start_x + ((castle_dock_x - start_x) as f32 * cruise) as usize;
        let voyage_boat_y = rows.saturating_sub(6);

        // ── Acts & Satan staging ───────────────────────────────────
        let (act, satan_stage) = match progress {
            p if p < 1.0 => (Act::Voyage, SatanStage::None),
            p if p < 2.0 => (Act::Cascade, SatanStage::None),
            p if p < 2.5 => (Act::Cascade, SatanStage::Face),
            p if p < 3.0 => (Act::Cascade, SatanStage::FaceTrident),
            _            => (Act::Hell,    SatanStage::FullHell),
        };

        // ── Boat per act ───────────────────────────────────────────
        let (boat_x, boat_y, boat_rotation, boat_wrecked);
        match act {
            Act::Voyage => {
                boat_x = voyage_boat_x;
                boat_y = voyage_boat_y;
                boat_rotation = 0;
                boat_wrecked = false;
            }
            Act::Cascade => {
                // Boat: continues right past castle, then falls vertically
                // along the right side. Fall progress in 0..1 across 1x..3x.
                let fall = ((progress - 1.0) / 2.0).clamp(0.0, 1.0);
                let horiz = (fall / 0.2).clamp(0.0, 1.0);              // first 20 % of fall
                let vert = ((fall - 0.2) / 0.8).clamp(0.0, 1.0);
                let bx = castle_dock_x
                    + ((cols.saturating_sub(castle_dock_x + 9)) as f32 * horiz) as usize;
                boat_x = bx.min(cols.saturating_sub(9));
                let max_fall = rows.saturating_sub(4);
                boat_y = voyage_boat_y + ((max_fall.saturating_sub(voyage_boat_y)) as f32 * vert) as usize;
                boat_rotation = ((inp.elapsed_ms / 250) % 4) as u8;
                boat_wrecked = false;
            }
            Act::Hell => {
                // Boat is now a wreck on the floor.
                boat_x = cols / 2 - 4;
                boat_y = rows.saturating_sub(4);
                boat_rotation = 0;
                boat_wrecked = true;
            }
        }

        // ── Horizon ────────────────────────────────────────────────
        let horizon_y = match act {
            Act::Voyage => rows.saturating_sub(2),
            Act::Cascade => {
                // Horizon slowly rises with cascade progress.
                let secs = (inp.total_elapsed as i32 - budget as i32).max(0) as usize;
                rows.saturating_sub(2).saturating_sub(secs / 2).max(rows / 5)
            }
            Act::Hell => rows.saturating_sub(2),
        };

        // ── Sun ────────────────────────────────────────────────────
        let sun_x = if matches!(act, Act::Voyage) {
            ((cols as f32 - 4.0) * progress.min(1.0)) as usize
        } else { 0 };

        // ── Islands state machine ──────────────────────────────────
        // For island k (0-based, holding soldier k+2) sitting at stops[k+1]:
        //   - consumed if soldier k+2 has already boarded (current_soldier > k+1)
        //   - towed   if its soldier == current_soldier+1 AND boat passed it
        //   - pulling if anchor_pull.stop_index == k+1 (animation in progress)
        //   - else: idle at stops[k+1]
        let mut islands = Vec::with_capacity(n.saturating_sub(1));
        let cur = inp.current_soldier as usize;
        for k in 0..n.saturating_sub(1) {
            let original_x = stops[k + 1];
            let waiting_soldier = (k + 2) as u32;          // 1-based
            let consumed = waiting_soldier <= inp.current_soldier;
            let mut x = original_x;
            let mut towed = false;
            let mut pulling = false;
            let mut tilt = 0u8;
            if !consumed {
                // Anchor-pull animation overrides position.
                if let Some(ap) = inp.anchor_pull {
                    if ap.stop_index == k + 1 {
                        let p = ap.progress.clamp(0.0, 1.0);
                        let dest = boat_x.saturating_add(2);    // arrive next to boat
                        x = lerp_x(original_x, dest, p);
                        pulling = true;
                        tilt = (p * 3.0).round() as u8;
                    }
                }
                // If not animating, check if the boat passed: the destination
                // of the *current* speaker is stops[cur]; the island holding
                // the next soldier is k+1, so it's "pushable" when k+1 == cur.
                if !pulling && k + 1 == cur && matches!(act, Act::Voyage) && boat_x > original_x {
                    // Tow at boat_x - 2 (two characters behind the boat's left edge)
                    let towed_x = boat_x.saturating_sub(2);
                    x = towed_x;
                    towed = true;
                    // tilt grows with how far past the original we are
                    let dist = boat_x.saturating_sub(original_x) as f32;
                    tilt = ((dist / 8.0).clamp(0.0, 1.0) * 3.0).round() as u8;
                }
            }
            islands.push(IslandState { x, towed, consumed, pulling, tilt });
        }

        let castle_state = if matches!(act, Act::Voyage) || (matches!(act, Act::Cascade) && progress < 1.05) {
            CastleState::Intact
        } else {
            CastleState::Destroyed
        };
        let cascade_x = cols.saturating_sub(4);

        WorldState {
            cols, rows, act, satan_stage, sun_x,
            boat_x, boat_y, boat_rotation, boat_wrecked,
            horizon_y, stops, islands,
            settled: inp.settled.clone(),
            current_soldier: inp.current_soldier, soldiers: inp.soldiers,
            castle_x, castle_w, castle_state, cascade_x,
        }
    }
}

fn lerp_x(from: usize, to: usize, p: f32) -> usize {
    let f = from as i32 as f32;
    let t = to as i32 as f32;
    (f + (t - f) * p).round() as i32 as usize
}

/// Paint the entire decor onto the (canvas, color_map). Call BEFORE the timer.
pub fn paint(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    // Sky: sun
    if matches!(w.act, Act::Voyage) && w.sun_x < w.cols {
        place(canvas, colors, w.sun_x, 1, '*', Color::Yellow);
    }

    // Horizon (sea / ground line) — only in Voyage and Cascade
    if !matches!(w.act, Act::Hell) && w.horizon_y < w.rows {
        for x in 0..w.cols {
            place(canvas, colors, x, w.horizon_y, '~', Color::Blue);
        }
    }

    // Hell flames background (Hell only)
    if matches!(w.act, Act::Hell) {
        paint_hell_flames(w, canvas, colors);
    }

    // Islands (Voyage only)
    if matches!(w.act, Act::Voyage) {
        // Draw settled (consumed) islands first — they're background landmarks.
        for si in &w.settled {
            paint_island(canvas, colors, si.x, w.horizon_y, 0);
            // Flag pole + colored flag with soldier name
            if w.horizon_y >= 5 {
                let pole_x = si.x;
                let flag_color = if si.was_early { Color::Green } else { Color::Red };
                // Pole
                place(canvas, colors, pole_x, w.horizon_y - 5, '|', Color::White);
                place(canvas, colors, pole_x, w.horizon_y - 6, '|', Color::White);
                // Flag (name, max 4 chars, to the right of the pole)
                let label: String = si.name.chars().take(4).collect();
                for (i, ch) in label.chars().enumerate() {
                    let fx = pole_x + 1 + i;
                    if fx < w.cols {
                        place(canvas, colors, fx, w.horizon_y - 6, ch, flag_color);
                    }
                }
            }
        }

        // Active (not-yet-consumed) islands.
        for (k, isl) in w.islands.iter().enumerate() {
            if isl.consumed { continue; }
            paint_island(canvas, colors, isl.x, w.horizon_y, isl.tilt);
            // Waiting soldier head
            if w.horizon_y >= 5 {
                place(canvas, colors, isl.x, w.horizon_y - 5, 'o', Color::White);
            }
            // Tow rope from boat to island (when towed or pulling)
            if (isl.towed || isl.pulling) && w.horizon_y >= 1 {
                let rope_y = w.horizon_y - 1;
                let (lo, hi) = if isl.x < w.boat_x {
                    (isl.x + 1, w.boat_x)
                } else {
                    (w.boat_x + 9, isl.x)
                };
                for x in lo..hi {
                    place(canvas, colors, x, rope_y, '-', Color::DarkGrey);
                }
                // Anchor at the island side
                let anchor_x = if isl.x < w.boat_x { isl.x + 1 } else { isl.x.saturating_sub(1) };
                place(canvas, colors, anchor_x, rope_y, 'J', Color::DarkGrey);
            }
            let _ = k;
        }
    }

    // Castle
    paint_castle(w, canvas, colors);

    // Cascade waterfall (Cascade only — Hell removes it)
    if matches!(w.act, Act::Cascade) {
        for y in 0..w.rows {
            place(canvas, colors, w.cascade_x, y, '|', Color::Cyan);
            if w.cascade_x + 1 < w.cols {
                place(canvas, colors, w.cascade_x + 1, y, '~', Color::Cyan);
            }
        }
    }

    // Satan staging
    paint_satan(w, canvas, colors);

    // Boat
    paint_boat(w, canvas, colors);
}

// ─── Boat ────────────────────────────────────────────────────────────
fn paint_boat(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    if w.boat_wrecked {
        // Wreck: scattered planks, smoke, fire
        let bx = w.boat_x;
        let by = w.boat_y;
        let parts = [
            (0, 0, '\\', Color::DarkYellow),
            (1, 0, '_', Color::DarkYellow),
            (2, 0, '/', Color::DarkYellow),
            (4, 0, '|', Color::DarkYellow),
            (6, 0, '\\', Color::DarkYellow),
            (7, 0, '_', Color::DarkYellow),
            // smoke / fire above
            (1, -1, '~', Color::Red),
            (3, -1, '*', Color::Yellow),
            (5, -1, '~', Color::Red),
        ];
        for (dx, dy, ch, c) in parts {
            let x = bx as i32 + dx;
            let y = by as i32 + dy;
            if x >= 0 && y >= 0 { place(canvas, colors, x as usize, y as usize, ch, c); }
        }
        return;
    }

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

    // Cascade: rotating boat
    let frames: [[&str; 3]; 4] = [
        ["  o    ", "__|||__", "\\_____/"],
        ["    o  ", "/|||___", "/_____\\"],
        ["\\_____/", "__|||__", "  o    "],
        ["\\_____\\", "___|||\\", "  o    "],
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
fn paint_island(
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
    cx: usize,
    horizon_y: usize,
    tilt: u8,
) {
    if horizon_y == 0 { return; }
    // Sand on horizon (5 wide)
    if horizon_y < canvas.len() {
        for dx in 0..5 {
            let x = cx.saturating_sub(2) + dx;
            place(canvas, colors, x, horizon_y, '~', Color::DarkYellow);
        }
    }
    let brown = Color::Rgb { r: 139, g: 69, b: 19 };
    // Trunk
    let (trunk_offsets, trunk_chars): ([i32; 2], [char; 2]) = match tilt {
        0 => ([0, 0], ['|', '|']),
        1 => ([1, 0], ['/', '|']),
        2 => ([1, 1], ['/', '/']),
        _ => ([2, 1], ['_', '/']),
    };
    if horizon_y >= 1 {
        let y = horizon_y - 1;
        let x = (cx as i32 + trunk_offsets[1]).max(0) as usize;
        place(canvas, colors, x, y, trunk_chars[1], brown);
    }
    if horizon_y >= 2 {
        let y = horizon_y - 2;
        let x = (cx as i32 + trunk_offsets[0]).max(0) as usize;
        place(canvas, colors, x, y, trunk_chars[0], brown);
    }
    // Canopy (5-wide bottom, 3-wide top)
    let canopy_dx: i32 = match tilt { 0 => 0, 1 => 1, 2 => 2, _ => 3 };
    if horizon_y >= 3 {
        let y = horizon_y - 3;
        for dx in 0..5 {
            let x = (cx as i32 - 2 + dx + canopy_dx).max(0) as usize;
            place(canvas, colors, x, y, '#', Color::Green);
        }
    }
    if horizon_y >= 4 {
        let y = horizon_y - 4;
        for dx in 0..3 {
            let x = (cx as i32 - 1 + dx + canopy_dx).max(0) as usize;
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
    // Hell removes the castle entirely — boat is on the hell floor.
    if matches!(w.act, Act::Hell) { return; }
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

// ─── Satan staging ───────────────────────────────────────────────────
fn paint_satan(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    if matches!(w.satan_stage, SatanStage::None) { return; }
    let sx = w.cols / 3;
    let sy = w.rows.saturating_sub(5);
    if sy + 4 >= w.rows { return; }
    // Flames row
    for dx in 0..9 {
        let ch = if dx % 2 == 0 { '\\' } else { '/' };
        place(canvas, colors, sx + dx, sy, ch, Color::Red);
    }
    // Face row (always once Satan is visible)
    place(canvas, colors, sx + 2, sy + 1, '(', Color::Red);
    place(canvas, colors, sx + 3, sy + 1, 'o', Color::Yellow);
    place(canvas, colors, sx + 4, sy + 1, '_', Color::Red);
    place(canvas, colors, sx + 5, sy + 1, 'o', Color::Yellow);
    place(canvas, colors, sx + 6, sy + 1, ')', Color::Red);
    // Trident row (FaceTrident or FullHell)
    if !matches!(w.satan_stage, SatanStage::Face) {
        place(canvas, colors, sx + 1, sy + 2, '\\', Color::DarkRed);
        place(canvas, colors, sx + 2, sy + 2, 'V', Color::Yellow);
        place(canvas, colors, sx + 3, sy + 2, 'V', Color::Yellow);
        place(canvas, colors, sx + 4, sy + 2, 'V', Color::Yellow);
        place(canvas, colors, sx + 5, sy + 2, '|', Color::DarkRed);
        place(canvas, colors, sx + 6, sy + 2, '|', Color::DarkRed);
        place(canvas, colors, sx + 7, sy + 2, '/', Color::DarkRed);
    }
    // Throne base
    for dx in 0..9 {
        place(canvas, colors, sx + dx, sy + 3, '#', Color::DarkRed);
    }
}

// ─── Hell flames background ──────────────────────────────────────────
fn paint_hell_flames(
    w: &WorldState,
    canvas: &mut [Vec<char>],
    colors: &mut [Vec<Option<Color>>],
) {
    // Fill the screen with flickering red/orange flame characters using a
    // cheap deterministic pseudo-random based on (x, y, time bucket).
    let palette = [Color::Red, Color::DarkRed, Color::Yellow, Color::Red, Color::DarkRed];
    let chars = ['^', '\\', '/', '|', 'A', 'V', '*', '~'];
    for y in 1..w.rows.saturating_sub(1) {
        for x in 0..w.cols {
            let h = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503) ^ ((y % 3) * 17)) as usize;
            let ch = chars[h % chars.len()];
            let col = palette[(h >> 3) % palette.len()];
            place(canvas, colors, x, y, ch, col);
        }
    }
    // Ground line (bottom) of bright orange
    let gy = w.rows.saturating_sub(2);
    for x in 0..w.cols {
        place(canvas, colors, x, gy, '#', Color::Yellow);
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

    fn make(progress: f32, soldiers: u32, current: u32, elapsed: u32, ap: Option<AnchorPull>) -> WorldState {
        let budget = 60u32;
        let total_elapsed = (budget as f32 * progress) as u32;
        WorldState::compute(&Inputs {
            total_elapsed, total_budget: budget, soldiers,
            current_soldier: current, elapsed_current: elapsed,
            current_allowance: 20, cols: 80, rows: 24, elapsed_ms: 0,
            anchor_pull: ap,
            settled: vec![],
        })
    }

    #[test]
    fn satan_stages_at_2x_2_5x_3x() {
        assert_eq!(make(0.5, 3, 1, 5, None).satan_stage, SatanStage::None);
        assert_eq!(make(1.5, 3, 3, 90, None).satan_stage, SatanStage::None);
        assert_eq!(make(2.1, 3, 3, 120, None).satan_stage, SatanStage::Face);
        assert_eq!(make(2.6, 3, 3, 150, None).satan_stage, SatanStage::FaceTrident);
        assert_eq!(make(3.1, 3, 3, 180, None).satan_stage, SatanStage::FullHell);
    }

    #[test]
    fn hell_act_at_3x() {
        assert_eq!(make(2.9, 3, 3, 170, None).act, Act::Cascade);
        let w = make(3.1, 3, 3, 180, None);
        assert_eq!(w.act, Act::Hell);
        assert!(w.boat_wrecked);
    }

    #[test]
    fn boat_constant_cruise_independent_of_speaker() {
        // Same total_elapsed → same boat_x regardless of which speaker.
        let a = make(0.5, 4, 1, 30, None);
        let b = make(0.5, 4, 3, 30, None);
        assert_eq!(a.boat_x, b.boat_x);
    }

    #[test]
    fn island_towed_when_boat_passes_without_pickup() {
        // 4 soldiers, boat at progress=0.4 (40% of cruise). Stops are at
        // 0%, 25%, 50%, 75%, 100%. Boat is past stops[1] (25%) so island 0
        // (the destination of speaker 1) should be towed.
        let w = make(0.4, 4, 1, 24, None);
        assert!(w.islands[0].towed, "island 0 should be towed at progress 0.4 with current=1");
        // Island 1 (destination of speaker 2) is NOT towed yet (boat hasn't arrived).
        assert!(!w.islands[1].towed);
    }

    #[test]
    fn anchor_pull_overrides_island_position() {
        let w = make(0.2, 4, 1, 12, Some(AnchorPull { stop_index: 1, progress: 0.5 }));
        assert!(w.islands[0].pulling);
        assert!(!w.islands[0].towed);
    }
}
