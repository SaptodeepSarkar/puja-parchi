//! Append one validated row to the real CASH ledger, preserving everything else.
//!
//! The sheet layout (verified against the 2026 workbook, 2026-10-22):
//!   A=Book no, B=Receipt No, C=Name, D=Cash amount, E=Mode, F=Online amount
//! Cash row   -> D=amount, E="CASH",  F empty
//! Online row -> D empty,   E="ONLINE", F=amount
//! The sheet name has a trailing space ("CASH "); callers pass it in.
//!
//! Contract:
//! - Returns the 1-based row number written and the fresh totals.
//! - Writes a timestamped backup under `<ledger dir>/backups/` before saving,
//!   keeping the newest 10, so a bad write never costs the book.
//! - Writes via a temp file + rename, so a crash mid-save cannot truncate the
//!   ledger.
//! - Must not touch any other sheet, formula, or cell.
//! - Percentages / totals are recomputed from the sheet, not from a counter.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use umya_spreadsheet::{reader, writer, Workbook, Worksheet};

use super::domain::{LedgerRow, Mode};
use crate::config::Config;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Totals {
    pub cash: i64,
    pub online: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct Appended {
    pub row: u32,
    pub totals: Totals,
}

pub fn append(cfg: &Config, row: &LedgerRow) -> Result<Appended> {
    let name = row.name.trim();
    if row.book <= 0 {
        bail!("book number must be a positive number");
    }
    if row.receipt <= 0 {
        bail!("receipt number must be a positive number");
    }
    if row.amount <= 0 {
        bail!("amount must be a positive number");
    }
    if name.is_empty() {
        bail!("name is empty");
    }

    let path = &cfg.ledger_path;
    let mut book = read_book(path)?;
    let row_no = {
        let sheet = get_sheet(&book, path, &cfg.ledger_sheet)?;
        target_row(sheet, row)
    };

    {
        let sheet_key = sheet_name(&book, path, &cfg.ledger_sheet)?;
        let sheet = book
            .sheet_by_name_mut(&sheet_key)
            .map_err(|_| missing_sheet(path, &cfg.ledger_sheet))?;
        sheet.cell_mut((1, row_no)).set_value_number(row.book as f64);
        sheet.cell_mut((2, row_no)).set_value_number(row.receipt as f64);
        sheet.cell_mut((3, row_no)).set_value_string(name);
        match row.mode {
            Mode::Cash => {
                sheet.cell_mut((4, row_no)).set_value_number(row.amount as f64);
            }
            Mode::Online => {
                sheet.cell_mut((6, row_no)).set_value_number(row.amount as f64);
            }
        }
        sheet.cell_mut((5, row_no)).set_value_string(row.mode.as_str());
    }

    // Verify against what the workbook now holds, before touching the real file.
    {
        let sheet = get_sheet(&book, path, &cfg.ledger_sheet)?;
        let expected_amount_col = match row.mode {
            Mode::Cash => 4,
            Mode::Online => 6,
        };
        ensure!(
            sheet.value_number((1, row_no)) == Some(row.book as f64),
            "book number did not save as a number"
        );
        ensure!(
            sheet.value_number((2, row_no)) == Some(row.receipt as f64),
            "receipt number did not save as a number"
        );
        ensure!(
            sheet.value_number((expected_amount_col, row_no)) == Some(row.amount as f64),
            "amount did not save as a number"
        );
        ensure!(
            sheet.value((3, row_no)).trim() == name,
            "name did not save: wrote {name:?}, cell holds {:?}",
            sheet.value((3, row_no))
        );
        ensure!(
            sheet.value((5, row_no)).trim() == row.mode.as_str(),
            "mode did not save"
        );
    }

    backup(path)?;
    atomic_save(&book, path)?;
    // Totals come from the workbook we just wrote, not from a second read: a
    // re-read failure after the save would look like a failed push and invite a
    // duplicate row.
    let totals = totals_of(&book, &cfg.ledger_sheet)?;
    Ok(Appended { row: row_no, totals })
}

/// Sum columns D and F over the data rows (row 2 onwards).
pub fn totals(path: &Path, sheet: &str) -> Result<Totals> {
    let book = read_book(path)?;
    totals_of(&book, sheet)
}

fn totals_of(book: &Workbook, sheet: &str) -> Result<Totals> {
    let name = find_sheet(book, sheet)
        .ok_or_else(|| anyhow::anyhow!("sheet {sheet:?} not found"))?;
    let sheet = book
        .sheet_by_name(name)
        .map_err(|_| anyhow::anyhow!("sheet {sheet:?} not found"))?;
    let last = last_row_of(sheet);
    let (mut cash, mut online) = (0i64, 0i64);
    for r in 2..=last {
        // Trust the mode column: row 148 of the real book carries a stray
        // F=1100 next to E=CASH, which must not count as an online donation.
        let mode = sheet.value((5, r));
        if mode.trim().eq_ignore_ascii_case("online") {
            online += sheet.value_number((6, r)).unwrap_or(0.0).round() as i64;
        } else {
            cash += sheet.value_number((4, r)).unwrap_or(0.0).round() as i64;
        }
    }
    Ok(Totals { cash, online, total: cash + online })
}

/// Highest row that has any of A..F filled. Row 1 is the header.
/// Completed row already holding this book+receipt, if any. Receipt numbers are
/// unique per book, so a completed match means the same stub is being entered
/// twice. Pre-printed rows (only A and B filled) are not duplicates - they are
/// where the next push belongs.
pub fn find_receipt(path: &Path, sheet: &str, book: i64, receipt: i64) -> Result<Option<u32>> {
    let wb = read_book(path)?;
    let sheet = get_sheet(&wb, path, sheet)?;
    let last = last_row_of(sheet);
    for r in 2..=last {
        let b = sheet.value_number((1, r));
        let n = sheet.value_number((2, r));
        if b == Some(book as f64) && n == Some(receipt as f64) && !is_blank_stub(sheet, r) {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

pub fn last_data_row(path: &Path, sheet: &str) -> Result<u32> {
    let book = read_book(path)?;
    Ok(last_row_of(get_sheet(&book, path, sheet)?))
}

fn read_book(path: &Path) -> Result<Workbook> {
    if !path.exists() {
        bail!("ledger file not found: {}", path.display());
    }
    reader::xlsx::read(path)
        .with_context(|| format!("cannot read ledger {}", path.display()))
}

fn missing_sheet(path: &Path, sheet: &str) -> anyhow::Error {
    anyhow::anyhow!("sheet {:?} not found in {}", sheet, path.display())
}

/// The workbook's sheet name is `"CASH "` with a trailing space, and env vars get
/// trimmed. Match exactly first, then on the trimmed name, so a `.env` line
/// `PUJA_LEDGER_SHEET=CASH` still finds the right sheet.
fn sheet_name(book: &Workbook, path: &Path, wanted: &str) -> Result<String> {
    find_sheet(book, wanted)
        .map(str::to_string)
        .ok_or_else(|| missing_sheet(path, wanted))
}

fn find_sheet<'a>(book: &'a Workbook, wanted: &str) -> Option<&'a str> {
    let names: Vec<&str> = book.sheet_collection().iter().map(|s| s.name()).collect();
    names
        .iter()
        .copied()
        .find(|n| *n == wanted)
        .or_else(|| names.iter().copied().find(|n| n.trim() == wanted.trim()))
}

fn get_sheet<'a>(book: &'a Workbook, path: &Path, sheet: &str) -> Result<&'a Worksheet> {
    let name = sheet_name(book, path, sheet)?;
    book.sheet_by_name(&name).map_err(|_| missing_sheet(path, sheet))
}

