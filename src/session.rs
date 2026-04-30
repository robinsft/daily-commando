// SPDX-License-Identifier: EUPL-1.2
//! Daily standup session state machine.
//!
//! Pure logic, no I/O. The same struct is driven from a TUI today and will be
//! driven from HTTP/WebSocket handlers tomorrow.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub soldiers: u32,
    pub total_seconds: u32,
    pub names: Vec<String>,
}

impl SessionConfig {
    pub fn per_soldier_seconds(&self) -> u32 {
        if self.soldiers == 0 { 0 } else { self.total_seconds / self.soldiers }
    }
    pub fn name_of(&self, idx: u32) -> String {
        self.names
            .get(idx as usize)
            .cloned()
            .unwrap_or_else(|| format!("Soldier {}", idx + 1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// App started, user filling the welcome form.
    Preparation,
    /// Daily in progress, current soldier speaking.
    Running,
    /// Timer paused mid-soldier.
    Paused,
    /// All soldiers done, debrief shown, app still open.
    Closing,
    /// User aborted before the end.
    Aborted,
    /// Final terminal state (after Closing → app exit).
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoldierStats {
    pub index: u32,
    pub name: String,
    pub elapsed_seconds: u32,
    pub overtime_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tick {
    pub phase: Phase,
    pub current_soldier: u32,
    pub current_name: String,
    pub soldiers: u32,
    pub per_soldier_seconds: u32,
    pub elapsed_current: u32,
    pub remaining_current: i32,
    pub total_elapsed: u32,
    pub total_budget: u32,
    pub prep_seconds: u32,
    pub closing_seconds: u32,
}

#[derive(Debug, Clone)]
pub struct Session {
    config: SessionConfig,
    phase: Phase,
    current: u32,
    elapsed: Vec<u32>,
    prep_seconds: u32,
    closing_seconds: u32,
}

impl Session {
    /// New session in `Preparation` phase. Config can still be mutated until
    /// `commence()` is called.
    pub fn new(config: SessionConfig) -> Self {
        let n = config.soldiers.max(1) as usize;
        Self {
            config,
            phase: Phase::Preparation,
            current: 0,
            elapsed: vec![0; n],
            prep_seconds: 0,
            closing_seconds: 0,
        }
    }

    pub fn config(&self) -> &SessionConfig { &self.config }
    pub fn phase(&self) -> Phase { self.phase }
    pub fn current_index(&self) -> u32 { self.current }
    pub fn elapsed(&self) -> &[u32] { &self.elapsed }
    pub fn prep_seconds(&self) -> u32 { self.prep_seconds }
    pub fn closing_seconds(&self) -> u32 { self.closing_seconds }

    /// Replace the configuration while still in `Preparation`. No-op otherwise.
    pub fn update_config(&mut self, config: SessionConfig) {
        if matches!(self.phase, Phase::Preparation) {
            let n = config.soldiers.max(1) as usize;
            self.elapsed = vec![0; n];
            self.config = config;
        }
    }

    /// Move from Preparation to Running.
    pub fn commence(&mut self) {
        if matches!(self.phase, Phase::Preparation) {
            self.phase = Phase::Running;
        }
    }

    pub fn toggle_pause(&mut self) {
        self.phase = match self.phase {
            Phase::Running => Phase::Paused,
            Phase::Paused => Phase::Running,
            other => other,
        };
    }

    pub fn abort(&mut self) {
        if matches!(self.phase, Phase::Running | Phase::Paused | Phase::Preparation) {
            self.phase = Phase::Aborted;
        }
    }

    /// All soldiers are done. Move to Closing (debrief stays on screen).
    pub fn close(&mut self) {
        if matches!(self.phase, Phase::Running | Phase::Paused) {
            self.phase = Phase::Closing;
        }
    }

    /// Advance the wall clock by one second; counter incremented depends on phase.
    pub fn tick_one_second(&mut self) {
        match self.phase {
            Phase::Preparation => self.prep_seconds += 1,
            Phase::Running => {
                if let Some(slot) = self.elapsed.get_mut(self.current as usize) {
                    *slot += 1;
                }
            }
            Phase::Closing => self.closing_seconds += 1,
            Phase::Paused | Phase::Aborted | Phase::Finished => {}
        }
    }

    /// Move to next soldier; transition to Closing if last.
    pub fn next_soldier(&mut self) {
        if !matches!(self.phase, Phase::Running | Phase::Paused) {
            return;
        }
        if self.current + 1 >= self.config.soldiers {
            self.phase = Phase::Closing;
        } else {
            self.current += 1;
            self.phase = Phase::Running;
        }
    }

    pub fn snapshot(&self) -> Tick {
        let per = self.config.per_soldier_seconds();
        let elapsed_current = *self.elapsed.get(self.current as usize).unwrap_or(&0);
        let total_elapsed: u32 = self.elapsed.iter().sum();
        Tick {
            phase: self.phase,
            current_soldier: self.current + 1,
            current_name: self.config.name_of(self.current),
            soldiers: self.config.soldiers,
            per_soldier_seconds: per,
            elapsed_current,
            remaining_current: per as i32 - elapsed_current as i32,
            total_elapsed,
            total_budget: self.config.total_seconds,
            prep_seconds: self.prep_seconds,
            closing_seconds: self.closing_seconds,
        }
    }

    pub fn stats(&self) -> Vec<SoldierStats> {
        let per = self.config.per_soldier_seconds();
        self.elapsed
            .iter()
            .enumerate()
            .map(|(i, &e)| SoldierStats {
                index: i as u32 + 1,
                name: self.config.name_of(i as u32),
                elapsed_seconds: e,
                overtime_seconds: e.saturating_sub(per),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(n: u32, total: u32) -> SessionConfig {
        SessionConfig { soldiers: n, total_seconds: total, names: vec![] }
    }

    #[test]
    fn per_soldier_divides_total() {
        assert_eq!(cfg(5, 900).per_soldier_seconds(), 180);
        assert_eq!(cfg(0, 900).per_soldier_seconds(), 0);
    }

    #[test]
    fn names_default_to_soldier_n() {
        let mut c = cfg(3, 30);
        c.names = vec!["Alice".into(), "Bob".into()];
        assert_eq!(c.name_of(0), "Alice");
        assert_eq!(c.name_of(2), "Soldier 3");
    }

    #[test]
    fn lifecycle_runs_through_all_soldiers_then_closing() {
        let mut s = Session::new(cfg(3, 30));
        assert_eq!(s.phase(), Phase::Preparation);
        s.tick_one_second(); // counted as prep
        assert_eq!(s.prep_seconds(), 1);
        s.commence();
        assert_eq!(s.phase(), Phase::Running);
        for _ in 0..10 { s.tick_one_second(); }
        s.next_soldier();
        for _ in 0..10 { s.tick_one_second(); }
        s.next_soldier();
        for _ in 0..10 { s.tick_one_second(); }
        s.next_soldier(); // last → Closing
        assert_eq!(s.phase(), Phase::Closing);
        s.tick_one_second();
        s.tick_one_second();
        assert_eq!(s.closing_seconds(), 2);
        // running counters frozen
        assert_eq!(s.elapsed()[2], 10);
    }

    #[test]
    fn pause_blocks_tick() {
        let mut s = Session::new(cfg(2, 20));
        s.commence();
        s.tick_one_second();
        s.toggle_pause();
        s.tick_one_second();
        s.tick_one_second();
        assert_eq!(s.elapsed()[0], 1);
        s.toggle_pause();
        s.tick_one_second();
        assert_eq!(s.elapsed()[0], 2);
    }

    #[test]
    fn overtime_reported_in_stats() {
        let mut s = Session::new(cfg(2, 20)); // 10s each
        s.commence();
        for _ in 0..15 { s.tick_one_second(); }
        let stats = s.stats();
        assert_eq!(stats[0].overtime_seconds, 5);
    }

    #[test]
    fn update_config_only_in_preparation() {
        let mut s = Session::new(cfg(2, 20));
        s.update_config(cfg(4, 40));
        assert_eq!(s.config().soldiers, 4);
        s.commence();
        s.update_config(cfg(8, 80));
        assert_eq!(s.config().soldiers, 4);
    }
}
