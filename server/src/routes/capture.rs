use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Multipart, Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::domain::merge::{self, Merged};
use crate::domain::{Draft, Field, Mode, Source, Status};
use crate::error::{ApiError, ApiResult};
use crate::monitor::{job_id, Level};
use crate::providers::ProviderError;
use crate::state::AppState;

/// POST /api/capture - one receipt: photo + optional 6 s voice clip.
///
/// Everything is stored before anything is interpreted, so a model outage never
/// loses the photo.
pub async fn capture(State(s): State<Arc<AppState>>, mut form: Multipart) -> ApiResult<Json<Draft>> {
    let mut image: Option<(Vec<u8>, String)> = None;
    let mut audio: Option<(Vec<u8>, String)> = None;
    let mut transcript = String::new();
    let mut client_id: Option<String> = None;
    let mut book: Option<i64> = None;

    while let Some(field) = form.next_field().await.map_err(|e| ApiError::BadRequest(e.to_string()))? {
        let name = field.name().unwrap_or_default().to_string();
        let mime = field.content_type().unwrap_or("application/octet-stream").to_string();
        match name.as_str() {
            "image" => image = Some((field.bytes().await.map_err(|e| ApiError::BadRequest(e.to_string()))?.to_vec(), mime)),
            "audio" => audio = Some((field.bytes().await.map_err(|e| ApiError::BadRequest(e.to_string()))?.to_vec(), mime)),
            "transcript" => transcript = field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?,
            "client_id" => client_id = Some(field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?),
            "book" => book = field.text().await.ok().and_then(|t| t.trim().parse().ok()),
            _ => {}
        }
    }

    // Retry-safe: the same client_id always returns the same draft. An empty id
    // is no id - storing "" would make the second empty-id capture fail UNIQUE.
    let client_id = client_id.filter(|c| !c.trim().is_empty());
    if let Some(cid) = client_id.as_deref() {
        if let Some(existing) = s.db.find_by_client_id(cid) {
            return Ok(Json(existing));
        }
    }

    let (img, img_mime) = image.ok_or_else(|| ApiError::BadRequest("image field is required".into()))?;
    if !crate::storage::looks_like_image(&img) {
        return Err(ApiError::BadRequest("that does not look like a photo (jpeg/png/webp/heic)".into()));
    }
    let book = book
        .or_else(|| s.db.active_book())
        .ok_or_else(|| ApiError::BadRequest("set the book number first".into()))?;
    if !(1..=999).contains(&book) {
        return Err(ApiError::BadRequest("book number must be 1-999".into()));
    }

    let photo = crate::storage::save_photo(&s.cfg.data_dir, book, &img, &img_mime)?;
    let audio_stored = match &audio {
        Some((bytes, mime)) => Some(crate::storage::save_audio(&s.cfg.data_dir, book, bytes, mime)?),
        None => None,
    };
    s.monitor.count(|m| {
        m.captures += 1;
        m.photo_bytes += img.len() as u64;
        m.audio_bytes += audio.as_ref().map_or(0, |(b, _)| b.len() as u64);
    });
    s.monitor.record(
        Level::Info,
        "capture",
        format!(
            "Capture · book {book} · photo {}{}",
            crate::monitor::size(img.len()),
            if audio.is_some() { " · voice clip" } else { " · no voice" }
        ),
        Some(json!({
            "book": book,
            "photo": photo.url,
            "bytes": img.len(),
            "audio": audio_stored.as_ref().map(|a| &a.url),
        })),
    );

    // Big vision model, if one is configured. Its failure is evidence, not a stop.
    let mut vision_out = None;
    let mut provider_note = None;
    let vision_job = job_id();
    s.monitor.start(
        &vision_job,
        format!("Vision · {}/{} · reading the photo", s.vision.name(), s.cfg.vision_model),
    );
    let vision_started = Instant::now();
    // A hung model must not hold the volunteer hostage.
    match tokio::time::timeout(Duration::from_secs(45), s.vision.analyze(&img, &img_mime)).await {
        Ok(Ok(v)) => {
            let ms = vision_started.elapsed().as_millis() as u64;
            s.monitor.count(|m| {
                m.vision_ok += 1;
                m.vision_ms += ms;
            });
            s.monitor.finish(
                &vision_job,
                Level::Ok,
                "vision",
                format!(
                    "Vision read · {} · R/{} · {} · {}",
                    v.name.as_deref().unwrap_or("?"),
                    v.receipt.as_deref().unwrap_or("?"),
                    v.amount.as_deref().map(|a| format!("₹{a}")).unwrap_or_else(|| "amount?".into()),
                    v.mode.map_or("mode?", |m| m.as_str())
                ),
                Some(json!({
                    "ms": ms,
                    "receipt": &v.receipt,
                    "name": &v.name,
                    "amount": &v.amount,
                    "mode": v.mode.map(|m| m.as_str()),
                    "confidence": v.confidence,
                    "note": &v.note,
                })),
            );
            vision_out = Some(v);
        }
        Ok(Err(ProviderError::NotConfigured)) => {
            s.monitor.count(|m| m.vision_skip += 1);
            s.monitor.finish(
                &vision_job,
                Level::Info,
                "vision",
                "Vision skipped · no model configured (voice + typing carry the capture)",
                None,
            );
        }
        Ok(Err(e)) => {
            let ms = vision_started.elapsed().as_millis() as u64;
            tracing::warn!("vision provider failed: {e}");
            s.monitor.count(|m| {
                m.vision_fail += 1;
                m.vision_ms += ms;
                m.errors += 1;
            });
            s.monitor.finish(
                &vision_job,
                Level::Warn,
                "vision",
                format!("Vision failed after {ms} ms · {e}"),
                None,
            );
            provider_note = Some(format!("vision model failed: {e}"));
        }
        Err(_) => {
            let ms = vision_started.elapsed().as_millis() as u64;
            tracing::warn!("vision provider timed out");
            s.monitor.count(|m| {
                m.vision_fail += 1;
                m.vision_ms += ms;
                m.errors += 1;
            });
            s.monitor.finish(
                &vision_job,
                Level::Warn,
                "vision",
                format!("Vision timed out after {ms} ms"),
                None,
            );
            provider_note = Some("vision model took too long, skipped".into());
        }
    }

    // Some phones cannot do on-device dictation; then the server transcribes.
    let mut spoken_text = transcript.trim().to_string();
    if !spoken_text.is_empty() {
        s.monitor.count(|m| m.stt_skip += 1);
        s.monitor.record(
            Level::Info,
            "stt",
            format!("Voice from the phone's dictation · {} chars", spoken_text.chars().count()),
            None,
        );
    } else if let Some((bytes, _)) = &audio {
        let stt_job = job_id();
        s.monitor.start(
            &stt_job,
            format!(
                "Speech · {} · transcribing {}",
                s.cfg.stt_model,
                crate::monitor::size(bytes.len())
            ),
        );
        let stt_started = Instant::now();
        match tokio::time::timeout(Duration::from_secs(20), s.stt.transcribe(bytes, "clip.webm")).await {
            Ok(Ok(t)) => {
                let ms = stt_started.elapsed().as_millis() as u64;
                let spoken = t.trim().to_string();
                let short: String = spoken.chars().take(120).collect();
                s.monitor.count(|m| {
                    m.stt_ok += 1;
                    m.stt_ms += ms;
                });
                s.monitor.finish(
                    &stt_job,
                    Level::Ok,
                    "stt",
                    format!("Heard · {short}"),
                    Some(json!({ "ms": ms, "chars": spoken.chars().count(), "text": spoken })),
                );
                spoken_text = t;
            }
            Ok(Err(ProviderError::NotConfigured)) => {
                s.monitor.count(|m| m.stt_skip += 1);
                s.monitor.finish(
                    &stt_job,
                    Level::Info,
                    "stt",
                    "Speech skipped · no STT model (type the details)",
                    None,
                );
            }
            Ok(Err(e)) => {
                let ms = stt_started.elapsed().as_millis() as u64;
                s.monitor.count(|m| {
                    m.stt_fail += 1;
                    m.stt_ms += ms;
                    m.errors += 1;
                });
                s.monitor.finish(
                    &stt_job,
                    Level::Warn,
                    "stt",
                    format!("Speech failed after {ms} ms · {e}"),
                    None,
                );
                provider_note = Some(format!("speech-to-text failed: {e}"));
            }
            Err(_) => {
                let ms = stt_started.elapsed().as_millis() as u64;
                s.monitor.count(|m| {
                    m.stt_fail += 1;
                    m.stt_ms += ms;
                    m.errors += 1;
                });
                s.monitor.finish(
                    &stt_job,
                    Level::Warn,
                    "stt",
                    format!("Speech timed out after {ms} ms, type the details"),
                    None,
                );
                provider_note = Some("speech-to-text took too long, type the details".into());
            }
        }
    } else {
        s.monitor.count(|m| m.stt_skip += 1);
        s.monitor.record(Level::Info, "stt", "No voice clip in this capture", None);
    }

    let mut m = merge::build(book, vision_out.as_ref(), &spoken_text);
    if let Some(n) = provider_note {
        m.warnings.push(n);
    }
    s.monitor.record(
        Level::Info,
        "merge",
        format!(
            "Draft · {} · R/{} · {} · {}",
            m.name.text().unwrap_or("no name"),
            m.receipt.text().unwrap_or("?"),
            m.mode.text().unwrap_or("mode?"),
            m.amount.text().map(|a| format!("₹{a}")).unwrap_or_else(|| "amount?".into()),
        ),
        Some(json!({ "warnings": &m.warnings })),
    );

    let draft = assemble(book, m, photo.url, audio_stored.map(|a| a.url), spoken_text, s.cfg.ledger_path.exists());
    s.db.save_draft(&draft, client_id.as_deref())?;
    s.monitor.record(
        Level::Ok,
        "draft",
        format!("Draft saved · awaiting review · {}", draft.id),
        Some(json!({ "id": draft.id, "book": book })),
    );
    tracing::info!(id = %draft.id, book, receipt = ?draft.receipt.text(), "captured");
    Ok(Json(draft))
}

