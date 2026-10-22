import { useEffect, useState } from 'react'
import { recent, stats } from '../api'
import { rupees, shortTime } from '../lib/format'
import type { Draft, Stats } from '../types'

export default function Recent({ onOpen }: { onOpen: (d: Draft) => void }) {
  const [items, setItems] = useState<Draft[] | null>(null)
  const [info, setInfo] = useState<Stats | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    recent(50)
      .then(setItems)
      .catch((e) => setError(e instanceof Error ? e.message : 'Could not load'))
    stats().then(setInfo).catch(() => undefined)
  }, [])

  const pending = items?.filter((d) => d.status === 'pending') ?? []
  const done = items?.filter((d) => d.status === 'committed') ?? []

  return (
    <section>
      <div className="screen-head">
        <h1>Today</h1>
        <p className="muted">Pushed from this phone</p>
      </div>

      {info && (
        <div className="card totals">
          <div><span>Cash</span><b>{rupees(info.today_cash)}</b></div>
          <div><span>Online</span><b>{rupees(info.today_online)}</b></div>
          <div><span>Today</span><b>{rupees(info.today_total)}</b></div>
        </div>
      )}

      {info && (
        <p className="muted small ledger-line">
          Ledger: {info.ledger_ok ? `${info.ledger_row} rows` : 'file missing'} · cash {rupees(info.ledger_cash)} ·
          online {rupees(info.ledger_online)}
        </p>
      )}

      {error && <div className="card error">{error}</div>}
      {items === null && !error && <p className="muted">Loading…</p>}

      {pending.length > 0 && (
        <>
          <h2 className="section-title">Waiting to be pushed</h2>
          {pending.map((d) => (
            <button key={d.id} className="list-card pending" onClick={() => onOpen(d)}>
              <span className="list-main">
                <b>R/{d.receipt.value ?? '—'}</b> {d.name.value ?? 'no name'}
              </span>
              <span className="list-side">{rupees(Number(d.amount.value ?? 0))}</span>
            </button>
          ))}
        </>
      )}

      <h2 className="section-title">Pushed</h2>
      {done.length === 0 && items !== null && <p className="muted">Nothing pushed yet.</p>}
      {done.map((d) => (
        <div key={d.id} className="list-card">
          <span className="list-main">
            <b>R/{d.receipt.value ?? '—'}</b> {d.name.value ?? ''}
            <em className={`badge badge-${d.mode.value === 'ONLINE' ? 'vision' : 'default'}`}>{d.mode.value}</em>
            <span className="when">{shortTime(d.created_at)}</span>
          </span>
          <span className="list-side">{rupees(Number(d.amount.value ?? 0))}</span>
        </div>
      ))}
    </section>
  )
}
