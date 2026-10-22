# Stack

Two deployables in one folder: a Rust server that owns the ledger, and a React PWA that
runs in the phone browser. One binary serves both, so there is nothing to pair or configure.

```
durga-puja-2026/
├── server/            Rust 1.99 · axum · umya-spreadsheet · SQLite · rustls
├── app/               React 19 · TypeScript · Vite 7 · PWA
├── data/              ledger, photos/<book>/, audio/<book>/, certs/, puja.db
├── docs/              PRD · FLOW · STACK · PHASES · research
└── scripts/           dev.sh · test.sh · make-icons.py
```

## Server

| Concern | Choice | Why this, not the alternative |
|---|---|---|
| Language | Rust 2024 edition | One static binary on a laptop, no runtime to install; the Excel writer must not corrupt a file, so memory safety matters. |
| HTTP | `axum` 0.8 | Small, tower-based, first-class multipart (photo upload) and typed extractors. |
| Runtime | `tokio` | Required by axum; also gives us the blocking-task escape hatch for Excel I/O. |
| Excel | `umya-spreadsheet` 3.1 | Only mature Rust xlsx writer. Verified against the real workbook: appending a row preserved every formula and value in `CASH ` and `ONLINE`. |
| State | SQLite via `rusqlite` (bundled) | Drafts, sticky book, history. Single writer, no server to run. `bundled` = no system libsqlite. |
| TLS | `axum-server` + `rustls`, certs from `rcgen` | Chrome refuses mic + service worker on http://LAN. Local CA installed once on the phone. |
| Vision / STT | Any OpenAI-compatible endpoint (`reqwest`) | OpenAI, Gemini's `/v1beta/openai`, Groq, vLLM — and local servers: Ollama for vision, whisper.cpp for speech. A model on this machine needs no key at all. No SDK lock-in. |
| Auth | `x-pin` header on writes | One shared PIN on one wifi; no accounts to manage. |
| Time | `chrono` local time | Filenames and rows should read like the book: IST. |
| Logs | `tracing` + `tower-http` | "Which receipt failed and why" at the pandal. |

Deliberately **not** used: an ORM (five queries), a queue (single writer), a framework
for config (env vars), Docker (the target is one laptop, not a fleet).

### API

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/health` | version, provider names, ledger path/presence, sticky book |
| GET | `/api/book` | current book + recently used |
| POST | `/api/book` | set the sticky book `{book}` |
| POST | `/api/capture` | multipart: `image` (required), `audio`, `transcript`, `book`, `client_id` → `Draft` |
| GET | `/api/drafts?limit=` | newest drafts (pending + committed) |
| GET | `/api/draft/{id}` | one draft |
| PATCH | `/api/draft/{id}` | reviewer corrections `{receipt,name,amount,mode,book}` |
| POST | `/api/draft/{id}/confirm` | append to Excel → `{draft,row,totals}` |
| POST | `/api/draft/{id}/reject` | discard a pending draft |
| GET | `/api/stats` | today's totals + whole-sheet totals |
| GET | `/api/monitor?since=` | ops console snapshot: server, models, counters, in-flight jobs, new events, recent rows |
| GET | `/monitor` | the laptop console page (open; every number behind it is PIN-guarded) |
| GET | `/ca.crt` | the local CA, for the phone's one-time trust install |
| GET | `/media/...` | photos and audio |
| GET | `/` | the built PWA |

`Draft` carries, per field, `value`, `source` (`voice|vision|ocr|namedb|manual|default`),
`confidence`, `evidence[]` and `alternates[]`, plus `raw_text`, `vision_note`,
`transcript`, `warnings[]`, `photo_url`, `audio_url`, `status`.

### Excel mapping (the part that must never be wrong)

```
CASH sheet, header row 1
A Book no │ B Receipt No │ C Name │ D Amount │ E Mode │ F Online amount
(F1 is blank in the real sheet; the app writes online amounts there, as the
existing rows do. D1 is "Amount ".)

