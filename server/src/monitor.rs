//! Live data for the laptop ops console at /monitor.
//!
//! In memory only, on purpose: the console is a window on the running pipeline,
//! not a second ledger. The xlsx and SQLite stay the record, so a restart makes
//! the log start empty without losing a single receipt.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;

use crate::ledger::Totals;

const RING: usize = 300;

#[derive(Debug, Clone, Copy)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// One line in the console log. `data` carries the structured bits (the row
/// that was written, what the vision model read, timings) for the page.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub seq: u64,
    pub at: String,
    pub level: &'static str,
    pub kind: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Running {
    pub id: String,
    pub label: String,
    pub ms: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Metrics {
    pub captures: u64,
    pub photo_bytes: u64,
    pub audio_bytes: u64,
    pub vision_ok: u64,
    pub vision_fail: u64,
    pub vision_skip: u64,
    pub vision_ms: u64,
    pub stt_ok: u64,
    pub stt_fail: u64,
    pub stt_skip: u64,
    pub stt_ms: u64,
    pub pushes: u64,
    pub errors: u64,
}

/// What the sheet looked like when it was last read or written.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerState {
    pub ok: bool,
    pub rows: u32,
    pub cash: i64,
    pub online: i64,
    pub total: i64,
    pub at: String,
    /// The tail of the sheet, for the console's preview.
    pub sheet: Vec<crate::ledger::SheetRow>,
}

/// How many rows of the sheet the console previews. Enough to see the row that
/// just landed and the pre-printed stubs it landed among.
pub const PREVIEW_ROWS: u32 = 30;

/// Whether the model servers were actually answering when last checked.
#[derive(Debug, Clone, Serialize)]
pub struct ModelHealth {
    pub vision: bool,
    pub stt: bool,
    pub at: String,
}

struct Checked {
    at: Instant,
    health: ModelHealth,
}

struct Job {
    id: String,
    label: String,
    started: Instant,
}

pub struct Monitor {
    started: Instant,
    seq: AtomicU64,
    events: Mutex<VecDeque<Event>>,
    running: Mutex<Vec<Job>>,
    metrics: Mutex<Metrics>,
    ledger: Mutex<Option<LedgerState>>,
    models: Mutex<Option<Checked>>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            seq: AtomicU64::new(0),
            events: Mutex::new(VecDeque::with_capacity(RING)),
            running: Mutex::new(Vec::new()),
            metrics: Mutex::new(Metrics::default()),
            ledger: Mutex::new(None),
            models: Mutex::new(None),
        }
    }

    pub fn uptime_s(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub fn record(
        &self,
        level: Level,
        kind: &'static str,
        message: impl Into<String>,
        data: Option<Value>,
    ) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let event = Event {
            seq,
            at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
            level: level.as_str(),
            kind,
            message: message.into(),
            data,
        };
        let mut ring = self.events.lock().unwrap_or_else(|e| e.into_inner());
        if ring.len() == RING {
            ring.pop_front();
        }
        ring.push_back(event);
        seq
    }

    /// A model call just started: the console shows it as a spinner until the
    /// matching `finish`.
    pub fn start(&self, id: &str, label: impl Into<String>) {
        let label = label.into();
        self.running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Job { id: id.to_string(), label: label.clone(), started: Instant::now() });
        self.record(Level::Info, "job", label, Some(serde_json::json!({ "job": id })));
    }

    pub fn finish(
        &self,
        id: &str,
        level: Level,
        kind: &'static str,
        message: impl Into<String>,
        data: Option<Value>,
    ) {
        self.running.lock().unwrap_or_else(|e| e.into_inner()).retain(|j| j.id != id);
        self.record(level, kind, message, data);
    }

    pub fn count(&self, f: impl FnOnce(&mut Metrics)) {
        f(&mut self.metrics.lock().unwrap_or_else(|e| e.into_inner()));
    }

    pub fn set_ledger(&self, ok: bool, rows: u32, totals: Totals, sheet: Vec<crate::ledger::SheetRow>) {
        *self.ledger.lock().unwrap_or_else(|e| e.into_inner()) = Some(LedgerState {
            ok,
            rows,
            cash: totals.cash,
            online: totals.online,
            total: totals.total,
            at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
            sheet,
        });
    }

    pub fn ledger(&self) -> Option<LedgerState> {
        self.ledger.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_models(&self, vision: bool, stt: bool) {
        let health = ModelHealth {
            vision,
            stt,
            at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
        };
        *self.models.lock().unwrap_or_else(|e| e.into_inner()) = Some(Checked { at: Instant::now(), health });
    }

    /// The last check, or None when it is older than `max_age` so the caller
    /// re-probes instead of showing a stale "ready".
    pub fn models(&self, max_age: std::time::Duration) -> Option<ModelHealth> {
        self.models
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|c| c.at.elapsed() <= max_age)
            .map(|c| c.health.clone())
    }

    /// `since` is the last seq the page has seen; the reply carries only newer
    /// events, so a 1 s poll never repeats a line or a pop-up.
    pub fn snapshot(&self, since: u64) -> (u64, Vec<Event>, Vec<Running>, Metrics) {
        let cursor = self.seq.load(Ordering::Relaxed);
        let events = self
            .events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|e| e.seq > since)
            .cloned()
            .collect();
        let running = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|j| Running {
                id: j.id.clone(),
                label: j.label.clone(),
                ms: j.started.elapsed().as_millis() as u64,
            })
            .collect();
        let metrics = *self.metrics.lock().unwrap_or_else(|e| e.into_inner());
        (cursor, events, running, metrics)
    }
}

