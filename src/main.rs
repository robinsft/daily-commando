// SPDX-License-Identifier: EUPL-1.2
//! Daily Commando CLI entrypoint.

use anyhow::Result;
use clap::{Parser, ValueEnum};
use daily_commando::{telemetry, Session, SessionConfig};

mod render_json;
mod render_tui;

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Mode { Tui, Json }

#[derive(Parser, Debug)]
#[command(name = "daily-commando", version, about = "Minimalist daily standup timer, commando style")]
struct Cli {
    /// Number of soldiers (1..=20)
    #[arg(short = 'n', long, default_value_t = 5)]
    soldiers: u32,

    /// Total daily duration, in minutes
    #[arg(short = 't', long, default_value_t = 15)]
    minutes: u32,

    /// Comma-separated soldier names (skips welcome name prompts when set)
    #[arg(long)]
    names: Option<String>,

    /// Skip the interactive welcome screen and start immediately (TUI only)
    #[arg(long, default_value_t = false)]
    skip_welcome: bool,

    /// Output mode
    #[arg(short = 'm', long, value_enum, default_value_t = Mode::Tui)]
    mode: Mode,

    /// Auto-advance to next soldier when their time is up
    #[arg(long, default_value_t = true)]
    auto_advance: bool,

    /// Port for the Prometheus /metrics HTTP endpoint
    #[arg(long, default_value_t = 9464)]
    metrics_port: u16,

    /// Disable the metrics HTTP server entirely
    #[arg(long, default_value_t = false)]
    no_metrics: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if !cli.no_metrics {
        if let Err(e) = telemetry::init(cli.metrics_port, matches!(cli.mode, Mode::Json)) {
            eprintln!("warning: telemetry init failed: {e}");
        }
    }
    telemetry::SESSIONS_TOTAL.inc();

    let parsed_names: Vec<String> = cli
        .names
        .as_deref()
        .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();

    let cfg = SessionConfig {
        soldiers: cli.soldiers.clamp(1, 20),
        total_seconds: cli.minutes.max(1) * 60,
        names: parsed_names,
    };
    let session = Session::new(cfg);

    match cli.mode {
        Mode::Json => render_json::run(session),
        Mode::Tui => render_tui::run(session, cli.auto_advance, cli.skip_welcome),
    }
}
