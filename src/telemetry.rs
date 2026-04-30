// SPDX-License-Identifier: EUPL-1.2
//! Observability layer — OpenTelemetry-aligned, dependency-light.
//!
//! * **Logs**  : structured JSON via `tracing` + `tracing-subscriber`,
//!               written to stderr so stdout stays usable for `--mode json`.
//! * **Traces**: `tracing` spans (`session.preparation`, `session.running`, …)
//!               carrying OTel semantic attributes.
//! * **Metrics**: Prometheus exposition (OTel-compatible) on `/metrics`,
//!                served by a tiny synchronous HTTP listener.
//!
//! Resource attributes (`service.name`, `service.version`) follow OTel
//! semconv. Metric names use the OTel `daily_commando_*` namespace.

use anyhow::Result;
use once_cell::sync::Lazy;
use prometheus::{
    Encoder, Gauge, GaugeVec, IntCounter, IntCounterVec, Opts, Registry, TextEncoder,
};
use std::sync::Arc;
use std::thread;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub const SERVICE_NAME: &str = "daily-commando";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub static REGISTRY: Lazy<Registry> = Lazy::new(|| {
    Registry::new_custom(
        Some("daily_commando".to_string()),
        Some(prometheus::labels! {
            "service_name".to_string() => SERVICE_NAME.to_string(),
            "service_version".to_string() => SERVICE_VERSION.to_string(),
        }),
    )
    .expect("registry")
});

pub static SESSIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::with_opts(Opts::new("sessions_total", "Daily sessions started")).unwrap();
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static PHASE_SECONDS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let m = IntCounterVec::new(
        Opts::new("phase_seconds_total", "Cumulative seconds spent in each phase"),
        &["phase"],
    )
    .unwrap();
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static SOLDIER_SECONDS: Lazy<GaugeVec> = Lazy::new(|| {
    let m = GaugeVec::new(
        Opts::new("soldier_seconds", "Seconds spent per soldier in current session"),
        &["index", "name"],
    )
    .unwrap();
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static OVERTIME_SECONDS: Lazy<Gauge> = Lazy::new(|| {
    let m = Gauge::with_opts(Opts::new(
        "overtime_seconds",
        "Total overtime in current session",
    ))
    .unwrap();
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static CURRENT_SOLDIER: Lazy<Gauge> = Lazy::new(|| {
    let m = Gauge::with_opts(Opts::new(
        "current_soldier_index",
        "1-based index of the soldier currently speaking",
    ))
    .unwrap();
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

/// Initialise tracing and start the Prometheus HTTP exporter on `port`.
///
/// `json_logs = true` emits one JSON object per log record on **stderr**, so
/// stdout-based modes (e.g. `--mode json`) remain machine-parseable.
pub fn init(port: u16, json_logs: bool) -> Result<()> {
    // Force lazy registration of all metrics so /metrics never returns empty.
    Lazy::force(&SESSIONS_TOTAL);
    Lazy::force(&PHASE_SECONDS_TOTAL);
    Lazy::force(&SOLDIER_SECONDS);
    Lazy::force(&OVERTIME_SECONDS);
    Lazy::force(&CURRENT_SOLDIER);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = tracing_subscriber::registry().with(filter);
    if json_logs {
        registry
            .with(
                fmt::layer()
                    .json()
                    .with_writer(std::io::stderr)
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .try_init()
            .ok();
    } else {
        registry
            .with(fmt::layer().with_writer(std::io::stderr).compact())
            .try_init()
            .ok();
    }

    spawn_metrics_server(port)?;
    tracing::info!(
        service.name = SERVICE_NAME,
        service.version = SERVICE_VERSION,
        metrics.port = port,
        "telemetry initialised"
    );
    Ok(())
}

fn spawn_metrics_server(port: u16) -> Result<()> {
    let server = tiny_http::Server::http(("0.0.0.0", port))
        .map_err(|e| anyhow::anyhow!("metrics server bind failed: {e}"))?;
    let server = Arc::new(server);
    let s = server.clone();
    thread::Builder::new()
        .name("metrics-http".into())
        .spawn(move || {
            for req in s.incoming_requests() {
                let url = req.url().to_string();
                let response = if url.starts_with("/metrics") {
                    let mut buf = Vec::new();
                    let encoder = TextEncoder::new();
                    let mf = REGISTRY.gather();
                    if let Err(e) = encoder.encode(&mf, &mut buf) {
                        tracing::warn!(error = %e, "metrics encode failed");
                    }
                    tiny_http::Response::from_data(buf).with_header(
                        "Content-Type: text/plain; version=0.0.4; charset=utf-8"
                            .parse::<tiny_http::Header>()
                            .unwrap(),
                    )
                } else if url == "/health" || url == "/healthz" {
                    tiny_http::Response::from_string("ok")
                } else {
                    tiny_http::Response::from_string("daily-commando metrics on /metrics\n")
                };
                let _ = req.respond(response);
            }
        })?;
    Ok(())
}

/// Snapshot the current session state into the metric vectors.
pub fn record(snap: &crate::Tick, stats: &[crate::SoldierStats]) {
    CURRENT_SOLDIER.set(snap.current_soldier as f64);
    let overtime: u32 = stats.iter().map(|s| s.overtime_seconds).sum();
    OVERTIME_SECONDS.set(overtime as f64);
    for s in stats {
        SOLDIER_SECONDS
            .with_label_values(&[&s.index.to_string(), &s.name])
            .set(s.elapsed_seconds as f64);
    }
}

/// Increment the per-phase second counter (called once per real second).
pub fn count_phase_second(phase: crate::Phase) {
    let label = match phase {
        crate::Phase::Preparation => "preparation",
        crate::Phase::Running => "running",
        crate::Phase::Paused => "paused",
        crate::Phase::Closing => "closing",
        crate::Phase::Aborted => "aborted",
        crate::Phase::Finished => "finished",
    };
    PHASE_SECONDS_TOTAL.with_label_values(&[label]).inc();
}
