//! End-to-end API tests against a throwaway copy of the real 2026 ledger.
//! These are the tests that decide whether a receipt can safely become a row.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use puja_server::config::Config;
use puja_server::routes;
use puja_server::state::AppState;

struct Fixture {
    _tmp: tempfile::TempDir,
    state: Arc<AppState>,
    ledger: PathBuf,
    photos: PathBuf,
}

fn real_ledger() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/ledger/Durga Puja 2026.xlsx")
}

fn fixture(pin: Option<&str>) -> Fixture {
    fixture_with(pin, "fake")
}

fn fixture_with(pin: Option<&str>, provider: &str) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().to_path_buf();
    std::fs::create_dir_all(data.join("ledger")).unwrap();
    let ledger = data.join("ledger").join("book.xlsx");
    std::fs::copy(real_ledger(), &ledger).unwrap();

    let cfg = Config {
        addr: "127.0.0.1:0".into(),
        tls_addr: "127.0.0.1:0".into(),
        tls: false,
        data_dir: data.clone(),
        ledger_path: ledger.clone(),
        ledger_sheet: "CASH ".into(),
        app_dir: PathBuf::new(),
        pin: pin.map(str::to_string),
        vision_provider: provider.into(),
        vision_base_url: String::new(),
        vision_api_key: None,
        vision_model: "fake".into(),
        stt_base_url: String::new(),
        stt_api_key: None,
        stt_model: "fake".into(),
        stt_language: None,
    };
    let state = AppState::new(cfg).unwrap();
    Fixture { photos: data.join("photos"), _tmp: tmp, state, ledger }
}

/// Smallest thing that passes the jpeg magic check.
fn fake_jpeg() -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0];
    v.resize(200, 0);
    v
}

fn multipart(fields: &[(&str, Option<&str>, &[u8], &str)], boundary: &str) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, filename, bytes, ctype) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(f) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\n").as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(format!("Content-Type: {ctype}\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

async fn send(state: &Arc<AppState>, req: Request<Body>) -> (StatusCode, Value) {
    let res = routes::router(state.clone()).oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn capture_request(boundary: &str, book: Option<i64>, transcript: &str, client_id: &str) -> Request<Body> {
    let jpeg = fake_jpeg();
    let book_text = book.map(|b| b.to_string()).unwrap_or_default();
    let mut fields: Vec<(&str, Option<&str>, &[u8], &str)> = vec![
        ("image", Some("stub.jpg"), &jpeg, "image/jpeg"),
        ("transcript", None, transcript.as_bytes(), "text/plain"),
        ("client_id", None, client_id.as_bytes(), "text/plain"),
    ];
    if book.is_some() {
        fields.push(("book", None, book_text.as_bytes(), "text/plain"));
    }
    Request::builder()
        .method("POST")
        .uri("/api/capture")
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(multipart(&fields, boundary)))
        .unwrap()
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn happy_path_writes_one_correct_excel_row() {
    let f = fixture(None);
    let (_, info) = send(&f.state, Request::builder().uri("/api/book").body(Body::empty()).unwrap()).await;
    assert_eq!(info["book"], Value::Null);

    let (status, book) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(book["book"], 18);

    let before = puja_server::ledger::last_data_row(&f.ledger, "CASH ").unwrap();
    let (status, draft) = send(
        &f.state,
        capture_request("B1", None, "name suddha dairy receipt number nine five zero one eleven hundred cash", "c-1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(draft["receipt"]["value"], "9501");
    assert_eq!(draft["name"]["value"], "SUDHA DAIRY");
    assert_eq!(draft["amount"]["value"], "1100");
    assert_eq!(draft["mode"]["value"], "CASH");
    assert_eq!(draft["status"], "pending");

    // the photo landed in the book folder, not somewhere random
    let url = draft["photo_url"].as_str().unwrap();
    assert!(url.starts_with("/media/photos/18/"), "{url}");
    let file = f.photos.join("18").join(url.rsplit('/').next().unwrap());
    assert!(file.exists(), "photo not saved at {file:?}");

    let id = draft["id"].as_str().unwrap();
    let (status, done) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["row"], before + 1);
    assert_eq!(done["draft"]["status"], "committed");
    assert_eq!(done["totals"]["total"].as_i64().unwrap(), done["totals"]["cash"].as_i64().unwrap() + done["totals"]["online"].as_i64().unwrap());

    // pushed twice must never write twice
    let (status, again) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["row"], done["row"]);
    assert_eq!(puja_server::ledger::last_data_row(&f.ledger, "CASH ").unwrap(), before + 1);
}

#[tokio::test]
async fn retry_with_same_client_id_returns_the_same_draft() {
    let f = fixture(None);
    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    let (_, a) = send(&f.state, capture_request("B2", Some(18), "receipt 852 1100 cash", "same-id")).await;
    let (_, b) = send(&f.state, capture_request("B3", Some(18), "receipt 852 1100 cash", "same-id")).await;
    assert_eq!(a["id"], b["id"]);
}

#[tokio::test]
async fn capture_needs_a_book_a_photo_and_an_amount() {
    let f = fixture(None);
    // no book chosen anywhere yet
    let (status, body) = send(&f.state, capture_request("B4", None, "something", "x-1")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().unwrap().contains("book"), "{body}");

    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    let boundary = "B5";
    let fields: Vec<(&str, Option<&str>, &[u8], &str)> =
        vec![("transcript", None, b"ram kumar 900 500 cash", "text/plain")];
    let req = Request::builder()
        .method("POST")
        .uri("/api/capture")
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(multipart(&fields, boundary)))
        .unwrap();
    let (status, body) = send(&f.state, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("image"));
}

