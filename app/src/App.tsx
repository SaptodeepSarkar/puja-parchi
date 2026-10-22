import { useCallback, useEffect, useState } from 'react'
import { health, getBook, getDraft, setBook, stats } from './api'
import type { Draft, Health, Stats } from './types'
import Capture from './screens/Capture'
import Review from './screens/Review'
import Recent from './screens/Recent'
import Settings from './screens/Settings'

type View = 'capture' | 'review' | 'recent' | 'settings'

const BOOK_KEY = 'puja.book'

export default function App() {
  const [view, setView] = useState<View>('capture')
  const [book, setBookState] = useState<number | null>(() => {
    const n = Number(localStorage.getItem(BOOK_KEY))
    return Number.isFinite(n) && n > 0 ? n : null
  })
  const [recentBooks, setRecentBooks] = useState<number[]>([])
  const [draft, setDraft] = useState<Draft | null>(null)
  const [healthInfo, setHealthInfo] = useState<Health | null>(null)
  const [statsInfo, setStatsInfo] = useState<Stats | null>(null)
  const [toast, setToast] = useState<string | null>(null)

  // The camera app can reload the tab on a low-memory phone; pick the receipt
  // back up instead of losing it.
  useEffect(() => {
    const id = sessionStorage.getItem('puja.draft')
    if (!id) return
    getDraft(id)
      .then((d) => {
        if (d.status === 'pending') {
          setDraft(d)
          setView('review')
        }
      })
      .catch(() => sessionStorage.removeItem('puja.draft'))
  }, [])

  const refreshStats = useCallback(() => {
    stats().then(setStatsInfo).catch(() => setStatsInfo(null))
  }, [])

  useEffect(() => {
    health().then(setHealthInfo).catch(() => setHealthInfo(null))
    getBook()
      .then((b) => {
        if (b.recent_books.length) setRecentBooks(b.recent_books)
        // "Forget this phone" must not be undone by the server's sticky book
        const forgot = sessionStorage.getItem('puja.forgot')
        sessionStorage.removeItem('puja.forgot')
        if (b.book && !forgot && !localStorage.getItem(BOOK_KEY)) {
          localStorage.setItem(BOOK_KEY, String(b.book))
          setBookState(b.book)
        }
      })
      .catch(() => undefined)
    refreshStats()
  }, [refreshStats])

  useEffect(() => {
    if (view === 'recent') refreshStats()
  }, [view, refreshStats])

  useEffect(() => {
    if (!toast) return
    const t = setTimeout(() => setToast(null), 2600)
    return () => clearTimeout(t)
  }, [toast])

  const chooseBook = useCallback(async (n: number) => {
    localStorage.setItem(BOOK_KEY, String(n))
    setBookState(n)
    try {
      const b = await setBook(n)
      setRecentBooks(b.recent_books)
    } catch {
      setToast('Book saved on this phone; server did not answer')
    }
  }, [])

  const onDraft = (d: Draft) => {
    setDraft(d)
    sessionStorage.setItem('puja.draft', d.id)
    setView('review')
  }

  // the review screen stays open on its success card; this only refreshes totals
  const onCommitted = () => {
    refreshStats()
    health().then(setHealthInfo).catch(() => undefined)
  }

  const onDone = () => {
    setDraft(null)
    sessionStorage.removeItem('puja.draft')
    setView('capture')
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark">দুর্গা</span>
          <span className="brand-name">Puja Parchi</span>
        </div>
        <button className="book-chip" onClick={() => setView('settings')}>
          Book {book ?? '—'}
        </button>
      </header>

      <main className="screen">
        {view === 'capture' && (
          <Capture book={book} recentBooks={recentBooks} onBook={chooseBook} onDraft={onDraft} />
        )}
        {view === 'review' && draft && (
          <Review draft={draft} onCommitted={onCommitted} onDone={onDone} />
        )}
        {view === 'recent' && (
          <Recent
            onOpen={(d) => {
              setDraft(d)
              sessionStorage.setItem('puja.draft', d.id)
              setView('review')
            }}
          />
        )}
        {view === 'settings' && (
          <Settings
            book={book}
            health={healthInfo}
            stats={statsInfo}
            onBook={chooseBook}
            onToast={setToast}
            onRecheck={() => health().then(setHealthInfo).catch(() => setHealthInfo(null))}
          />
        )}
      </main>

      {toast && <div className="toast">{toast}</div>}

      <nav className="tabs">
        <button className={view === 'capture' ? 'tab on' : 'tab'} onClick={() => setView('capture')}>
          <span className="tab-icon">◎</span>Capture
        </button>
        <button className={view === 'recent' ? 'tab on' : 'tab'} onClick={() => setView('recent')}>
          <span className="tab-icon">≡</span>Recent
        </button>
        <button className={view === 'settings' ? 'tab on' : 'tab'} onClick={() => setView('settings')}>
          <span className="tab-icon">⚙</span>Setup
        </button>
      </nav>
    </div>
  )
}
