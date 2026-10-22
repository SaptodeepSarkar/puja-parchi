import type { BookInfo, ConfirmResult, Draft, Health, Stats, Totals } from './types'

const BASE_KEY = 'puja.api_base'
const PIN_KEY = 'puja.pin'

export const api = {
  base(): string {
    return localStorage.getItem(BASE_KEY) ?? ''
  },
  setBase(v: string) {
    const clean = v.trim().replace(/\/+$/, '')
    if (clean) localStorage.setItem(BASE_KEY, clean)
    else localStorage.removeItem(BASE_KEY)
  },
  pin(): string {
    return localStorage.getItem(PIN_KEY) ?? ''
  },
  setPin(v: string) {
    if (v) localStorage.setItem(PIN_KEY, v)
    else localStorage.removeItem(PIN_KEY)
  },
}

export class ApiError extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

/** Photo and voice URLs cannot send headers, so the PIN rides along in the query. */
export function media(url: string): string {
  const pin = api.pin()
  if (!pin || !url) return url
  return `${url}${url.includes('?') ? '&' : '?'}pin=${encodeURIComponent(pin)}`
}

async function request<T>(path: string, init: RequestInit = {}, timeoutMs = 20_000): Promise<T> {
  const headers = new Headers(init.headers)
  const pin = api.pin()
  if (pin) headers.set('x-pin', pin)
  if (init.body && typeof init.body === 'string') headers.set('content-type', 'application/json')

  let res: Response
  try {
    res = await fetch(api.base() + path, { signal: AbortSignal.timeout(timeoutMs), ...init, headers })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'TimeoutError') {
      throw new ApiError(0, 'The server did not answer in time. Check the wifi and try again.')
    }
    throw new ApiError(0, 'Cannot reach the server. Are you on the puja wifi?')
  }
  const text = await res.text()
  let body: unknown = null
  try {
    body = text ? JSON.parse(text) : null
  } catch {
    body = text
  }
  if (!res.ok) {
    // axum answers 413 and friends with plain text, which is more useful than
    // "Request failed (413)".
    const plain = typeof body === 'string' && body.length < 200 && !body.startsWith('<') ? body.trim() : ''
    const msg =
      body && typeof body === 'object' && 'error' in body
        ? String((body as { error: unknown }).error)
        : plain || `Request failed (${res.status})`
    throw new ApiError(res.status, msg)
  }
  return body as T
}

export const get = <T>(path: string) => request<T>(path)
export const post = <T>(path: string, body?: unknown) =>
  request<T>(path, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) })
export const patch = <T>(path: string, body: unknown) =>
  request<T>(path, { method: 'PATCH', body: JSON.stringify(body) })

export const health = () => get<Health>('/api/health')
export const getBook = () => get<BookInfo>('/api/book')
export const setBook = (book: number) => post<BookInfo>('/api/book', { book })
export const stats = () => get<Stats>('/api/stats')
export const recent = (limit = 25) => get<Draft[]>(`/api/drafts?limit=${limit}`)
export const getDraft = (id: string) => get<Draft>(`/api/draft/${id}`)
export const rejectDraft = (id: string) => post<Draft>(`/api/draft/${id}/reject`)
export const editDraft = (
  id: string,
  body: { book?: number; receipt?: string; name?: string; amount?: string; mode?: string },
) => patch<Draft>(`/api/draft/${id}`, body)
export const confirmDraft = (id: string) => post<ConfirmResult>(`/api/draft/${id}/confirm`)
export const totals = (t: Totals) => t

export async function capture(input: {
  image: Blob
  book: number
  transcript: string
  audio?: Blob | null
  clientId: string
}): Promise<Draft> {
  const form = new FormData()
  form.set('image', input.image, 'receipt.jpg')
  if (input.audio) form.set('audio', input.audio, 'voice.webm')
  form.set('transcript', input.transcript)
  form.set('book', String(input.book))
  form.set('client_id', input.clientId)
  // a vision call may take 45 s and speech 20 s on a slow connection
  return request<Draft>('/api/capture', { method: 'POST', body: form }, 90_000)
}