/// Short id that ties a start event to its finish event.
pub fn job_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// "904 B" / "2345 KB" for the console messages.
pub fn size(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else {
        format!("{} KB", n / 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn monitor() -> Monitor {
        Monitor::new()
    }

    #[test]
    fn a_start_is_visible_until_its_finish() {
        let m = monitor();
        m.start("j1", "Vision · reading the photo");
        let (_, events, running, _) = m.snapshot(0);
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, "j1");
        assert_eq!(events.len(), 1);

        m.finish("j1", Level::Ok, "vision", "Vision read the stub", None);
        let (_, events, running, _) = m.snapshot(1);
        assert!(running.is_empty());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "vision");
        assert_eq!(events[0].level, "ok");
    }

    #[test]
    fn since_only_returns_newer_events() {
        let m = monitor();
        let first = m.record(Level::Info, "capture", "one", None);
        m.record(Level::Info, "capture", "two", None);
        let (cursor, events, _, _) = m.snapshot(first);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "two");
        assert_eq!(cursor, 2);
        assert!(m.snapshot(cursor).1.is_empty());
    }

    #[test]
    fn the_ring_keeps_the_newest_events() {
        let m = monitor();
        for i in 0..RING + 20 {
            m.record(Level::Info, "capture", format!("n{i}"), None);
        }
        let (cursor, events, _, _) = m.snapshot(0);
        assert_eq!(cursor, (RING + 20) as u64);
        assert_eq!(events.len(), RING);
        assert_eq!(events.last().unwrap().message, format!("n{}", RING + 19));
    }

    #[test]
    fn counters_and_the_ledger_snapshot() {
        let m = monitor();
        m.count(|c| {
            c.captures += 1;
            c.vision_ok += 1;
            c.vision_ms += 420;
            c.pushes += 1;
        });
        m.set_ledger(true, 311, Totals { cash: 121_654, online: 26_226, total: 147_880 }, Vec::new());
        let (_, _, _, metrics) = m.snapshot(0);
        assert_eq!(metrics.captures, 1);
        assert_eq!(metrics.vision_ms, 420);
        let ledger = m.ledger().unwrap();
        assert!(ledger.ok);
        assert_eq!(ledger.rows, 311);
        assert_eq!(ledger.total, 147_880);
    }

    #[test]
    fn event_data_survives_json() {
        let m = monitor();
        m.record(Level::Ok, "ledger", "row", Some(json!({ "row": 311 })));
        let (_, events, _, _) = m.snapshot(0);
        assert_eq!(events[0].data.as_ref().unwrap()["row"], 311);
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(size(904), "904 B");
        assert_eq!(size(2_345 * 1024), "2345 KB");
    }
}
