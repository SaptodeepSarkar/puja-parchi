use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;
use crate::ledger::Totals;
use crate::monitor::Monitor;
use crate::providers::{Stt, Vision};

pub struct AppState {
    pub cfg: Config,
    pub db: Db,
    pub vision: Arc<dyn Vision>,
    pub stt: Arc<dyn Stt>,
    /// One Excel writer at a time. Held across the blocking save only.
    pub ledger: tokio::sync::Mutex<()>,
    /// Live pipeline events for the laptop console at /monitor.
    pub monitor: Monitor,
}

impl AppState {
    pub fn new(cfg: Config) -> anyhow::Result<Arc<Self>> {
        let db = Db::open(&cfg.data_dir.join("puja.db"))?;
        let vision = crate::providers::vision_from_config(&cfg);
        let stt = crate::providers::stt_from_config(&cfg);
        // Seed the monitor once so /monitor has numbers before the first push.
        let monitor = Monitor::new();
        if cfg.ledger_path.exists() {
            match crate::ledger::last_data_row(&cfg.ledger_path, &cfg.ledger_sheet)
                .and_then(|rows| Ok((rows, crate::ledger::totals(&cfg.ledger_path, &cfg.ledger_sheet)?)))
            {
                Ok((rows, totals)) => monitor.set_ledger(true, rows, totals),
                Err(e) => {
                    tracing::warn!("ledger exists but could not be read at startup: {e:#}");
                    monitor.set_ledger(false, 0, Totals::default());
                }
            }
        }
        Ok(Arc::new(Self {
            cfg,
            db,
            vision,
            stt,
            ledger: tokio::sync::Mutex::new(()),
            monitor,
        }))
    }
}
