// SPDX-License-Identifier: EUPL-1.2
//! Daily Commando — headless core + observability primitives.

pub mod session;
pub mod telemetry;

pub use session::{Phase, Session, SessionConfig, SoldierStats, Tick};
