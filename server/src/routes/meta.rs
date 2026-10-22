use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::ok;
use crate::domain::Draft;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Serialize)]
pub struct Health {
    ok: bool,
    version: &'static str,
    vision: &'static str,
    stt: &'static str,
    ledger_ok: bool,
    ledger_path: String,
    active_book: Option<i64>,
    recent_books: Vec<i64>,
}

pub async fn health(State(s): State<Arc<AppState>>) -> ApiResult<Json<Health>> {
    ok(Health {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        vision: s.vision.name(),
        stt: s.stt.name(),
        ledger_ok: s.cfg.ledger_path.exists(),
        ledger_path: s.cfg.ledger_path.display().to_string(),
        active_book: s.db.active_book(),
        recent_books: s.db.recent_books(),
    })
}

/// The phone imports this once as a trusted CA, which is what unlocks the mic.
pub async fn ca_cert(State(s): State<Arc<AppState>>) -> ApiResult<axum::response::Response> {
    let path = s.cfg.data_dir.join("certs").join("ca.crt");
    let pem = std::fs::read_to_string(&path)
        .map_err(|_| ApiError::NotFound("no CA yet; start the server once with TLS on".into()))?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/x-x509-ca-cert")],
        pem,
    )
        .into_response())
}

#[derive(Serialize)]
pub struct BookInfo {
    book: Option<i64>,
    recent_books: Vec<i64>,
}

pub async fn get_book(State(s): State<Arc<AppState>>) -> ApiResult<Json<BookInfo>> {
    ok(BookInfo { book: s.db.active_book(), recent_books: s.db.recent_books() })
}

#[derive(Deserialize)]
pub struct SetBook {
    book: i64,
}

pub async fn set_book(State(s): State<Arc<AppState>>, Json(body): Json<SetBook>) -> ApiResult<Json<BookInfo>> {
    if !(1..=999).contains(&body.book) {
        return Err(ApiError::BadRequest("book number must be 1-999".into()));
    }
    s.db.set_active_book(body.book)?;
    ok(BookInfo { book: Some(body.book), recent_books: s.db.recent_books() })
}

#[derive(Deserialize)]
pub struct ListQuery {
    limit: Option<u32>,
}

pub async fn list(State(s): State<Arc<AppState>>, Query(q): Query<ListQuery>) -> ApiResult<Json<Vec<Draft>>> {
    ok(s.db.list(q.limit.unwrap_or(25).clamp(1, 100)))
}

pub async fn get_draft(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult<Json<Draft>> {
    s.db.get_draft(&id).map(Json).ok_or_else(|| ApiError::NotFound("no such draft".into()))
}

#[derive(Serialize)]
pub struct Stats {
    today_cash: i64,
    today_online: i64,
    today_total: i64,
    today_receipts: i64,
    ledger_ok: bool,
    ledger_cash: i64,
    ledger_online: i64,
    ledger_total: i64,
    ledger_row: u32,
}

pub async fn stats(State(s): State<Arc<AppState>>) -> ApiResult<Json<Stats>> {
    let (cash, online) = s.db.today_totals();
    let today_receipts = s.db.today_count();

    let cfg = s.cfg.clone();
    let (totals, row) = tokio::task::spawn_blocking(move || {
        let t = crate::ledger::totals(&cfg.ledger_path, &cfg.ledger_sheet).unwrap_or_default();
        let r = crate::ledger::last_data_row(&cfg.ledger_path, &cfg.ledger_sheet).unwrap_or(0);
        (t, r)
    })
    .await
    .map_err(|e| ApiError::Internal(e.into()))?;

    ok(Stats {
        today_cash: cash,
        today_online: online,
        today_total: cash + online,
        today_receipts,
        ledger_ok: s.cfg.ledger_path.exists(),
        ledger_cash: totals.cash,
        ledger_online: totals.online,
        ledger_total: totals.total,
        ledger_row: row,
    })
}
