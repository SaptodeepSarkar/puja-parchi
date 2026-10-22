import { useEffect, useRef, useState, type CSSProperties } from 'react'
import { capture } from '../api'
import { newCaptureId, shrinkPhoto } from '../lib/photo'
import { VoiceCapture, dictationSupported, type VoiceResult } from '../lib/recorder'
import type { Draft } from '../types'

const SECONDS = 6

type Phase = 'pick' | 'recording' | 'sending' | 'error'

interface Props {
  book: number | null
  recentBooks: number[]
  onBook: (n: number) => void
  onDraft: (d: Draft) => void
}

export default function Capture({ book, recentBooks, onBook, onDraft }: Props) {
  const [photo, setPhoto] = useState<Blob | null>(null)
  const [preview, setPreview] = useState('')
  const [phase, setPhase] = useState<Phase>('pick')
  const [left, setLeft] = useState(SECONDS)
  const [heard, setHeard] = useState('')
  const [note, setNote] = useState('')
  const [error, setError] = useState('')
  const [pickerOpen, setPickerOpen] = useState(false)
  const [bookInput, setBookInput] = useState(String(book ?? ''))
  const [bookError, setBookError] = useState('')

  const capRef = useRef<VoiceCapture | null>(null)
  const timers = useRef<number[]>([])
  const finishing = useRef(false)
  const fileRef = useRef<HTMLInputElement>(null)
  const finishRef = useRef<() => void>(() => undefined)
  // one id per photo, reused on retry so a lost response cannot duplicate the draft
  const clientIdRef = useRef('')
  const countdown = useRef(0)
  // set when the photo is thrown away or the screen unmounts, so a slow
  // permission prompt cannot start a timer or a second upload
  const aborted = useRef(false)
  const previewRef = useRef('')
  const voiceRef = useRef<VoiceResult | null>(null)

  const clearTimers = () => {
    timers.current.forEach((t) => window.clearTimeout(t))
    timers.current = []
    window.clearInterval(countdown.current)
  }
  useEffect(
    () => () => {
      aborted.current = true
      clearTimers()
      void capRef.current?.stop()
      if (previewRef.current) URL.revokeObjectURL(previewRef.current)
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  )

  function showPreview(url: string) {
    if (previewRef.current) URL.revokeObjectURL(previewRef.current)
    previewRef.current = url
    setPreview(url)
  }

  async function pick(file: File | undefined) {
    if (!file) return
    if (!book) {
      setPickerOpen(true)
      return
    }
    const blob = await shrinkPhoto(file)
    aborted.current = false
    clientIdRef.current = newCaptureId()
    voiceRef.current = null
    setPhoto(blob)
    showPreview(URL.createObjectURL(blob))
    setError('')
    setNote('')
    setHeard('')
    setPhase('recording')
    setLeft(SECONDS)

    const cap = new VoiceCapture((t) => setHeard(t))
    capRef.current = cap
    const micOk = await cap.start()
    // the volunteer may have thrown the photo away while the prompt was open
    if (aborted.current) return
    setNote(
      cap.note ||
        (micOk || dictationSupported() ? '' : 'No microphone here. Type the name, receipt and amount on the next screen.'),
    )

    countdown.current = window.setInterval(() => setLeft((l) => Math.max(0, l - 1)), 1000)
    timers.current.push(window.setTimeout(() => finishRef.current(), SECONDS * 1000))
  }

  async function finish() {
    if (finishing.current) return
    finishing.current = true
    clearTimers()
    const res = await capRef.current?.stop()
    voiceRef.current = res ?? null
    setPhase('sending')
    try {
      const draft = await capture({
        image: photo as Blob,
        book: book as number,
        transcript: res?.transcript ?? '',
        audio: res?.audio ?? null,
        clientId: clientIdRef.current,
      })
      onDraft(draft)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not send the capture')
      setPhase('error')
    } finally {
      finishing.current = false
    }
  }
  finishRef.current = finish

  function reset() {
    aborted.current = true
    clearTimers()
    void capRef.current?.stop()
    capRef.current = null
    voiceRef.current = null
    showPreview('')
    clientIdRef.current = ''
    setPhoto(null)
    setHeard('')
    setNote('')
    setError('')
    setLeft(SECONDS)
    setPhase('pick')
  }

  return (
    <section className="capture">
      <div className="screen-head">
        <h1>New receipt</h1>
        <p className="muted">
          Book {book ?? 'not set'} · stays the same until you change it
        </p>
      </div>

      {!book && (
        <div className="card warn">
          <strong>Set the book number first.</strong>
          <p>Every photo goes into that book's folder and ledger column.</p>
          <button className="btn primary" onClick={() => setPickerOpen(true)}>Choose book</button>
        </div>
      )}

      {phase === 'pick' && book && (
        <div className="capture-stage">
          <button
            className="shutter"
            onClick={() => fileRef.current?.click()}
            aria-label="Photograph the receipt"
          >
            <span className="shutter-dot" />
          </button>
          <p className="shutter-hint">Tap to photograph the stub</p>
          <div className="say-script">
            <p className="say-title">Then say it in this order — 6 seconds</p>
            <ol>
              <li>
                <b>1</b> Name <span>“Sudha Dairy”</span>
              </li>
              <li>
                <b>2</b> Receipt number <span>“nine five zero one”</span>
              </li>
              <li>
                <b>3</b> Cash or online <span>“cash”</span>
              </li>
              <li>
                <b>4</b> Amount <span>“eleven hundred”</span>
              </li>
            </ol>
            <p className="say-foot">Four things, one after another, in one breath.</p>
          </div>
          {note && <p className="note">{note}</p>}
        </div>
      )}

      {phase === 'recording' && (
        <div className="card recording">
          <div className="rec-top">
            {preview && <img className="thumb" src={preview} alt="receipt" />}
            <div className={`dial ${left <= 2 ? 'hot' : ''}`} style={{ '--p': `${(left / SECONDS) * 100}%` } as CSSProperties}>
              <span className="dial-num">{left}</span>
              <span className="dial-sub">sec</span>
            </div>
          </div>
          <p className="speak-order">
            <b>1 Name</b> · <b>2 Receipt no</b> · <b>3 Cash/Online</b> · <b>4 Amount</b>
          </p>
          <p className={heard ? 'heard' : 'heard empty'}>{heard || 'Listening…'}</p>
          {note && <p className="note">{note}</p>}
          <button className="btn primary big" onClick={() => finishRef.current()}>
            Stop &amp; use this
          </button>
        </div>
      )}

      {phase === 'sending' && (
        <div className="card center">
          <div className="spinner" />
          <p>Reading the stub and saving the photo…</p>
          {note && <p className="note">{note}</p>}
          {preview && <img className="thumb wide" src={preview} alt="receipt" />}
        </div>
      )}

      {phase === 'error' && (
        <div className="card warn">
          <strong>Could not send</strong>
          <p>{error}</p>
          <div className="row">
            <button className="btn" onClick={reset}>Retake</button>
            <button
              className="btn primary"
              onClick={async () => {
                setPhase('sending')
                try {
                  const voice = voiceRef.current
                  const draft = await capture({
                    image: photo as Blob,
                    book: book as number,
                    transcript: voice?.transcript ?? heard,
                    audio: voice?.audio ?? null,
                    clientId: clientIdRef.current,
                  })
                  onDraft(draft)
                } catch (e) {
                  setError(e instanceof Error ? e.message : 'Still failing')
                  setPhase('error')
                }
              }}
            >
              Try again
            </button>
          </div>
        </div>
      )}

      <input
        ref={fileRef}
        type="file"
        accept="image/*"
        capture="environment"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          void pick(file)
        }}
      />

      {pickerOpen && (
        <div className="sheet-back" onClick={() => setPickerOpen(false)}>
          <div className="sheet" onClick={(e) => e.stopPropagation()}>
            <h2>Receipt book</h2>
            <p className="muted small">Pick the book you are holding. It stays set.</p>
            <div className="chips">
              {recentBooks.map((n) => (
                <button
                  key={n}
                  className={n === book ? 'chip on' : 'chip'}
                  onClick={() => {
                    onBook(n)
                    setPickerOpen(false)
                  }}
                >
                  {n}
                </button>
              ))}
            </div>
            <form
              className="row"
              onSubmit={(e) => {
                e.preventDefault()
                const n = Number(bookInput)
                if (n >= 1 && n <= 999) {
                  onBook(n)
                  setBookError('')
                  setPickerOpen(false)
                } else {
                  setBookError('Book number must be 1-999')
                }
              }}
            >
              <input
                className="input"
                inputMode="numeric"
                aria-label="Book number"
                placeholder="Book no"
                value={bookInput}
                onChange={(e) => setBookInput(e.target.value.replace(/\D/g, ''))}
              />
              <button className="btn primary" type="submit">Save</button>
            </form>
            {bookError && <p className="note">{bookError}</p>}
          </div>
        </div>
      )}
    </section>
  )
}
