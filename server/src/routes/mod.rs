use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;

use tower_http::services::ServeDir;

use crate::error::ApiResult;
use crate::state::AppState;

pub mod capture;
pub mod meta;
pub mod monitor;

/// 20 MB: a phone photo is 2-6 MB, a 6 s voice clip is ~100 KB.
const BODY_LIMIT: usize = 20 * 1024 * 1024;

pub fn router(state: Arc<AppState>) -> Router {
    // Everything that shows donor names, amounts, photos or voice is behind the
    // PIN, not just the writes. `/api/health` and the CA stay open so a phone can
    // join before anyone types the PIN.
    let guarded = Router::new()
        .route("/api/book", get(meta::get_book).post(meta::set_book))
        .route("/api/stats", get(meta::stats))
        .route("/api/drafts", get(meta::list))
        .route("/api/draft/{id}", get(meta::get_draft).patch(capture::edit))
        .route("/api/draft/{id}/confirm", post(capture::confirm))
        .route("/api/draft/{id}/reject", post(capture::reject))
        .route("/api/capture", post(capture::capture))
        .route("/api/monitor", get(monitor::snapshot))
        .nest_service("/media/photos", ServeDir::new(state.cfg.data_dir.join("photos")))
        .nest_service("/media/audio", ServeDir::new(state.cfg.data_dir.join("audio")))
        .layer(middleware::from_fn_with_state(state.clone(), require_pin));

    // The console page holds no data; everything it shows comes from the
    // guarded endpoint above, so it can load before the PIN is typed.
    let open = Router::new()
        .route("/api/health", get(meta::health))
        .route("/ca.crt", get(meta::ca_cert))
        .route("/monitor", get(monitor::page))
        .route("/monitor/", get(monitor::page));

    open.merge(guarded)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

/// If PUJA_PIN is set, every API call and every photo/voice file must carry it:
/// in the `x-pin` header, or as `?pin=` for `<img>`/`<audio>` URLs, which cannot
/// send headers.
async fn require_pin(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let Some(expected) = state.cfg.pin.as_deref() else {
        return next.run(req).await;
    };
    let header = req
        .headers()
        .get("x-pin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let query = req
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("pin=")))
        .map(pct_decode)
        .unwrap_or_default();
    let given = if header.is_empty() { query.as_str() } else { header.as_str() };
    if constant_eq(given, expected) {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"error":"missing or wrong PIN"})))
            .into_response()
    }
}

/// Minimal percent-decoder for the PIN query parameter (`%40`, `+`).
fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn ok<T: serde::Serialize>(v: T) -> ApiResult<axum::Json<T>> {
    Ok(axum::Json(v))
}