/// Pre-printed stubs sit in the sheet with only A and B filled, so write into
/// that row instead of appending a second line for the same receipt number.
fn target_row(sheet: &Worksheet, row: &LedgerRow) -> u32 {
    let last = last_row_of(sheet);
    for r in 2..=last {
        if sheet.value_number((1, r)) == Some(row.book as f64)
            && sheet.value_number((2, r)) == Some(row.receipt as f64)
            && is_blank_stub(sheet, r)
        {
            return r;
        }
    }
    last + 1
}

/// A pre-printed row: book and receipt present, nothing else filled in.
fn is_blank_stub(sheet: &Worksheet, r: u32) -> bool {
    (3..=6u32).all(|c| sheet.value((c, r)).trim().is_empty())
}

/// Highest row with any value in A..F; 1 when only the header exists.
fn last_row_of(sheet: &Worksheet) -> u32 {
    let mut last = 1;
    for r in 1..=sheet.highest_row() {
        for c in 1..=6u32 {
            if let Some(cell) = sheet.cell((c, r)) {
                if !cell.value().trim().is_empty() {
                    last = r;
                    break;
                }
            }
        }
    }
    last
}

fn parent_dir(path: &Path) -> &Path {
    path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."))
}

fn file_stem(path: &Path) -> &str {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("ledger")
}