fn assemble(
    book: i64,
    m: Merged,
    photo_url: String,
    audio_url: Option<String>,
    transcript: String,
    ledger_ok: bool,
) -> Draft {
    let mut warnings = m.warnings;
    if !ledger_ok {
        warnings.push("ledger file not found - pushing will fail until it is back".into());
    }
    Draft {
        id: uuid::Uuid::new_v4().to_string(),
        book,
        receipt: m.receipt,
        name: m.name,
        amount: m.amount,
        mode: m.mode,
        status: Status::Pending,
        raw_text: m.raw_text,
        vision_note: m.vision_note,
        transcript,
        photo_url,
        audio_url,
        warnings,
        created_at: chrono::Local::now().to_rfc3339(),
        committed_row: None,
    }
}

#[derive(Deserialize)]
pub struct EditBody {
    pub book: Option<i64>,
    pub receipt: Option<String>,
    pub name: Option<String>,
    pub amount: Option<String>,
    pub mode: Option<String>,
}

/// PATCH /api/draft/{id} - the reviewer's corrections win, and are marked manual.
pub async fn edit(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<EditBody>,
) -> ApiResult<Json<Draft>> {
    let mut d = s.db.get_draft(&id).ok_or_else(|| ApiError::NotFound("no such draft".into()))?;
    if d.status == Status::Committed {
        return Err(ApiError::BadRequest("already pushed to the ledger; it cannot be edited".into()));
    }
    if let Some(b) = body.book {
        if !(1..=999).contains(&b) {
            return Err(ApiError::BadRequest("book number must be 1-999".into()));
        }
        d.book = b;
    }
    let set = |f: &mut Field, v: Option<String>| {
        if let Some(v) = v {
            let v = v.trim().to_string();
            *f = if v.is_empty() { Field::empty(Source::Manual) } else { Field::new(v, Source::Manual, 1.0) };
        }
    };
    set(&mut d.receipt, body.receipt);
    // control characters break the xlsx shared-strings table, so clean here too
    set(&mut d.name, body.name.map(|n| crate::domain::clean_name(&n)));
    set(&mut d.amount, body.amount);
    if let Some(m) = body.mode {
        if !m.trim().is_empty() {
            let mode = Mode::parse(&m).ok_or_else(|| ApiError::BadRequest("mode must be cash or online".into()))?;
            d.mode = Field::new(mode.as_str(), Source::Manual, 1.0);
        }
    }
    d.warnings.retain(|w| !w.contains("is missing") && !w.contains("you said"));
    if d.name.text().is_some() && d.receipt.text().is_some() && d.amount.text().is_some() {
        d.warnings.retain(|w| !w.contains("type it before pushing"));
    }
    s.db.update_draft(&d)?;
    Ok(Json(d))
}