cash   → A=book B=receipt C=NAME D=amount E="CASH"   F empty
online → A=book B=receipt C=NAME D empty   E="ONLINE" F=amount
```

Every write: validate → read → fill the pre-printed row for that receipt, else
append under the last used row → verify in memory →
backup (keep 10) → write temp → rename → re-read totals. Pinned by a test against a copy
of the real file (`server/src/ledger.rs`).

## App

| Concern | Choice | Why |
|---|---|---|
| Framework | React 19 + TypeScript | Three screens and a form; nothing heavier is earned. |
| Build | Vite 7 | Instant dev server, one `dist/` the Rust server hosts. |
| State | `useState` + one `App` shell | No Redux for four fields and a book number. |
| Camera | `<input type="file" capture="environment">` | One tap, native camera quality, no in-page permission, works even on http. |
| Voice | `MediaRecorder` + `SpeechRecognition` (`en-IN`) | Clip for the server + instant text for the reviewer; both degrade to typing. |
| Image | canvas → 1600 px / JPEG 0.85 | 3–5 MB → ~350 KB; under a second on pandal wifi. |
| Styling | one hand-written CSS file, mobile-first | A component library would outweigh the app. |
| PWA | manifest + 192/512/maskable icons + 20-line service worker | Installable on the home screen; shell cached, API never cached. |
| Tests | Vitest (pure helpers) + Playwright (full flow) | The flow is the product; it is tested in a real browser. |

Deliberately **not** used: a UI kit, a router (a tab enum), IndexedDB (SQLite lives
server-side).

### Android app (`android/`)

| Concern | Choice | Why |
|---|---|---|
| Language / UI | Kotlin + Jetpack Compose | A real app was asked for; Compose gives four screens without XML. |
| Transport | OkHttp + `org.json` | Multipart upload and the server's own JSON, no codegen and no Moshi. |
| Camera | CameraX `ImageCapture` | Preview plus a JPEG the server can read; no intent round-trip. |
| Microphone | `AudioRecord` → 16 kHz mono **WAV** | Every speech model takes WAV without a converter, and one process owns the mic. |
| Dictation | none — the clip goes to the server | Two live mic users is how the browser version silently heard nothing. |
| Transport security | plain `http://<laptop>:8080` | The mic-needs-https rule is a *browser* rule; an app is exempt, so no CA install. |
| Signing | debug keystore | Installed from adb onto the pandal phone; not a store release. |
| Finish-build | `./gradlew assembleDebug` | Only toolchain requirement: JDK 17 + Android SDK (`android-35`, `build-tools 35.0.0`). |

Deliberately **not** used: an HTTP framework (Retrofit — a dozen calls), a DI container,
a navigation library (a tab enum), an image loader (one bitmap on the review screen).

### Phone setup (once) — browser fallback

1. Laptop and phone on the same wifi. `./scripts/dev.sh` on the laptop.
2. Phone → `https://<laptop-ip>:8443/ca.crt` → download.
3. Android: Settings → Security → Encryption & credentials → Install a certificate →
   CA certificate → the downloaded file. (iOS: install profile, then
   Settings → General → About → Certificate Trust Settings → enable.)
4. Open `https://<laptop-ip>:8443`, set the book, "Add to home screen".

> The CA private key sits in `data/certs/ca.key`. It only grants trust to this server on
> this LAN. Delete `data/certs/` after the puja and the phone's trust entry with it.

### Model providers

| `PUJA_VISION_PROVIDER` | What happens |
|---|---|
| `none` (default) | No calls at all. Voice + typing carry the receipt. |
| `openai` | OpenAI-compatible chat with an image, and `/audio/transcriptions` for the clip. Covers OpenAI, Gemini's compat endpoint, Groq, **Ollama** and **whisper.cpp**. |
| `fake` | Fixed answer; used by the test suite and for demos without a key. |

`PUJA_VISION_BASE_URL` is the reading endpoint; `PUJA_STT_BASE_URL` defaults to it and can
point somewhere else (a local Whisper on `:8081` beside a local vision model on `:11434`).
A host on this machine or the LAN needs **no API key**.

Cost note: one 1600 px stub plus a ~200-token answer is roughly 800–1200 image tokens per
receipt on the small OpenAI vision models. At 300 receipts that is cents, not rupees —
but it is the operator's key and the operator's choice, which is why `none` is the default,
and why local models are the zero-cost option.

## Testing

```
server:  53 unit tests + 15 API integration tests (real xlsx copy, fake provider)
app:     vitest helpers, tsc --noEmit, vite build
e2e:     Playwright, real browser: photo → 6 s screen → stop → review → push → row 311
android: ./gradlew assembleDebug  (JDK 17 + Android SDK), installed with adb
```

The Excel tests never touch the live file: they copy it into a temp dir first, and a test
asserts that the fixture path differs from `data/ledger/`.
