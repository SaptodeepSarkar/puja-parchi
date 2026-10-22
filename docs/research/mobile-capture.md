# Mobile capture: phone camera + 6 s voice note

Scope: volunteer on an Android phone, app served from a Rust process on a laptop on the same wifi.
No Android SDK → the client is a PWA (installable web app). iOS Safari notes given where they differ.

## 1. Secure context: what actually works

`http://192.168.x.x` is **not** a secure context; only `http://localhost` / `127.0.0.1` are treated as trustworthy. `https://` with a cert error is still "https by scheme", but Chrome refuses service-worker registration on it and does not offer install.

| API we need | `http://192.168.x.x` | self-signed + "Proceed" | cert trusted by phone |
|---|---|---|---|
| `<input type=file accept="image/*" capture>` | works | works | works |
| `navigator.mediaDevices.getUserMedia` (mic/cam) | **fails** — `navigator.mediaDevices` is `undefined` on insecure origins, else `NotAllowedError` | unreliable (cert-error origin) | works |
| `MediaRecorder` | API exists, useless: no stream to record | same | works |
| `SpeechRecognition` / `webkitSpeechRecognition` | constructor exists, but it needs the mic permission Chrome will not grant on an insecure origin → `not-allowed` (exact Android error: OPEN) | unreliable | works, but see §3 |
| Service worker (`register()`) | blocked (not a secure context) | **blocked**: `Failed to register a ServiceWorker: An SSL certificate error occurred when fetching the script` | works |
| PWA install / "Add to home screen" as app | no | no install prompt | works |

PWA installability is stricter than secure context: it needs real HTTPS (or localhost), not just a "potentially trustworthy" scheme.

**DECISION:** treat a phone-trusted HTTPS cert as a hard prerequisite for voice and install, and ship a degraded photo-only mode that still works over plain HTTP if TLS setup fails. Feature-detect (`'mediaDevices' in navigator`, `window.SpeechRecognition || window.webkitSpeechRecognition`) and never let a missing API break the receipt form.

## 2. TLS: is self-signed click-through the right call?

No. Tapping through the warning gives you a page where the service worker cannot register and the app cannot be installed; Chrome also re-shows the interstitial per profile/session and the camera/mic permission UI is unreliable on cert-error origins. HSTS is not the problem for an IP literal (IPs cannot be HSTS-preloaded), but never put a self-signed cert on a domain that is HSTS-preloaded — Chrome then refuses the bypass entirely.

Options, cheapest reliable first:

1. **Local CA installed on the phone once** — generate a CA + leaf with the LAN IP in `subjectAltName` (mkcert, or `rcgen` in Rust), serve on `https://192.168.x.x:8443`, download `ca.crt` over HTTP once, install via Settings → Security → Encryption & credentials → Install a certificate → CA certificate. Chrome on Android trusts user CAs for browsing. One-time, offline, no domain needed. Same origin afterwards → the mic permission grant persists.
2. **Real domain + Let's Encrypt (DNS-01)** pointing at the LAN IP, if you own a domain. Zero phone setup, but needs DNS control and a renewal path.
3. **Tunnel** (cloudflared/ngrok) for a valid public HTTPS URL. Simplest to set up, but depends on internet during the event and adds latency.
4. Plain HTTP + photo only. Not acceptable as the main path.

**DECISION:** local CA (mkcert or `rcgen`) with IP SAN, leaf valid for the event, served by rustls on `:8443`; phone installs the CA once. Fall back to a tunnel only if it is already working. Keep the same IP:port for the whole event so permissions and install survive.

## 3. Speech capture

Web Speech in 2026: still shipping on Android Chrome, still **cloud-backed by default** — MDN states Chrome sends audio to a web service and it does not work offline. The on-device variant ("On-device Web Speech API") is behind a flag (Chrome 139 dev trial), so do not count on it. Practical behaviour on Android: `continuous` is effectively single-shot, recognition auto-stops on a short silence and fires `onend`, interim results work, `lang = 'hi-IN'` works, permission prompt appears on first `start()`. Accuracy on names and spoken digits is poor (numbers come back as words, Devanagari vs Latin is inconsistent).

MediaRecorder + server Whisper is deterministic: exact 6 s of audio, one consistent transcript, works with no internet (laptop-local), keeps the audio for human audit, and `initial_prompt` can bias toward "receipt number, amount, cash, UPI, cheque, name". 6 s of mono Opus at 32 kbps is ~24 KB; `faster-whisper small` int8 transcribes it in ~1–3 s on a laptop CPU.

**DECISION:** primary = `MediaRecorder` → `POST /api/transcribe` (OpenAI-compatible `/audio/transcriptions` shape, `faster-whisper`/`whisper.cpp`, `language` hint + `initial_prompt`), prefill editable fields, store the blob on the receipt. Fallback = Web Speech for an instant draft, then manual keypad. Saving never waits on transcription.

```js
const stream = await navigator.mediaDevices.getUserMedia({
  audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true }
});
const mime = MediaRecorder.isTypeSupported('audio/webm;codecs=opus') ? 'audio/webm;codecs=opus' : '';
const rec = new MediaRecorder(stream, mime ? { mimeType: mime, audioBitsPerSecond: 32000 } : undefined);
const chunks = [];
rec.ondataavailable = e => e.data.size && chunks.push(e.data);
const stopped = new Promise(r => (rec.onstop = r));
rec.start(250);                                  // timeslice keeps memory small
const hard = setTimeout(() => rec.state === 'recording' && rec.stop(), 6000);
// stop button → rec.stop();  then:
await stopped; clearTimeout(hard);
stream.getTracks().forEach(t => t.stop());       // release the mic (red dot / battery)
const blob = new Blob(chunks, { type: rec.mimeType });
```

