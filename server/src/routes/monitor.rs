//! The laptop console: a read-only window on the pipeline.
//!
//! One polling endpoint instead of a websocket: the page asks "what happened
//! since seq N" every second, so a dropped connection never needs a resume
//! protocol - it just asks again.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Html;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::domain::Status;
use crate::error::ApiResult;
use crate::monitor::{Event, Metrics, Running};
use crate::state::AppState;

/// Static page, deliberately unguarded: it contains no donor data. The data
/// endpoint below is behind the same PIN as the rest of the API.
pub async fn page() -> Html<&'static str> {
    Html(include_str!("../../static/monitor.html"))
}

#[derive(Deserialize)]
pub struct Since {
    since: Option<u64>,
}

#[derive(Serialize)]
struct Model {
    provider: &'static str,
    model: String,
    base_url: String,
    key: bool,
    ready: bool,
}

#[derive(Serialize)]
struct Models {
    vision: Model,
    stt: Model,
}

#[derive(Serialize)]
struct ServerInfo {
    version: &'static str,
    pid: u32,
    uptime_s: u64,
    http: String,
    https: Option<String>,
    pin: bool,
    data_dir: String,
    app_dir: String,
}

#[derive(Serialize)]
struct LedgerInfo {
    path: String,
    sheet: String,
    ok: bool,
    rows: u32,
    cash: i64,
    online: i64,
    total: i64,
    at: Option<String>,
}

#[derive(Serialize)]
struct Committed {
    row: u32,
    book: i64,
    receipt: String,
    name: String,
    mode: String,
    amount: i64,
    at: String,
    photo_url: String,
}

#[derive(Serialize)]
pub struct Snapshot {
    now: String,
    server: ServerInfo,
    ledger: LedgerInfo,
    models: Models,
    metrics: Metrics,
    running: Vec<Running>,
    events: Vec<Event>,
    cursor: u64,
    committed: Vec<Committed>,
}

pub async fn snapshot(
    State(s): State<Arc<AppState>>,
    Query(q): Query<Since>,
) -> ApiResult<Json<Snapshot>> {
    let (cursor, events, running, metrics) = s.monitor.snapshot(q.since.unwrap_or(0));
    let ledger = s.monitor.ledger();

    let committed = s
        .db
        .list(100)
        .into_iter()
        .filter(|d| d.status == Status::Committed)
        .take(10)
        .map(|d| Committed {
            row: d.committed_row.unwrap_or(0),
            book: d.book,
            receipt: d.receipt.text().unwrap_or_default().to_string(),
            name: d.name.text().unwrap_or_default().to_string(),
            mode: d.mode.text().unwrap_or_default().to_string(),
            amount: d.amount.number().unwrap_or_default(),
            at: d.created_at,
            photo_url: d.photo_url,
        })
        .collect();

    let vision_ready = s.vision.name() != "none";
    let stt_ready = s.stt.name() != "none";
    let key = s.cfg.vision_api_key.is_some() || s.cfg.stt_api_key.is_some();
    let model = |ready: bool, id: &str| {
        if ready {
            id.to_string()
        } else {
            "—".to_string()
        }
    };

    Ok(Json(Snapshot {
        now: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
        server: ServerInfo {
            version: env!("CARGO_PKG_VERSION"),
            pid: std::process::id(),
            uptime_s: s.monitor.uptime_s(),
            http: s.cfg.addr.clone(),
            https: s.cfg.tls.then(|| s.cfg.tls_addr.clone()),
            pin: s.cfg.pin.is_some(),
            data_dir: s.cfg.data_dir.display().to_string(),
            app_dir: s.cfg.app_dir.display().to_string(),
        },
        ledger: LedgerInfo {
            path: s.cfg.ledger_path.display().to_string(),
            sheet: s.cfg.ledger_sheet.clone(),
            ok: s.cfg.ledger_path.exists(),
            rows: ledger.as_ref().map_or(0, |l| l.rows),
            cash: ledger.as_ref().map_or(0, |l| l.cash),
            online: ledger.as_ref().map_or(0, |l| l.online),
            total: ledger.as_ref().map_or(0, |l| l.total),
            at: ledger.map(|l| l.at),
        },
        models: Models {
            vision: Model {
                provider: s.vision.name(),
                model: model(vision_ready, &s.cfg.vision_model),
                base_url: s.cfg.vision_base_url.clone(),
                key,
                ready: vision_ready,
            },
            stt: Model {
                provider: s.stt.name(),
                model: model(stt_ready, &s.cfg.stt_model),
                base_url: s.cfg.stt_base_url.clone(),
                key,
                ready: stt_ready,
            },
        },
        metrics,
        running,
        events,
        cursor,
        committed,
    }))
}
