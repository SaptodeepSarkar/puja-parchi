/**
 * One 6-second voice grab: microphone audio for the server, plus live dictation
 * text so the volunteer can see what was heard before it is sent.
 *
 * Dictation needs a secure origin (https or localhost). On plain http over the
 * LAN the mic is blocked, so the caller must offer the type-it fallback.
 */

type SpeechRecognitionLike = {
  lang: string
  continuous: boolean
  interimResults: boolean
  start: () => void
  stop: () => void
  abort: () => void
  onresult: ((ev: any) => void) | null
  onerror: ((ev: any) => void) | null
  onend: (() => void) | null
}

function recognitionCtor(): (new () => SpeechRecognitionLike) | undefined {
  const w = window as unknown as Record<string, unknown>
  return (w.SpeechRecognition ?? w.webkitSpeechRecognition) as (new () => SpeechRecognitionLike) | undefined
}

export const dictationSupported = () => Boolean(recognitionCtor())

function pickMime(): string {
  if (typeof MediaRecorder === 'undefined') return ''
  const options = ['audio/webm;codecs=opus', 'audio/webm', 'audio/mp4']
  return options.find((t) => MediaRecorder.isTypeSupported(t)) ?? ''
}

export interface VoiceResult {
  audio: Blob | null
  transcript: string
  note: string
}

export class VoiceCapture {
  transcript = ''
  audio: Blob | null = null
  note = ''
  private stream?: MediaStream
  private rec?: MediaRecorder
  private chunks: Blob[] = []
  private recog?: SpeechRecognitionLike
  private finalText = ''
  private stopped = false
  private recogBroken = false

  constructor(private readonly onText: (text: string) => void) {}

  /** Returns false when no microphone could be opened, but dictation may still work. */
  async start(): Promise<boolean> {
    let mic = false
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      // stop() may have run while the permission prompt was open; never leave the mic hot
      if (this.stopped) {
        stream.getTracks().forEach((t) => t.stop())
        return false
      }
      this.stream = stream
      const mime = pickMime()
      this.rec = new MediaRecorder(this.stream, mime ? { mimeType: mime } : undefined)
      this.rec.ondataavailable = (e) => {
        if (e.data.size > 0) this.chunks.push(e.data)
      }
      this.rec.start()
      mic = true
    } catch {
      this.note = 'Microphone is blocked. It needs https and mic permission.'
    }
    this.startDictation()
    return mic
  }

  private startDictation() {
    if (this.stopped) return
    const Ctor = recognitionCtor()
    if (!Ctor) {
      if (!this.note) this.note = 'Live dictation is not supported here; type the details instead.'
      return
    }
    try {
      const r = new Ctor()
      r.lang = 'en-IN'
      r.continuous = true
      r.interimResults = true
      r.onresult = (ev: any) => {
        let interim = ''
        for (let i = ev.resultIndex; i < ev.results.length; i++) {
          const res = ev.results[i]
          if (res.isFinal) this.finalText += `${res[0].transcript} `
          else interim += res[0].transcript
        }
        this.transcript = `${this.finalText}${interim}`.trim()
        this.onText(this.transcript)
      }
      r.onerror = (ev: any) => {
        const kind = String(ev?.error ?? '')
        if (kind === 'not-allowed' || kind === 'service-not-allowed') {
          this.note = 'Dictation permission denied; type the details instead.'
          this.recogBroken = true
        } else if (kind === 'no-speech') {
          this.note = 'Nothing heard. You can type the details.'
          this.recogBroken = true
        } else if (kind === 'network') {
          this.note = 'Dictation needs internet; type the details if it stays quiet.'
          this.recogBroken = true
        }
      }
      // Chrome ends the session after a silence; keep listening until we stop.
      r.onend = () => {
        if (this.stopped || this.recogBroken) return
        try {
          r.start()
        } catch {
          /* already restarted */
        }
      }
      this.recog = r
      r.start()
    } catch {
      this.note = 'Could not start dictation; type the details instead.'
      this.recogBroken = true
    }
  }

  async stop(): Promise<VoiceResult> {
    this.stopped = true
    try {
      this.recog?.stop()
    } catch {
      /* not running */
    }
    const rec = this.rec
    if (rec && rec.state !== 'inactive') {
      await Promise.race([
        new Promise<void>((resolve) => {
          rec.onstop = () => resolve()
          rec.stop()
        }),
        new Promise<void>((resolve) => setTimeout(resolve, 800)),
      ])
    }
    this.stream?.getTracks().forEach((t) => t.stop())
    if (this.chunks.length > 0) {
      this.audio = new Blob(this.chunks, { type: rec?.mimeType || 'audio/webm' })
    }
    // Interim words are real words: the amount is often still "interim" when the
    // 6 s dial runs out, and dropping it would block the push.
    return { audio: this.audio, transcript: (this.transcript || this.finalText).trim(), note: this.note }
  }
}