#[derive(serde::Serialize)]
pub struct ConfirmResponse {
    draft: Draft,
    row: u32,
    totals: crate::ledger::Totals,
}

/// POST /api/draft/{id}/confirm - the only place a row enters the real ledger.
pub async fn confirm(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult<Json<ConfirmResponse>> {
    let mut d = s.db.get_draft(&id).ok_or_else(|| ApiError::NotFound("no such draft".into()))?;
    if d.status == Status::Rejected {
        return Err(ApiError::BadRequest("this capture was rejected".into()));
    }
    // Idempotent: pushing twice must never write two rows.
    if d.status == Status::Committed {
        let totals = s.cfg.ledger_path.exists().then(|| {
            crate::ledger::totals(&s.cfg.ledger_path, &s.cfg.ledger_sheet).unwrap_or_default()
        });
        return Ok(Json(ConfirmResponse { draft: d.clone(), row: d.committed_row.unwrap_or(0), totals: totals.unwrap_or_default() }));
    }

    // One writer at a time; the lock is held across the blocking excel save.
    let _guard = s.ledger.lock().await;
    // A second tap may have committed this draft while we waited for the lock.
    d = s.db.get_draft(&id).ok_or_else(|| ApiError::NotFound("no such draft".into()))?;
    if d.status == Status::Committed {
        let totals = s.cfg.ledger_path.exists().then(|| {
            crate::ledger::totals(&s.cfg.ledger_path, &s.cfg.ledger_sheet).unwrap_or_default()
        });
        return Ok(Json(ConfirmResponse { draft: d.clone(), row: d.committed_row.unwrap_or(0), totals: totals.unwrap_or_default() }));
    }
    if d.status == Status::Rejected {
        return Err(ApiError::BadRequest("this capture was rejected".into()));
    }
    let row = d.row().map_err(|errs| ApiError::BadRequest(errs.join("; ")))?;

    let cfg = s.cfg.clone();
    let (b, r) = (row.book, row.receipt);
    let dup = tokio::task::spawn_blocking(move || {
        crate::ledger::find_receipt(&cfg.ledger_path, &cfg.ledger_sheet, b, r)
    })
    .await
    .map_err(|e| ApiError::Internal(e.into()))?
    .map_err(|e| {
        s.monitor.record(
            Level::Error,
            "ledger",
            format!("Could not read the ledger · {e:#}"),
            None,
        );
        ApiError::Internal(anyhow::anyhow!("cannot check the ledger: {e:#}"))
    })?;
    if let Some(existing) = dup {
        s.monitor.record(
            Level::Warn,
            "ledger",
            format!(
                "Duplicate refused · R/{} in book {} is already row {} · nothing written",
                row.receipt, row.book, existing
            ),
            Some(json!({ "row": existing, "book": row.book, "receipt": row.receipt, "refused": true })),
        );
        return Err(ApiError::Conflict(format!(
            "R/{} in book {} is already row {} of the ledger. Check the stub - nothing was written.",
            row.receipt, row.book, existing
        )));
    }

    let cfg = s.cfg.clone();
    let write = row.clone();
    let appended = tokio::task::spawn_blocking(move || crate::ledger::append(&cfg, &write))
        .await
        .map_err(|e| ApiError::Internal(e.into()))?
        .map_err(|e| {
            s.monitor.count(|m| m.errors += 1);
            s.monitor.record(
                Level::Error,
                "ledger",
                format!("Ledger write failed · {e:#}"),
                None,
            );
            ApiError::Internal(anyhow::anyhow!("could not write the ledger: {e:#}"))
        })?;

    d.status = Status::Committed;
    d.committed_row = Some(appended.row);
    if let Err(e) = s.db.set_status(&id, Status::Committed, Some(appended.row), &d) {
        // The row is already in the book. Say so loudly: a blind retry duplicates it.
        tracing::error!(id = %id, row = appended.row, "ledger written but draft not saved: {e:#}");
        s.monitor.record(
            Level::Error,
            "ledger",
            format!("Row {} is in Excel but the app could not record it · {e:#}", appended.row),
            Some(json!({ "row": appended.row })),
        );
        return Err(ApiError::Conflict(format!(
            "Row {} is already in Excel, but the app could not record it. Do not push again - check the sheet.",
            appended.row
        )));
    }
    s.monitor.count(|m| m.pushes += 1);
    s.monitor.set_ledger(s.cfg.ledger_path.exists(), appended.row, appended.totals);
    s.monitor.record(
        Level::Ok,
        "ledger",
        format!(
            "Row {} added · {} · R/{} · {} ₹{}",
            appended.row,
            row.name,
            row.receipt,
            row.mode.as_str(),
            row.amount
        ),
        Some(json!({
            "row": appended.row,
            "book": row.book,
            "receipt": row.receipt,
            "name": row.name,
            "mode": row.mode.as_str(),
            "amount": row.amount,
            "photo_url": &d.photo_url,
            "cash": appended.totals.cash,
            "online": appended.totals.online,
            "total": appended.totals.total,
        })),
    );
    tracing::info!(id = %id, row = appended.row, "pushed to ledger");
    Ok(Json(ConfirmResponse { draft: d, row: appended.row, totals: appended.totals }))
}

pub async fn reject(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult<Json<Draft>> {
    let mut d = s.db.get_draft(&id).ok_or_else(|| ApiError::NotFound("no such draft".into()))?;
    if d.status == Status::Committed {
        return Err(ApiError::BadRequest("already pushed; delete the row in Excel if it is wrong".into()));
    }
    d.status = Status::Rejected;
    s.db.set_status(&id, Status::Rejected, None, &d)?;
    s.monitor.record(Level::Info, "draft", format!("Draft rejected · {id}"), None);
    Ok(Json(d))
}