Order matters: `getUserMedia` first (permission prompt), timer starts only after `rec.start()`, and cap the whole flow (~10 s) so a slow permission grant cannot silently record nothing. On iOS Safari the mime is `audio/mp4`; accept both server-side. Web Speech sequence is `start()` → `onresult` (interim then final in `event.results`) → `stop()` at 6 s → wait for `onend` (final result can arrive after `stop()`); wrap in an 8 s hard timeout because `onend` can hang on network errors.

## 4. Camera

`<input type="file" accept="image/*" capture="environment">` opens the native camera app: one tap, no in-page camera permission, works on plain HTTP, best still quality and autofocus/HDR. Costs: the page is backgrounded while the camera app is open (Android may reload a memory-starved tab — keep form state in `sessionStorage`), returns a full-res 3–5 MB JPEG with EXIF orientation, and capture can fail on very low storage.

`getUserMedia` live preview needs HTTPS + camera permission, keeps the user in-page (faster repeat shots) but stills are lower quality unless you go through `ImageCapture.takePhoto()` (not universally supported), and it is more code for no accuracy gain on a paper stub.

HEIC: Android camera apps default to JPEG; some OEM "high efficiency" modes write HEIF, which Chrome may not be able to decode into a canvas (OPEN per device). Server should accept JPEG/PNG/WebP, reject anything else with a "retake with standard photo mode" message.

Orientation and size: EXIF is not normalized by the file input. Client canvas normalizes it (`createImageBitmap(file, { imageOrientation: 'from-image' })`), but the server must also apply EXIF rotation for raw uploads, strip EXIF afterwards, downscale to 1600 px long edge, re-encode JPEG q85, and cap the request body (10 MB) and stored file (2 MB).

**DECISION:** use `<input type="file" accept="image/*" capture="environment">`; client downscales to 1600 px/q85 with a raw-upload fallback; server applies EXIF orientation, downscales, strips metadata, and rejects unsupported formats.

## 5. PWA installability checklist (2026)

- **HTTPS with a trusted cert** (localhost does not help the phone). This is the only hard gate that commonly bites.
- **Manifest** linked from every page: `name` + `short_name`, `start_url`, `scope`, `id`, `display: "standalone"`, `theme_color`, `background_color`, `lang: "hi-IN"`, `orientation: "portrait"`, `prefer_related_applications: false`.
- **Icons**: at least 192×192 and 512×512 PNG, plus a `purpose: "maskable"` variant (safe zone 40%).
- **Service worker**: no longer required for installability in Chromium (MDN now says so explicitly), but keep a minimal one for the offline shell and fast reload; never make correctness depend on it.
- **Install UX**: Chrome Android fires `beforeinstallprompt` after the engagement heuristic (~30 s + a gesture). Stash the event, show your own "Install app" button, call `prompt()` on tap. If the event never fires, the menu item "Add to home screen" still works (as a shortcut if criteria are not met).
- **iOS Safari**: no `beforeinstallprompt`; users go Share → Add to Home Screen. Manifest/`display: standalone` are honored; service worker needed for offline.

**DECISION:** ship manifest + 192/512/maskable icons + `display: standalone` + a ~20-line service worker, with a custom Install button driven by `beforeinstallprompt`; installation is optional and the URL must be fully usable uninstalled.

## 6. Battery and data

A 3–5 MB photo over wifi is ~1–2 s at 20 Mbps, but up to ~15 s on a congested pandal access point, and 12 MP JPEG decode on the laptop costs ~0.1–0.3 s plus 5 MB per receipt on disk. Client downscale to 1600 px/q85 lands at ~250–450 KB (≈10× smaller) for ~50 ms of canvas work and negligible battery; it also makes retries cheap when the signal drops. Six seconds of Opus audio is 25–40 KB, irrelevant.

**DECISION:** downscale on the client before upload (canvas → `toBlob('image/jpeg', 0.85)`, max 1600 px), fall back to the original if decoding fails, and enforce a 10 MB request cap / 2 MB stored cap server-side.

## Sources

- Secure contexts & restricted features: https://developer.mozilla.org/en-US/docs/Web/Security/Secure_Contexts/Features_restricted_to_secure_contexts
- `getUserMedia` (secure context, `mediaDevices` undefined on HTTP): https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia
- `ServiceWorker` (secure context): https://developer.mozilla.org/en-US/docs/Web/API/ServiceWorker
- `SpeechRecognition` (Chrome server-based, no offline): https://developer.mozilla.org/en-US/docs/Web/API/SpeechRecognition
- `SpeechRecognition.continuous`: https://developer.mozilla.org/en-US/docs/Web/API/SpeechRecognition/continuous
- On-device Web Speech API, behind a flag: https://chromestatus.com/feature/6090916291674112
- Making PWAs installable (2026 criteria, HTTPS, SW not required): https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Making_PWAs_installable
- Chrome install criteria: https://web.dev/articles/install-criteria
- `capture` attribute: https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Attributes/capture
- Self-signed cert blocks service worker registration: https://stackoverflow.com/questions/38728176/can-you-use-a-service-worker-with-a-self-signed-certificate
- PWA install with self-signed/expired cert (Chromium issue): https://issues.chromium.org/issues/41494880
