export type Mode = 'CASH' | 'ONLINE'
export type Source = 'voice' | 'vision' | 'ocr' | 'namedb' | 'manual' | 'default'
export type Status = 'pending' | 'committed' | 'rejected'

export interface Field {
  value: string | null
  source: Source
  confidence: number
  evidence: string[]
  alternates: string[]
}

export interface Draft {
  id: string
  book: number
  receipt: Field
  name: Field
  amount: Field
  mode: Field
  status: Status
  raw_text: string
  vision_note: string
  transcript: string
  photo_url: string
  audio_url: string | null
  warnings: string[]
  created_at: string
  committed_row: number | null
}

export interface Totals {
  cash: number
  online: number
  total: number
}

export interface ConfirmResult {
  draft: Draft
  row: number
  totals: Totals
}

export interface Health {
  ok: boolean
  version: string
  vision: string
  stt: string
  ledger_ok: boolean
  ledger_path: string
  active_book: number | null
  recent_books: number[]
}

export interface BookInfo {
  book: number | null
  recent_books: number[]
}

export interface Stats {
  today_cash: number
  today_online: number
  today_total: number
  today_receipts: number
  ledger_ok: boolean
  ledger_cash: number
  ledger_online: number
  ledger_total: number
  ledger_row: number
}
