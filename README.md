# Puja Parchi

Photograph a receipt stub, speak for six seconds, check four fields, push one clean row
into the Durga Puja ledger. Rust server + installable phone app. Runs on one laptop, on
the pandal wifi, with no cloud account required.

```
phone (PWA)  ──https──►  Rust server  ──►  Durga Puja 2026.xlsx
                              │
                              ├──► data/photos/<book>/…
                              ├──► data/audio/<book>/…
                              └──► data/puja.db  (drafts + sticky book)
```

## Quick start (laptop)

```bash
cd durga-puja-2026
./scripts/dev.sh
```

First run installs the app build, starts the server on `:8080` (http) and `:8443` (https),
and prints the URLs to use on the phone.

Defaults: writes to `data/ledger/Durga Puja 2026.xlsx`. To write your live book instead,
set `PUJA_LEDGER_PATH` (see `.env.example`).

`dev.sh` starts the reading and speech models too, so starting the server also means the
models are loaded. Set `PUJA_MODELS=off` to skip that (for a cloud provider).

## Local models (no cloud account)

Both models run on the laptop, so the stub and the voice never leave the building.

```bash
# vision - ollama (already installed for this laptop under ~/.local)
~/.local/ollama/bin/ollama serve &

# speech - whisper.cpp server, 16 kHz audio in, text out
~/.local/src/whisper.cpp/build/bin/whisper-server \
  -m ~/.local/src/whisper.cpp/models/ggml-small.bin \
  --host 127.0.0.1 --port 8081 --convert \
  --inference-path /v1/audio/transcriptions -t 8
```

`./scripts/dev.sh` does the above for you and skips whatever is already running:

```bash
./scripts/models.sh          # start anything not already up (and keep the model warm)
./scripts/models.sh status   # what is answering right now
./scripts/models.sh stop     # stop the ones this script started
```

The server also probes both endpoints at boot and records the result in the monitor, and
`/api/health` reports `vision_ready` / `stt_ready`, so "configured but not running" is
visible on the Setup screen and in the console instead of being discovered at the first
capture.

### Which model, and why (measured on this laptop)

| | Model where | RAM | Time per capture |
|---|---|---|---|
| Vision | `gemma3:4b` on the RTX 3050 (100% GPU) | ~1.1 GB host | ~5 s warm |
| Speech | `ggml-small.bin` on CPU, 8 threads | ~0.55 GB | ~3 s for a 6 s clip |

Speech is deliberately **small**, not `large-v3-turbo`: on this CPU-only whisper build
turbo costs **1.7 GB and ~17 s** for a 5.5 s clip, small costs **0.55 GB and ~3 s**, and on
the real clip both produce the same words ("Atharvasingh 866" vs "Atherv Singh 866") whose
spelling the name gazetteer repairs anyway. Turbo is 3× slower than realtime — no use at a
pandal desk. Set `WHISPER_MODEL=…/ggml-large-v3-turbo.bin` if you ever want it.

Together the two models want about **1.7 GB of RAM**. On a 15 GB laptop with a browser
open, running out is the one thing that makes the whole desk feel broken, so `models.sh`
warns before it starts if less than 1.5 GB is free. The vision model is released again
after `PUJA_KEEP_ALIVE` (default `15m`) with no captures, which hands back its 1.1 GB and
2.9 GB of VRAM. If you are still tight: `pkill -f GradleDaemon` frees the ~1 GB an old
Android build leaves behind.

## Phone app (Kotlin)

The phone app is native Android, in `android/`. No browser, no certificate: an app is
allowed to use the microphone over plain http, so it talks straight to
`http://<laptop-ip>:8080`.

```bash
cd android
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

First run: type the laptop address (for example `http://192.168.0.110:8080`), the PIN if
the server has one, and tap **Save and test** — it reports the vision model, the speech
model and whether the ledger was found. Then set the receipt book number, which stays
fixed until you change it.

Three things it does that the browser could not:

- **One microphone owner.** It records 16 kHz mono WAV and the *server* transcribes it,
  so there is no second dictation engine to fail quietly.
- **The speaking order is on screen before the clock starts** — 1 name, 2 receipt number,
  3 cash or online, 4 amount — each with an example. Nobody has to guess.
- **One upload, one wait**: photo plus clip go in a single request and the review screen
  opens with what the vision model read and what was heard.

## Phone (browser fallback)

The same server still serves the installable web app; it is the fallback if the phone
cannot run the APK.

1. Same wifi as the laptop.
2. `https://<laptop-ip>:8443/ca.crt` → download the certificate.
3. Android: Settings → Security → Encryption & credentials → Install a certificate →
   **CA certificate** → pick the download. (iOS: install the profile, then enable it under
   Settings → General → About → Certificate Trust Settings.)
4. Open `https://<laptop-ip>:8443` → browser menu → **Add to home screen**.
5. In Setup: set the receipt book number. It stays set until you change it.

> The mic only works over HTTPS with a trusted certificate — that is a browser rule, not
> this app's choice. Without step 2 the app still photographs and lets you type.

## Screens

| Capture | 6-second voice | Review | Review (bottom) | Pushed |
|---|---|---|---|---|
| ![capture](docs/screenshots/1-capture.png) | ![voice](docs/screenshots/2-voice.png) | ![review](docs/screenshots/3-review.png) | ![review bottom](docs/screenshots/3b-review-bottom.png) | ![done](docs/screenshots/4-done.png) |

## Using it

1. **Capture** — tap the big shutter, photograph the stub.
2. **Speak** — you get 6 seconds, or tap *Stop & use this* early: *name · receipt no ·
   cash/online · amount*.
3. **Check** — receipt, name, amount, mode are pre-filled with badges showing where each
   value came from. Warnings show any disagreement. Fix anything; edits are final.