#[tokio::test]
async fn a_draft_without_a_name_cannot_be_pushed() {
    // no vision provider: nothing can invent a name
    let f = fixture_with(None, "none");
    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    let (_, draft) = send(&f.state, capture_request("B6", Some(18), "receipt 853 700 cash", "x-2")).await;
    let id = draft["id"].as_str().unwrap();
    let (status, body) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("name"), "{body}");
}

#[tokio::test]
async fn reviewer_corrections_win_and_online_goes_in_column_f() {
    let f = fixture(None);
    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    let (_, draft) = send(&f.state, capture_request("B7", Some(18), "receipt 9502 400 cash", "x-3")).await;
    let id = draft["id"].as_str().unwrap();

    let patch = Request::builder()
        .method("PATCH")
        .uri(format!("/api/draft/{id}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"name": "U.N. Singh", "amount": "400", "mode": "online"}).to_string()))
        .unwrap();
    let (status, edited) = send(&f.state, patch).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(edited["name"]["source"], "manual");
    assert_eq!(edited["mode"]["value"], "ONLINE");

    let (status, done) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");

    let row = done["row"].as_u64().unwrap() as u32;
    let book = umya_spreadsheet::reader::xlsx::read(&f.ledger).unwrap();
    let ws = book.sheet_by_name("CASH ").unwrap();
    let cell = |c: u32| ws.cell((c, row)).map(|x| x.value().to_string()).unwrap_or_default();
    assert_eq!(cell(1), "18");
    assert_eq!(cell(2), "9502");
    assert_eq!(cell(3), "U.N. Singh", "a human edit is never re-spelled");
    assert_eq!(cell(4), "", "online rupees must not go in the cash column");
    assert_eq!(cell(5), "ONLINE");
    assert_eq!(cell(6), "400");
}

/// Attach the PIN where a browser would: header for API calls, query for media.
fn with_pin(mut req: Request<Body>, pin: &str) -> Request<Body> {
    req.headers_mut().insert("x-pin", pin.parse().unwrap());
    req
}

#[tokio::test]
async fn a_pin_closes_the_donor_data_and_the_photos_too() {
    let f = fixture(Some("4821"));
    let (status, draft) = send(
        &f.state,
        with_pin(
            capture_request("M1", Some(18), "receipt 9511 100 cash", "m-1"),
            "4821",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    let photo = draft["photo_url"].as_str().unwrap().to_string();

    let (status, _) = send(&f.state, Request::builder().uri("/api/drafts").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "donor list must not be public");
    let (status, _) = send(&f.state, Request::builder().uri(&photo).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "photos must not be public");

    let (status, _) = send(
        &f.state,
        Request::builder().uri(format!("{photo}?pin=4821")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "?pin= is how an <img> authenticates");
    let (status, _) = send(
        &f.state,
        Request::builder().uri("/api/drafts").header("x-pin", "4821").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_pin_is_required_for_writes() {
    let f = fixture(Some("4821"));
    let (status, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let mut ok = post_json("/api/book", json!({"book": 18}));
    ok.headers_mut().insert("x-pin", "4821".parse().unwrap());
    let (status, _) = send(&f.state, ok).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&f.state, Request::builder().uri("/api/health").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "health stays open for the phone to probe");
}

#[tokio::test]
async fn stats_reflect_the_sheet_not_just_the_app() {
    let f = fixture(None);
    let (_, body) = send(&f.state, Request::builder().uri("/api/stats").body(Body::empty()).unwrap()).await;
    let sheet_cash = puja_server::ledger::totals(&f.ledger, "CASH ").unwrap().cash;
    assert_eq!(body["ledger_cash"].as_i64().unwrap(), sheet_cash);
    assert_eq!(body["ledger_total"].as_i64().unwrap(), body["ledger_cash"].as_i64().unwrap() + body["ledger_online"].as_i64().unwrap());
}

#[tokio::test]
async fn the_same_receipt_cannot_enter_the_book_twice() {
    let f = fixture(None);
    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    // book 18 / receipt 851 already exists in the real ledger (row 259)
    let (status, draft) = send(&f.state, capture_request("B8", Some(18), "receipt 851 1100 cash", "dup-1")).await;
    assert_eq!(status, StatusCode::OK);
    let id = draft["id"].as_str().unwrap();
    let rows_before = puja_server::ledger::last_data_row(&f.ledger, "CASH ").unwrap();
    let (status, body) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].as_str().unwrap().contains("already row"), "{body}");
    assert_eq!(puja_server::ledger::last_data_row(&f.ledger, "CASH ").unwrap(), rows_before);
}

#[tokio::test]
async fn a_preprinted_stub_is_filled_instead_of_duplicated() {
    let f = fixture(None);
    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    // rows 274-307 are pre-printed book 18 / receipts 866-900 with nothing filled in
    let (status, draft) = send(
        &f.state,
        capture_request("P1", None, "name pre printed receipt number eight nine zero fifty one cash", "p-1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(draft["receipt"]["value"], "890");
    assert_eq!(draft["amount"]["value"], "51");

    let id = draft["id"].as_str().unwrap();
    let (status, done) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["row"], 297, "must write the pre-printed row, not append");

    let book = umya_spreadsheet::reader::xlsx::read(&f.ledger).unwrap();
    let ws = book.sheet_by_name("CASH ").unwrap();
    let at = |c: u32| ws.cell((c, 297)).map(|x| x.value().to_string()).unwrap_or_default();
    assert_eq!(at(3), draft["name"]["value"].as_str().unwrap());
    assert_eq!(at(4), "51");
    assert_eq!(at(5), "CASH");
    assert_eq!(at(6), "", "online column must stay empty");
}

#[tokio::test]
async fn four_simultaneous_pushes_write_one_row() {
    let f = fixture(None);
    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    let (_, draft) = send(&f.state, capture_request("S1", None, "receipt 9507 70 cash", "s-1")).await;
    let id = draft["id"].as_str().unwrap().to_string();
    let before = puja_server::ledger::last_data_row(&f.ledger, "CASH ").unwrap();

    let mut handles = Vec::new();
    for _ in 0..4 {
        let state = f.state.clone();
        let id = id.clone();
        handles.push(tokio::spawn(async move {
            send(
                &state,
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/draft/{id}/confirm"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
        }));
    }
    let mut rows = Vec::new();
    for h in handles {
        let (status, body) = h.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        rows.push(body["row"].as_u64().unwrap());
    }
    assert!(rows.iter().all(|r| *r == rows[0]), "different rows: {rows:?}");
    assert_eq!(puja_server::ledger::last_data_row(&f.ledger, "CASH ").unwrap(), before + 1);
}

#[tokio::test]
async fn a_name_cannot_corrupt_the_workbook() {
    let f = fixture(None);
    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    let (_, draft) = send(&f.state, capture_request("K1", None, "receipt 9508 100 cash", "k-1")).await;
    let id = draft["id"].as_str().unwrap();
    let dirty = format!("A\u{1}B {}", "x".repeat(5000));
    let (status, patched) = send(
        &f.state,
        Request::builder()
            .method("PATCH")
            .uri(format!("/api/draft/{id}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "name": dirty }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    let name = patched["name"]["value"].as_str().unwrap();
    assert_eq!(name.chars().count(), 200);
    assert!(!name.chars().any(char::is_control), "{name:?}");

    let (status, done) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let book = umya_spreadsheet::reader::xlsx::read(&f.ledger).expect("workbook must stay readable");
    let ws = book.sheet_by_name("CASH ").unwrap();
    let row = done["row"].as_u64().unwrap() as u32;
    assert_eq!(ws.cell((3, row)).unwrap().value(), name);
}

#[tokio::test]
async fn the_monitor_shows_the_pipeline_and_the_row_that_was_pushed() {
    let f = fixture(None);
    let (_, _) = send(&f.state, post_json("/api/book", json!({"book": 18}))).await;
    let (_, draft) = send(
        &f.state,
        capture_request("MO1", None, "name suddha dairy receipt number nine five zero one eleven hundred cash", "mo-1"),
    )
    .await;
    let id = draft["id"].as_str().unwrap().to_string();

    let (status, snap) = send(&f.state, Request::builder().uri("/api/monitor").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{snap}");
    assert_eq!(snap["metrics"]["captures"], 1);
    assert_eq!(snap["metrics"]["vision_ok"], 1);
    assert_eq!(snap["metrics"]["stt_skip"], 1, "the phone sent a transcript");
    assert_eq!(snap["models"]["vision"]["provider"], "fake");
    assert_eq!(snap["models"]["vision"]["ready"], true);
    assert_eq!(snap["models"]["stt"]["provider"], "fake");
    assert_eq!(snap["ledger"]["ok"], true);
    assert_eq!(snap["ledger"]["rows"], 310);
    assert!(snap["running"].as_array().unwrap().is_empty());
    let kinds: Vec<&str> = snap["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    for kind in ["capture", "vision", "stt", "merge", "draft"] {
        assert!(kinds.contains(&kind), "no {kind} event in {kinds:?}");
    }
    let cursor = snap["cursor"].as_u64().unwrap();
    assert!(cursor > 0);
    let (_, idle) = send(
        &f.state,
        Request::builder().uri(format!("/api/monitor?since={cursor}")).body(Body::empty()).unwrap(),
    )
    .await;
    assert!(idle["events"].as_array().unwrap().is_empty(), "the cursor must not repeat events");

    let (status, done) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let row = done["row"].as_u64().unwrap();

    let (_, after) = send(
        &f.state,
        Request::builder().uri(format!("/api/monitor?since={cursor}")).body(Body::empty()).unwrap(),
    )
    .await;
    let pushed = after["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "ledger" && e["data"]["row"] == row)
        .expect("push event");
    assert_eq!(pushed["level"], "ok");
    assert_eq!(pushed["data"]["name"], "SUDHA DAIRY");
    assert_eq!(pushed["data"]["mode"], "CASH");
    assert_eq!(pushed["data"]["amount"], 1100);
    assert_eq!(pushed["data"]["photo_url"], draft["photo_url"]);
    assert_eq!(after["metrics"]["pushes"], 1);
    assert_eq!(after["ledger"]["rows"], row);
    assert_eq!(after["committed"][0]["row"], row);
    assert_eq!(after["committed"][0]["name"], "SUDHA DAIRY");

    let before = puja_server::ledger::last_data_row(&f.ledger, "CASH ").unwrap();
    let (_, again) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(again["row"], row);
    let (_, dup) = send(
        &f.state,
        capture_request("MO2", Some(18), "receipt 9501 1100 cash", "mo-2"),
    )
    .await;
    let dup_id = dup["id"].as_str().unwrap();
    let (status, _) = send(
        &f.state,
        Request::builder().method("POST").uri(format!("/api/draft/{dup_id}/confirm")).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(puja_server::ledger::last_data_row(&f.ledger, "CASH ").unwrap(), before);
    let (_, last) = send(&f.state, Request::builder().uri("/api/monitor").body(Body::empty()).unwrap()).await;
    assert!(last["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["data"]["refused"] == true && e["level"] == "warn"));
}

#[tokio::test]
async fn the_monitor_is_pin_guarded_but_its_page_is_not() {
    let f = fixture(Some("4821"));
    let (status, _) = send(&f.state, Request::builder().uri("/api/monitor").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &f.state,
        with_pin(Request::builder().uri("/api/monitor").body(Body::empty()).unwrap(), "4821"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The page carries no data, so it loads before the PIN is typed.
    let res = routes::router(f.state.clone())
        .oneshot(Request::builder().uri("/monitor").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("Server monitor"));
}

/// Guard: the test fixture must never be the live ledger.
#[test]
fn tests_use_a_copy_of_the_ledger() {
    let f = fixture(None);
    assert_ne!(f.ledger.canonicalize().unwrap(), real_ledger().canonicalize().unwrap());
}
