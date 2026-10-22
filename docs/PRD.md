# PRD — Puja Parchi

Receipt capture for the 2026 Durga Puja collection drive.
Version 1.0 · 22 Oct 2026 · owner: the volunteer holding the receipt book.

---

## 1. Problem

The donation ledger (`Durga Puja 2026.xlsx`, sheet `CASH `) is filled by hand from
paper receipt stubs. Three things go wrong at the pandal:

1. **Transcription.** Handwriting is read once, in a queue, by tired eyes. Names come
   out as `SUDDHA` for `SUDHA`, `PRASSAD` for `PRASAD`, `DEVR=ENDRA` for `DEVENDRA`.
2. **Mode.** An online payment is a blank cash column plus an amount in the online
   column. Volunteers forget, and the cash total drifts.
3. **Traceability.** A photo of the stub exists only on someone's phone, if at all.
   When a row is questioned there is no evidence to check.

The list outgrew one column block earlier in the season: extra names were written to
the side of the sheet (`K:O`) and had to be shifted back to the bottom by hand. That is
the failure mode this product removes — one list, one write path, no side lists.

## 2. Who it is for

| Person | What they do | What they need |
|---|---|---|
| Volunteer at the desk | Takes the money, holds the stub, has one free hand | One tap to photograph, 6 seconds to speak, one tap to confirm |
| Treasurer | Reconciles cash and online at the end of the day | The ledger is the source of truth, totals always add up |
| Book keeper | Checks a disputed receipt later | Photo, voice clip and the machine's reasoning on disk |

Not a multi-tenant product. One laptop, one or two phones, one wifi, one event.

## 3. Honest scope

**In scope (v1)**
- Mobile web app (installable PWA) served by a local Rust server.
- Camera → 6 s voice note → vision model → name normalisation → review → Excel row.
- Photo and voice clip archived per book number.
- Cash/online split written the way the existing sheet already works.
- Retry-safe capture, idempotent push, backup before every ledger write.
- Works with **no API key at all**: photo, typing and push always work. Spoken
  details need either a configured OpenAI-compatible STT or Chrome's own
  dictation (which needs internet); without one of those, type the four fields.

**Out of scope (v1)**
- Editing or deleting ledger rows from the app (Excel remains the master; fix it there).
- A native APK (no Android SDK on the build machine — see D-1).
- Multi-user accounts, roles, cloud sync, dashboards.
- OCR of handwriting as the primary source. The volunteer's voice is the primary source
  because OCR on Indian handwriting is genuinely weak; the vision model is a second opinion.
- Cheque/DD modes. Only `CASH` and `ONLINE` exist in the sheet.

**Non-goal stated explicitly:** the app never re-spells a name a human typed. A person's
correction is final (see FR-9).

## 4. The one job to be done

> "I am holding a receipt stub and someone's money. Let me get one clean, provable row
> into the ledger in under 20 seconds without putting the book down."

Every requirement below is judged against that sentence.

## 5. Functional requirements

**Capture**
- **FR-1** The book number is chosen before capture and persists across app restarts,
  server restarts, and receipts, until changed. It is never inferred from the photo.
- **FR-2** One tap opens the phone camera (`capture="environment"`); the photo is
  downscaled to ≤1600 px / JPEG 0.85 before upload.
- **FR-3** The image is stored on the server at `data/photos/<book>/<timestamp>-<id>.jpg`
  **before** any model sees it. A model outage never loses a photo.
- **FR-4** Voice capture starts automatically after the photo: a 6-second countdown with
  live dictation text, a **Stop & use this** button, and a hard 6 s cutoff. If the
  microphone is unavailable the photo is still usable and the fields are typed.

**Extraction**
- **FR-5** The vision model returns: raw text, receipt number, name, amount, mode,
  its own note, and a confidence. It must not invent values; unreadable means `null`.
- **FR-6** Spoken input is parsed for name, receipt number, mode and amount in any order,
  English or Hindi numerals, digits or words.
- **FR-7** Names are repaired against a gazetteer built from this ledger's own 300+ names
  plus regional given names and business words. Every change is shown as evidence
  (`SUDDHA -> SUDHA (gazetteer, 0.96)`), never applied silently.
- **FR-8** Arbitration: the volunteer's voice wins over the vision model. A disagreement
  is surfaced as a warning, with the other reading kept in `alternates`.
- **FR-9** A manually edited field is marked `manual`, confidence 1.0, and is never
  rewritten by the gazetteer.