/// Copy before saving so a bad append can always be rolled back; keep the
/// newest 10 to bound disk use.
fn backup(path: &Path) -> Result<()> {
    let dir = parent_dir(path).join("backups");
    fs::create_dir_all(&dir)
        .with_context(|| format!("cannot create backup dir {}", dir.display()))?;
    let stem = file_stem(path);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let dest = dir.join(format!("{stem}-{stamp}-{}.xlsx", &unique[..6]));
    fs::copy(path, &dest)
        .with_context(|| format!("cannot back up {} to {}", path.display(), dest.display()))?;

    let prefix = format!("{stem}-");
    let mut old: Vec<PathBuf> = fs::read_dir(&dir)
        .with_context(|| format!("cannot list backup dir {}", dir.display()))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    // Timestamp names sort chronologically; drop everything past the newest 10.
    old.sort();
    let excess = old.len().saturating_sub(10);
    for old in old.into_iter().take(excess) {
        let _ = fs::remove_file(old);
    }
    Ok(())
}

/// Write next to the original, then rename over it, so a failed write never
/// truncates the ledger.
fn atomic_save(book: &Workbook, path: &Path) -> Result<()> {
    let tmp = parent_dir(path).join(format!("{}.tmp.xlsx", file_stem(path)));
    if let Err(e) = writer::xlsx::write(book, &tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("cannot write ledger {}", path.display()));
    }
    // Flush the bytes before the rename, or a power cut can leave a file that
    // exists but is empty.
    fs::File::open(&tmp)?.sync_all()?;
    fs::rename(&tmp, path).with_context(|| {
        format!("cannot replace ledger {} with {}", path.display(), tmp.display())
    })?;
    // And make the rename itself durable.
    fs::File::open(parent_dir(path))?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use umya_spreadsheet::Cell;

    fn real_ledger() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/ledger/Durga Puja 2026.xlsx")
    }

    fn cfg_for(path: &Path, sheet: &str) -> Config {
        Config {
            addr: "127.0.0.1:0".into(),
            tls_addr: "127.0.0.1:0".into(),
            tls: false,
            data_dir: path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            ledger_path: path.to_path_buf(),
            ledger_sheet: sheet.into(),
            app_dir: PathBuf::new(),
            pin: None,
            vision_provider: "none".into(),
            vision_base_url: String::new(),
            vision_api_key: None,
            vision_model: String::new(),
            stt_base_url: String::new(),
            stt_api_key: None,
            stt_model: String::new(),
            stt_language: None,
        }
    }

    fn formula_of(cell: &Cell) -> String {
        let f = cell.formula();
        if f.starts_with('=') { f.to_string() } else { format!("={f}") }
    }

    fn assert_empty(ws: &Worksheet, col: u32, row: u32) {
        let empty = ws.cell((col, row)).is_none_or(|c| c.value().trim().is_empty());
        assert!(empty, "expected column {col} row {row} to be empty");
    }

    #[test]
    fn append_preserves_workbook_and_totals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Durga Puja 2026.xlsx");
        fs::copy(real_ledger(), &path).unwrap();

        let cash_cfg = cfg_for(&path, "CASH ");
        let before = totals(&path, "CASH ").unwrap();
        let last = last_data_row(&path, "CASH ").unwrap();
        assert_eq!(last, 310, "real ledger is expected to have 310 data rows");

        let cash = LedgerRow {
            book: 18,
            receipt: 901,
            name: "TEST CASH".into(),
            amount: 501,
            mode: Mode::Cash,
        };
        let app = append(&cash_cfg, &cash).unwrap();
        assert_eq!(app.row, last + 1);
        assert_eq!(app.row, 311);
        assert_eq!(app.totals.cash, before.cash + 501);
        assert_eq!(app.totals.online, before.online);
        assert_eq!(app.totals.total, app.totals.cash + app.totals.online);

        let online = LedgerRow {
            book: 18,
            receipt: 902,
            name: "TEST ONLINE".into(),
            amount: 1100,
            mode: Mode::Online,
        };
        let app2 = append(&cash_cfg, &online).unwrap();
        assert_eq!(app2.row, last + 2);
        assert_eq!(app2.totals.cash, before.cash + 501);
        assert_eq!(app2.totals.online, before.online + 1100);
        assert_eq!(app2.totals.total, app2.totals.cash + app2.totals.online);

        let book = reader::xlsx::read(&path).unwrap();
        let ws = book.sheet_by_name("CASH ").unwrap();

        assert_eq!(ws.value_number((1, 311)), Some(18.0));
        assert_eq!(ws.value_number((2, 311)), Some(901.0));
        assert_eq!(ws.value((3, 311)), "TEST CASH");
        assert_eq!(ws.value_number((4, 311)), Some(501.0));
        assert_eq!(ws.value((5, 311)), "CASH");
        assert_empty(ws, 6, 311);

        assert_eq!(ws.value_number((1, 312)), Some(18.0));
        assert_eq!(ws.value_number((2, 312)), Some(902.0));
        assert_eq!(ws.value((3, 312)), "TEST ONLINE");
        assert_empty(ws, 4, 312);
        assert_eq!(ws.value((5, 312)), "ONLINE");
        assert_eq!(ws.value_number((6, 312)), Some(1100.0));

        // Formulas on the untouched side of the sheet and the other sheet survive.
        let l205 = ws.cell("L205").unwrap();
        assert!(l205.is_formula());
        assert_eq!(formula_of(l205), "=SUM(L201:L204)");

        let online_ws = book.sheet_by_name("ONLINE").unwrap();
        let a8 = online_ws.cell("A8").unwrap();
        assert!(a8.is_formula());
        assert_eq!(formula_of(a8), "=A7+1");
        let e66 = online_ws.cell("E66").unwrap();
        assert!(e66.is_formula());
        assert_eq!(formula_of(e66), "=SUM(E2:E65)");

        // Backups exist and the original still opens.
        let backups = dir.path().join("backups");
        let count = fs::read_dir(&backups)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("xlsx")))
            .count();
        assert!(count >= 1, "expected a timestamped backup");
        assert!(reader::xlsx::read(&path).is_ok());
    }

    #[test]
    fn invalid_rows_are_rejected_before_touching_the_file() {
        let cfg = cfg_for(Path::new("does-not-exist.xlsx"), "CASH ");
        let bad = LedgerRow {
            book: 0,
            receipt: 1,
            name: "A".into(),
            amount: 1,
            mode: Mode::Cash,
        };
        let err = append(&cfg, &bad).unwrap_err().to_string();
        assert!(err.contains("book"), "unexpected error: {err}");

        let missing = LedgerRow { book: 1, ..bad.clone() };
        let err = append(&cfg, &missing).unwrap_err().to_string();
        assert!(err.contains("not found"), "unexpected error: {err}");
    }

    #[test]
    fn totals_follow_the_mode_column_not_the_amount_columns() {
        let t = totals(&real_ledger(), "CASH ").unwrap();
        // Book state on 2026-10-22. Row 148 carries F=1100 next to E=CASH; it
        // must count as cash, not as an online donation.
        assert_eq!(t.cash, 121_654);
        assert_eq!(t.online, 26_226);
        assert_eq!(t.total, 147_880);
    }

    #[test]
    fn sheet_name_survives_a_trimmed_env_var() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("book.xlsx");
        std::fs::copy(real_ledger(), &path).unwrap();
        // what `.env` produces after trimming "CASH "
        let cfg = cfg_for(&path, "CASH");
        let before = last_data_row(&path, "CASH").unwrap();
        let row = LedgerRow {
            book: 18,
            receipt: 9503,
            name: "TRIM TEST".into(),
            amount: 11,
            mode: Mode::Cash,
        };
        let done = append(&cfg, &row).unwrap();
        assert_eq!(done.row, before + 1);
        assert_eq!(totals(&path, "CASH ").unwrap().cash, totals(&path, "CASH").unwrap().cash);
    }
}
