//! SQLite store for drafts and the sticky book number.
//!
//! Tiny single-user workload: a `std::sync::Mutex<Connection>` is plenty and no
//! lock is ever held across an await.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{Draft, Mode, Status};

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS drafts (
                id            TEXT PRIMARY KEY,
                client_id     TEXT UNIQUE,
                book          INTEGER NOT NULL,
                receipt       INTEGER,
                name          TEXT,
                amount        INTEGER,
                mode          TEXT,
                status        TEXT NOT NULL,
                created_at    TEXT NOT NULL,
                committed_row INTEGER,
                json          TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS drafts_status ON drafts(status, created_at DESC);
            CREATE TABLE IF NOT EXISTS settings (k TEXT PRIMARY KEY, v TEXT NOT NULL);
            "#,
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.lock()
            .query_row("SELECT v FROM settings WHERE k = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.lock().execute(
            "INSERT INTO settings (k, v) VALUES (?1, ?2)
             ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn active_book(&self) -> Option<i64> {
        self.get_setting("active_book")?.parse().ok()
    }

    pub fn recent_books(&self) -> Vec<i64> {
        self.get_setting("recent_books")
            .and_then(|v| serde_json::from_str::<Vec<i64>>(&v).ok())
            .unwrap_or_default()
    }

    /// Remember the book the phone is working on. Stays until changed.
    pub fn set_active_book(&self, book: i64) -> rusqlite::Result<()> {
        self.set_setting("active_book", &book.to_string())?;
        let mut books = self.recent_books();
        books.retain(|b| *b != book);
        books.insert(0, book);
        books.truncate(8);
        self.set_setting("recent_books", &serde_json::to_string(&books).unwrap_or_default())
    }

    pub fn save_draft(&self, d: &Draft, client_id: Option<&str>) -> anyhow::Result<()> {
        let json = serde_json::to_string(d)?;
        self.lock().execute(
            "INSERT INTO drafts (id, client_id, book, receipt, name, amount, mode, status,
                                 created_at, committed_row, json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                d.id,
                client_id,
                d.book,
                d.receipt.number(),
                d.name.text(),
                d.amount.number(),
                d.mode.text().unwrap_or("CASH"),
                d.status.as_str(),
                d.created_at,
                d.committed_row,
                json
            ],
        )?;
        Ok(())
    }

    pub fn get_draft(&self, id: &str) -> Option<Draft> {
        let json: Option<String> = self
            .lock()
            .query_row("SELECT json FROM drafts WHERE id = ?1", params![id], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        json.and_then(|j| serde_json::from_str(&j).ok())
    }

    pub fn client_id_of(&self, id: &str) -> Option<String> {
        self.lock()
            .query_row("SELECT client_id FROM drafts WHERE id = ?1", params![id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn find_by_client_id(&self, client_id: &str) -> Option<Draft> {
        let json: Option<String> = self
            .lock()
            .query_row("SELECT json FROM drafts WHERE client_id = ?1", params![client_id], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        json.and_then(|j| serde_json::from_str(&j).ok())
    }

    pub fn set_status(&self, id: &str, status: Status, row: Option<u32>, d: &Draft) -> anyhow::Result<()> {
        let json = serde_json::to_string(d)?;
        self.lock().execute(
            "UPDATE drafts SET status = ?2, committed_row = ?3, json = ?4 WHERE id = ?1",
            params![id, status.as_str(), row, json],
        )?;
        Ok(())
    }

    /// Replace the editable fields after the reviewer corrected them.
    pub fn update_draft(&self, d: &Draft) -> anyhow::Result<()> {
        let json = serde_json::to_string(d)?;
        self.lock().execute(
            "UPDATE drafts SET receipt = ?2, name = ?3, amount = ?4, mode = ?5, json = ?6 WHERE id = ?1",
            params![d.id, d.receipt.number(), d.name.text(), d.amount.number(),
                    d.mode.text().unwrap_or("CASH"), json],
        )?;
        Ok(())
    }

    pub fn list(&self, limit: u32) -> Vec<Draft> {
        let conn = self.lock();
        let mut stmt = match conn.prepare(
            "SELECT json FROM drafts WHERE status != 'rejected' ORDER BY created_at DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![limit], |r| r.get::<_, String>(0));
        match rows {
            Ok(rows) => rows.filter_map(|r| r.ok()).filter_map(|j| serde_json::from_str(&j).ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// How many receipts this app pushed today.
    pub fn today_count(&self) -> i64 {
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.lock()
            .query_row(
                "SELECT COUNT(*) FROM drafts WHERE status = 'committed' AND created_at LIKE ?1",
                params![format!("{day}%")],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// Cash / online totals of what this app pushed today (not the whole book).
    pub fn today_totals(&self) -> (i64, i64) {
        let conn = self.lock();
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        let sum = |mode: Mode| -> i64 {
            conn.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM drafts
                 WHERE status = 'committed' AND mode = ?1 AND created_at LIKE ?2",
                params![mode.as_str(), format!("{day}%")],
                |r| r.get(0),
            )
            .unwrap_or(0)
        };
        (sum(Mode::Cash), sum(Mode::Online))
    }
}