- **FR-10** No `mode` written on the stub means **CASH** (the book's own convention).

**Review and push**
- **FR-11** Before pushing, the reviewer sees each field with its source badge, the
  warnings, the photo, the transcript, the raw vision text and the field evidence.
- **FR-12** Push validates: book 1–999, receipt 1–9999, name ≥2 characters,
  amount 1–10,000,000. Invalid drafts cannot be pushed and say why.
- **FR-13** Cash rows write `D=amount, E=CASH`; online rows write `E=ONLINE, F=amount`,
  leaving the cash column empty — exactly the layout already in the sheet.
- **FR-14** The row goes where the book expects it: if `CASH ` already has a
  pre-printed line for that book+receipt (A and B filled, C–F empty), that line is
  filled in; otherwise the row goes under the last used row. Never a side block.
- **FR-15** Pushing the same draft twice writes **one** row and returns the original row
  number.
- **FR-16** A receipt number that is already in the book cannot be pushed twice: the
  push is refused with the row that already holds it, and nothing is written.
- **FR-17** Before saving, the ledger is copied to `data/ledger/backups/` (newest 10 kept)
  and saved via temp-file + rename, so a crash cannot truncate the book.
- **FR-18** After a push the screen shows the row number and fresh sheet totals.

**Operations**
- **FR-19** `/api/health` reports version, provider, ledger presence and the sticky book.
- **FR-20** The server prints the LAN URL(s) for the phone on boot.
- **FR-21** If `PUJA_PIN` is set, every API call and every photo/voice file requires
  the PIN — `x-pin` header for the app's calls, `?pin=` for `<img>`/`<audio>` URLs.
  Only `/api/health` and `/ca.crt` stay open so a phone can join.

## 6. Non-functional requirements

| Area | Requirement |
|---|---|
| Latency | Photo upload + merge under 3 s without a vision model; under 12 s with one. Model calls time out (vision 45 s, speech 20 s) and degrade instead of hanging. |
| Reliability | No receipt is ever silently lost: capture is persisted before interpretation, retries are keyed by `client_id`, ledger writes are atomic with backups. |
| Safety | The live Excel file is only ever appended to; no row is edited or deleted by the app. |
| Privacy | Photos and voice clips stay on the laptop. Only a capture is sent to a configured model provider, and only when the operator sets a key. |
| Usability | Readable at arm's length in a dim pandal: ≥16 px body text, ≥44 px touch targets (≥56 px for the primary action), one primary action per screen. |
| Security | Bind to LAN only; PIN for writes; image magic-byte check; body cap 20 MB; book folder name is numeric; no path from user input. |
| Portability | Rust server on Linux/macOS; any Android Chrome or iOS Safari phone; no app store. |

## 7. Success criteria

1. A volunteer who has never seen the app completes a receipt unaided after one demo.
2. 20 consecutive receipts produce 20 rows, in book order, with cash/online correctly split.
3. Kill the server mid-push: the ledger still opens, no partial row, a backup exists.
4. Kill the wifi after the photo: the photo is on disk and the draft can be retried.
5. With no API key configured, a receipt still goes through: spoken fields when the
   phone has internet (Chrome dictation), typed fields otherwise.
6. Sheet totals in the app equal totals computed by opening the file in Excel.

## 8. Key decisions

| # | Decision | Why |
|---|---|---|
| D-1 | PWA, not a native APK | No Android SDK/JDK on the build machine; a phone browser is the only testable target today. Native shell can wrap the same UI later. |
| D-2 | Rust server is also the static host and the Excel writer | One process, one folder, no CORS, no second deploy. |
| D-3 | `umya-spreadsheet` in place | Verified against the real workbook: formulas (`L205`, `ONLINE!A8`, `E66`) survive a round trip. |
| D-4 | Voice is primary, vision is secondary | OCR on Indian handwriting is the weakest link; the person holding the stub is the best sensor. |
| D-5 | Local CA + HTTPS on :8443 | Chrome blocks microphone and service workers on plain-HTTP LAN origins. Click-through self-signed certs do not restore them. |
| D-6 | SQLite, not Postgres | One writer, a few hundred rows, zero ops. |
| D-7 | Deterministic merge rules, not a second LLM | Rules are testable, explainable and cheap; the evidence panel is the explainability. |

## 9. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Phone cannot install the CA → no mic | Photo-only mode still works; typing is always available. Setup page documents the one-time install. |
| Vision model hallucinates a name | Voice wins; every field carries source + confidence; the reviewer must tap push. |
| Excel open on the laptop while pushing | Atomic replace, backup first; docs say close the workbook during collection. |
| Two phones pushing at once | Single writer mutex on the server; SQLite serialises drafts. |
| Sheet layout changes (new columns) | Column mapping is one constant in `ledger.rs` with a test pinned to the real file. |
| Ledger copy drifts from the live file | `PUJA_LEDGER_PATH` points at whichever file is authoritative; the app shows the exact path it writes. |

## 10. Open questions

1. Should online rows keep `D=0` (as a few legacy rows do) or stay empty? Chosen: empty,
   matching the majority and keeping the cash sum honest.
2. Do we need an undo? Excel is the undo. Revisit if volunteers ask.
3. Which vision model is affordable per receipt? Operator's choice via env; costs are
   documented in `docs/STACK.md`.
4. Hindi dictation quality on the actual phone is untested — flagged for a 5-minute
   check on the event phone before the first real receipt.