4. **Push** — writes one row to the bottom of `CASH `, shows the row number and totals.

Nothing is written to Excel until you tap Push. Photos and voice clips are saved before
any model sees them.

## Server monitor (laptop)

A second, read-only console for the laptop — not the phone app:

```
http://127.0.0.1:8080/monitor          # same machine
https://<laptop-ip>:8443/monitor       # from anywhere on the wifi
```

| Console | Row added (pop-up) | Model call in flight |
|---|---|---|
| ![monitor](docs/screenshots/5-server-monitor.png) | ![row added](docs/screenshots/6-monitor-row-added.png) | ![vision running](docs/screenshots/7-monitor-vision-running.png) |

- **Live** — the model call happening right now (*Vision · openai/gpt-4o-mini · reading the
  photo*) with a running clock, so you can see the server working a capture.
- **Added to the ledger** — the last ten committed rows, each with a link to its stub photo.
- **A pop-up** the moment a row lands in Excel: name, receipt, mode, amount, row number and
  the new book total.
- **Event log** — every stage of every capture since the server started, newest first, last
  300 lines: capture → vision → speech → merge → draft → ledger; failures in amber and red.
- **Models** — the provider and model actually in use, the endpoint, whether an API key is set.
- **Ledger, counters, server** — file, sheet, rows, cash/online/total, throughput, average
  model latency, version, pid, listeners, PIN state.

It uses the same PIN as the phone (`PUJA_PIN`); the page asks once and remembers it, or take
`/monitor?pin=1234`. With no PIN set it opens directly — the page itself holds no data, so it
loads before the PIN is typed and everything it shows comes from the guarded API behind it.

The console is in-memory on purpose: it reports what happened since this server started. The
workbook and `data/puja.db` stay the record.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `PUJA_DATA_DIR` | `data` | photos, audio, db, certs |
| `PUJA_LEDGER_PATH` | `data/ledger/Durga Puja 2026.xlsx` | the workbook to append to |
| `PUJA_LEDGER_SHEET` | `CASH ` | sheet name (trailing space is real) |
| `PUJA_ADDR` | `0.0.0.0:8080` | http listener |
| `PUJA_TLS_ADDR` | `0.0.0.0:8443` | https listener |
| `PUJA_TLS` | `on` | `off` disables https (phone mic then won't work) |
| `PUJA_PIN` | unset | if set, the app demands this PIN (header `x-pin`, or `?pin=` for photos) |
| `PUJA_VISION_PROVIDER` | `none` | `none` · `openai` · `fake` |
| `PUJA_VISION_BASE_URL` | `https://api.openai.com/v1` | any OpenAI-compatible endpoint; the local ollama is `http://127.0.0.1:11434/v1` |
| `PUJA_VISION_API_KEY` | unset | provider key |
| `PUJA_VISION_MODEL` | `gpt-4o-mini` | vision model id (local: `gemma3:4b`) |
| `PUJA_STT_BASE_URL` | same as vision | speech endpoint, when the transcription server is elsewhere (local: `http://127.0.0.1:8081/v1`) |
| `PUJA_STT_API_KEY` | same as vision | key for the speech endpoint |
| `PUJA_STT_MODEL` | `whisper-1` | used only when the browser cannot dictate; whisper.cpp ignores the value |
| `PUJA_STT_LANGUAGE` | blank (auto) | force a language. Do **not** set `hi`: Whisper then answers in Devanagari, which the number parser cannot read |
| `PUJA_MODELS` | `on` | `off` stops `dev.sh` from starting the local models |
| `PUJA_KEEP_ALIVE` | `15m` | how long ollama holds the vision model after the last capture; `0` unloads it immediately |
| `OLLAMA_BIN`, `OLLAMA_MODELS`, `WHISPER_BIN`, `WHISPER_MODEL`, `WHISPER_THREADS` | `~/.local/...` | where `scripts/models.sh` finds the local model servers |

A model on this machine needs **no key**: `localhost`, `127.0.0.1` and the private ranges
(`192.168.…`, `10.…`, `172.…`) are accepted without `PUJA_*_API_KEY`.

## Test

```bash
./scripts/test.sh          # 69 server tests, app typecheck/unit/build, browser e2e
```

The Excel tests copy the real workbook into a temp dir; they never touch your ledger.

## Safety

- Every write: validate → fill the pre-printed line for that book+receipt if the sheet
  already has one, else append under the last row → verify in memory → backup
  (`data/ledger/backups/`, newest 10) → temp file + fsync → rename.
- Pushing the same draft twice writes one row. A receipt number that is already
  completed in the book is refused, with the row that holds it.
- The app never edits or deletes existing rows. Excel stays the master.
- Photos and names are readable only with the PIN when `PUJA_PIN` is set; without it,
  anyone on the wifi can read them. Keep the laptop on a network you trust.
- Close the workbook in Excel while collecting, so your editor does not overwrite the
  server's append (the server re-serialises the whole file on every push).

## What this is not

Not an APK (no Android SDK here — it is an installable web app instead), not multi-user,
not an accounting system, and not a handwriting OCR replacement. The volunteer's voice is
the primary source because OCR on these stubs is genuinely weak; the vision model is a
second opinion, and every field shows its evidence.

Voice is the one part that can need the internet: spoken words are transcribed by
Chrome's own dictation (online) or by the provider you configure. With neither, the
6-second clip is still saved and you type the four fields — photo, review and push keep
working with no network at all.

Docs: [PRD](docs/PRD.md) · [flow](docs/FLOW.md) · [stack](docs/STACK.md) ·
[16 phases](docs/PHASES.md) · [mobile research](docs/research/mobile-capture.md)
