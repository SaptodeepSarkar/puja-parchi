import type { Source } from '../types'

export const rupees = (n: number): string =>
  `₹${new Intl.NumberFormat('en-IN', { maximumFractionDigits: 0 }).format(Math.round(n || 0))}`

export const sourceLabel = (s: Source): string =>
  ({ voice: 'you said', vision: 'vision', ocr: 'ocr', namedb: 'name db', manual: 'typed', default: 'default' })[s] ??
  s

export const sourceClass = (s: Source): string => `badge badge-${s}`

export const shortTime = (iso: string): string => {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleString('en-IN', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' })
}

export const confidenceWord = (c: number): string =>
  c >= 0.9 ? 'strong' : c >= 0.7 ? 'likely' : c >= 0.5 ? 'check this' : 'guess'
