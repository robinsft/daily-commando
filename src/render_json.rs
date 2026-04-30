// SPDX-License-Identifier: EUPL-1.2
//! JSON renderer: emits one snapshot per second + a final summary line.
//! No interactive welcome — the CLI flags fully define the session.

use anyhow::Result;
use daily_commando::{telemetry, Phase, Session};
use serde::Serialize;
use std::io::{self, Write};
use std::thread::sleep;
use std::time::Duration;
use tracing::info_span;

#[derive(Serialize)]
struct Summary<'a> {
    event: &'a str,
    phase: Phase,
    stats: Vec<daily_commando::SoldierStats>,
    total_elapsed: u32,
    prep_seconds: u32,
    closing_seconds: u32,
}

pub fn run(mut session: Session) -> Result<()> {
    let mut out = io::stdout().lock();

    // ── Preparation: 0s in JSON mode (no interactive welcome) ────────
    let prep_span = info_span!("session.preparation").entered();
    session.commence();
    drop(prep_span);

    // ── Running ──────────────────────────────────────────────────────
    let run_span = info_span!("session.running").entered();
    loop {
        let snap = session.snapshot();
        telemetry::record(&snap, &session.stats());
        writeln!(out, "{}", serde_json::to_string(&snap)?)?;
        out.flush()?;

        if matches!(snap.phase, Phase::Closing | Phase::Aborted) { break; }

        sleep(Duration::from_secs(1));
        session.tick_one_second();
        telemetry::count_phase_second(session.phase());

        let s = session.snapshot();
        if s.elapsed_current >= s.per_soldier_seconds {
            session.next_soldier();
        }
    }
    drop(run_span);

    // ── Closing: short fixed window so the JSON pipeline has a proper end ─
    let close_span = info_span!("session.closing").entered();
    for _ in 0..3 {
        let snap = session.snapshot();
        telemetry::record(&snap, &session.stats());
        writeln!(out, "{}", serde_json::to_string(&snap)?)?;
        out.flush()?;
        sleep(Duration::from_secs(1));
        session.tick_one_second();
        telemetry::count_phase_second(session.phase());
    }
    drop(close_span);

    let snap = session.snapshot();
    let summary = Summary {
        event: "session_finished",
        phase: session.phase(),
        stats: session.stats(),
        total_elapsed: snap.total_elapsed,
        prep_seconds: snap.prep_seconds,
        closing_seconds: snap.closing_seconds,
    };
    writeln!(out, "{}", serde_json::to_string(&summary)?)?;
    Ok(())
}
