import { useEffect, useState } from 'react'
import { api } from '../api'
import { rupees } from '../lib/format'
import type { Health, Stats } from '../types'

interface Props {
  book: number | null
  health: Health | null
  stats: Stats | null
  onBook: (n: number) => void
  onToast: (msg: string) => void
  onRecheck: () => void
}

export default function Settings({ book, health, stats, onBook, onToast, onRecheck }: Props) {
  const [bookInput, setBookInput] = useState(String(book ?? ''))
  const [base, setBase] = useState(api.base())
  const [pin, setPin] = useState(api.pin())
  const [installable, setInstallable] = useState<any>(null)

  useEffect(() => {
    const handler = (e: Event) => {
      e.preventDefault()
      setInstallable(e)
    }
    window.addEventListener('beforeinstallprompt', handler)
    return () => window.removeEventListener('beforeinstallprompt', handler)
  }, [])

  return (
    <section>
      <div className="screen-head">
        <h1>Setup</h1>
        <p className="muted">Set once on this phone</p>
      </div>

      <div className="card form">
        <label className="field">
          <span>Receipt book</span>
          <input
            inputMode="numeric"
            value={bookInput}
            onChange={(e) => setBookInput(e.target.value.replace(/\D/g, ''))}
          />
          <em className="badge badge-manual">used for every capture until changed</em>
        </label>
        <button
          className="btn primary"
          onClick={() => {
            const n = Number(bookInput)
            if (n >= 1 && n <= 999) {
              onBook(n)
              onToast(`Book ${n} set`)
            } else {
              onToast('Book number must be 1-999')
            }
          }}
        >
          Save book number
        </button>
      </div>

      <div className="card form">
        <label className="field">
          <span>Server address</span>
          <input
            value={base}
            placeholder="same as the page (recommended)"
            onChange={(e) => setBase(e.target.value)}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </label>
        <label className="field">
          <span>PIN (only if the server asks)</span>
          <input value={pin} onChange={(e) => setPin(e.target.value)} inputMode="numeric" />
        </label>
        <button
          className="btn"
          onClick={() => {
            api.setBase(base)
            api.setPin(pin)
            onToast('Saved')
            setTimeout(() => window.location.reload(), 400)
          }}
        >
          Save connection
        </button>
      </div>

      <div className="card">
        <h3>Server</h3>
        {health ? (
          <ul className="kv">
            <li><span>Status</span><b>{health.ok ? 'online' : 'offline'}</b></li>
            <li><span>Vision model</span><b>{health.vision}</b></li>
            <li><span>Dictation</span><b>{health.stt}</b></li>
            <li><span>Ledger file</span><b>{health.ledger_ok ? 'found' : 'MISSING'}</b></li>
            <li><span>Writing to</span><b className="path">{health.ledger_path}</b></li>
            <li><span>Version</span><b>{health.version}</b></li>
          </ul>
        ) : (
          <p className="muted">Not connected.</p>
        )}
        {stats && <p className="muted small">Whole sheet (all books), cash + online: {rupees(stats.ledger_total)}</p>}
        <button className="btn small-btn" onClick={onRecheck}>Check again</button>
      </div>

      {installable && (
        <button
          className="btn primary"
          onClick={async () => {
            installable.prompt()
            const choice = await installable.userChoice
            onToast(choice?.outcome === 'accepted' ? 'Installed' : 'Not installed')
            setInstallable(null)
          }}
        >
          Install on this phone
        </button>
      )}

      <button
        className="btn danger small-btn"
        onClick={() => {
          if (!confirm('Forget the book number, PIN and server address on this phone?')) return
          localStorage.clear()
          // without this the app re-adopts the server's sticky book on reload
          sessionStorage.setItem('puja.forgot', '1')
          window.location.reload()
        }}
      >
        Forget this phone's settings
      </button>

      <p className="muted small center-text">
        Photos are saved on the server under data/photos/&lt;book&gt;/. Nothing is sent to the cloud except the
        photo and voice clip you capture, and only if a vision model is configured.
      </p>
    </section>
  )
}
