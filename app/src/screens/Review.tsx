import { useState } from 'react'
import { ApiError, confirmDraft, editDraft, media, rejectDraft } from '../api'
import { confidenceWord, rupees, sourceLabel } from '../lib/format'
import type { ConfirmResult, Draft } from '../types'

interface Props {
  draft: Draft
  /** called once the row is in the sheet; parent refreshes totals */
  onCommitted: () => void
  /** leave the review screen */
  onDone: () => void
}

export default function Review({ draft, onCommitted, onDone }: Props) {
  const [receipt, setReceipt] = useState(draft.receipt.value ?? '')
  const [name, setName] = useState(draft.name.value ?? '')
  const [amount, setAmount] = useState(draft.amount.value ?? '')
  const [mode, setMode] = useState((draft.mode.value ?? 'CASH').toUpperCase())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [done, setDone] = useState<ConfirmResult | null>(null)
  // never re-send a field the volunteer did not touch: that would wipe its
  // evidence, confidence and alternates on the server
  const changed: Record<string, string> = {}
  if (receipt !== (draft.receipt.value ?? '')) changed.receipt = receipt
  if (name !== (draft.name.value ?? '')) changed.name = name
  if (amount !== (draft.amount.value ?? '')) changed.amount = amount
  if (mode !== (draft.mode.value ?? 'CASH').toUpperCase()) changed.mode = mode
  const valid =
    name.trim().length >= 2 && Number(receipt) >= 1 && Number(receipt) <= 9999 && Number(amount) >= 1

  async function push() {
    setBusy(true)
    setError('')
    try {
      if (Object.keys(changed).length) await editDraft(draft.id, changed)
      const result = await confirmDraft(draft.id)
      setDone(result)
      onCommitted()
    } catch (e) {
      // the push landed but its answer was lost: confirm is idempotent, so just
      // ask again instead of telling the volunteer it failed
      if (e instanceof ApiError && e.status === 400 && e.message.includes('already pushed')) {
        try {
          const result = await confirmDraft(draft.id)
          setDone(result)
          onCommitted()
          return
        } catch {
          /* fall through to the real error */
        }
      }
      setError(e instanceof Error ? e.message : 'Could not push to Excel')
    } finally {
      setBusy(false)
    }
  }

  if (done) {
    return (
      <section className="review">
        <div className="card success">
          <div className="tick">✓</div>
          <h1>Pushed to Excel</h1>
          <p className="big-line">
            Book {done.draft.book} · R/{done.draft.receipt.value} · {done.draft.name.value}
          </p>
          <p className="big-line">{rupees(Number(done.draft.amount.value ?? 0))} · {done.draft.mode.value}</p>
          <p className="muted">Written on ledger row {done.row}</p>
          <div className="totals">
            <div><span>Cash</span><b>{rupees(done.totals.cash)}</b></div>
            <div><span>Online</span><b>{rupees(done.totals.online)}</b></div>
            <div><span>Sheet total</span><b>{rupees(done.totals.total)}</b></div>
          </div>
          <button className="btn primary big" onClick={() => { setDone(null); onDone() }}>
            Next receipt
          </button>
        </div>
      </section>
    )
  }

  return (
    <section className="review">
      <div className="screen-head">
        <h1>Check the stub</h1>
        <p className="muted">Book {draft.book} · fix anything the machine got wrong</p>
      </div>

      {draft.photo_url && (
        <a className="photo-link" href={media(draft.photo_url)} target="_blank" rel="noreferrer">
          <img className="photo" src={media(draft.photo_url)} alt="receipt stub" />
          <span>Tap to zoom</span>
        </a>
      )}

      {draft.warnings.length > 0 && (
        <div className="card warn small">
          {draft.warnings.map((w) => <p key={w}>⚠ {w}</p>)}
        </div>
      )}

      <div className="card form">
        <label className="field">
          <span>Receipt no</span>
          <input inputMode="numeric" value={receipt} onChange={(e) => setReceipt(e.target.value.replace(/[^\d]/g, ''))} />
          <em className={`badge badge-${draft.receipt.source}`}>
            {sourceLabel(draft.receipt.source)} · {confidenceWord(draft.receipt.confidence)}
          </em>
        </label>

        <label className="field">
          <span>Name</span>
          <input value={name} onChange={(e) => setName(e.target.value.toUpperCase())} autoCapitalize="characters" />
          <em className={`badge badge-${draft.name.source}`}>
            {sourceLabel(draft.name.source)} · {confidenceWord(draft.name.confidence)}
          </em>
        </label>

        <label className="field">
          <span>Amount ₹</span>
          <input inputMode="numeric" value={amount} onChange={(e) => setAmount(e.target.value.replace(/[^\d]/g, ''))} />
          <em className={`badge badge-${draft.amount.source}`}>
            {sourceLabel(draft.amount.source)} · {confidenceWord(draft.amount.confidence)}
          </em>
        </label>

        <div className="field">
          <span>Mode</span>
          <div className="toggle">
            <button className={mode === 'CASH' ? 'on' : ''} onClick={() => setMode('CASH')}>Cash</button>
            <button className={mode === 'ONLINE' ? 'on' : ''} onClick={() => setMode('ONLINE')}>Online</button>
          </div>
          <em className={`badge badge-${draft.mode.source}`}>
            {sourceLabel(draft.mode.source)}
          </em>
        </div>
      </div>

      {error && <div className="card error">{error}</div>}

      <div className="row">
        <button
          className="btn"
          disabled={busy}
          onClick={() =>
            void rejectDraft(draft.id)
              .then(onDone)
              .catch((e) => setError(e instanceof Error ? e.message : 'Could not discard'))
          }
        >
          Throw away
        </button>
        <button className="btn primary grow" disabled={busy || !valid} onClick={() => void push()}>
          {busy ? 'Writing to Excel…' : valid ? 'Push to Excel' : 'Fill the fields first'}
        </button>
      </div>

      <details className="card evidence">
        <summary>What the machine saw</summary>
        {draft.transcript && (<><h4>You said</h4><p>{draft.transcript}</p></>)}
        {draft.raw_text && (<><h4>Text on the stub</h4><pre>{draft.raw_text}</pre></>)}
        {draft.vision_note && (<><h4>Vision model note</h4><p>{draft.vision_note}</p></>)}
        <h4>Field evidence</h4>
        <ul>
          {([['receipt', draft.receipt], ['name', draft.name], ['amount', draft.amount], ['mode', draft.mode]] as const).map(
            ([key, f]) => (
              <li key={key}>
                <b>{key}</b>: {(f.evidence.length ? f.evidence.join('; ') : 'no evidence')}
                {f.alternates.length > 0 && <span className="alt"> · other readings: {f.alternates.join(', ')}</span>}
              </li>
            ),
          )}
        </ul>
        {draft.audio_url && <p className="muted small">Voice clip kept: <a href={media(draft.audio_url)}>listen</a></p>}
      </details>
    </section>
  )
}
