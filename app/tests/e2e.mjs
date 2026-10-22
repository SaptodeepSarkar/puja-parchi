/**
 * Real-browser smoke test: photo -> 6 s voice screen -> review -> Excel row.
 * Runs the server with the fake vision provider so it needs no API key.
 *
 *   node tests/e2e.mjs
 */
import { spawn } from 'node:child_process'
import { chromium } from 'playwright'
import { mkdtempSync, copyFileSync, mkdirSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '../..')
const PORT = 8097
const BASE = `http://127.0.0.1:${PORT}`

const work = mkdtempSync(join(tmpdir(), 'parchi-e2e-'))
mkdirSync(join(work, 'ledger'), { recursive: true })
const ledger = join(work, 'ledger', 'book.xlsx')
copyFileSync(join(root, 'data/ledger/Durga Puja 2026.xlsx'), ledger)

const photo = join(work, 'stub.jpg')
copyFileSync(resolve(root, 'app/public/icon-512.png'), photo) // any real image will do here

function fail(msg) {
  console.error(`\n  FAIL: ${msg}\n`)
  process.exitCode = 1
}

const server = spawn(resolve(root, 'server/target/debug/puja-server'), [], {
  cwd: root,
  env: {
    ...process.env,
    PUJA_APP_DIR: join(root, 'app/dist'),
    PUJA_DATA_DIR: work,
    PUJA_LEDGER_PATH: ledger,
    PUJA_VISION_PROVIDER: 'fake',
    PUJA_ADDR: `127.0.0.1:${PORT}`,
    PUJA_TLS: 'off',
    RUST_LOG: 'warn',
  },
  stdio: ['ignore', 'pipe', 'pipe'],
})
server.stderr.on('data', (d) => process.env.DEBUG_E2E && process.stderr.write(d))

const wait = (ms) => new Promise((r) => setTimeout(r, ms))
for (let i = 0; i < 60; i++) {
  try {
    const res = await fetch(`${BASE}/api/health`)
    if (res.ok) break
  } catch {
    /* not up yet */
  }
  await wait(250)
}

const before = await (await fetch(`${BASE}/api/stats`)).json()
const expectRow = before.ledger_row + 1

const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
})
const context = await browser.newContext({
  viewport: { width: 390, height: 844 },
  deviceScaleFactor: 2,
  permissions: ['camera', 'microphone'],
})
const page = await context.newPage()
const errors = []
page.on('pageerror', (e) => errors.push(e.message))

try {
  await page.addInitScript(() => {
    localStorage.setItem('puja.book', '18')
  })
  await page.goto(BASE)
  await page.waitForSelector('text=New receipt')
  if (!(await page.isVisible('text=Book 18'))) fail('book chip did not show 18')
  await page.screenshot({ path: join(work, '1-capture.png') })

  await page.setInputFiles('input[type=file]', photo)
  await page.waitForSelector('text=Listening', { timeout: 5000 })
  const countdown = await page.textContent('.dial-num')
  if (countdown !== '6' && countdown !== '5') fail(`countdown started at ${countdown}`)
  await page.screenshot({ path: join(work, '2-voice.png') })

  // a volunteer stops early; we must not wait for the full 6 s
  await page.click('text=Stop & use this')
  await page.waitForSelector('text=Check the stub', { timeout: 20000 })
  const name = await page.inputValue('label:has-text("Name") input')
  if (name !== 'SUDHA DAIRY') fail(`name field was ${JSON.stringify(name)}`)
  // the picture must really decode, not just be fetchable
  const shown = await page
    .waitForFunction(() => {
      const img = document.querySelector('.photo')
      return img && img.complete && img.naturalWidth > 0
    }, null, { timeout: 10000 })
    .then(() => true)
    .catch(() => false)
  if (!shown) fail('receipt photo did not render')
  const receipt = await page.inputValue('label:has-text("Receipt no") input')
  if (receipt !== '9501') fail(`receipt field was ${receipt}`)
  await page.screenshot({ path: join(work, '3-review.png') })
  // the bottom of the form must be reachable above the tab bar
  await page.locator('text=Push to Excel').scrollIntoViewIfNeeded()
  await page.screenshot({ path: join(work, '3b-review-bottom.png') })

  await page.click('text=Push to Excel')
  await page.waitForSelector('text=Pushed to Excel', { timeout: 20000 })
  await page.screenshot({ path: join(work, '4-done.png') })

  const health = await (await fetch(`${BASE}/api/stats`)).json()
  if (health.ledger_row !== expectRow) fail(`ledger row is ${health.ledger_row}, expected ${expectRow}`)

  const stats = await (await fetch(`${BASE}/api/stats`)).json()
  if (stats.today_total !== 1100) fail(`today total ${stats.today_total}`)

  if (errors.length) fail(`page errors: ${errors.join(' | ')}`)
  console.log(`\n  PASS  screenshots in ${work}`)
  console.log(`  ledger rows: ${health.ledger_row}, today: ₹${stats.today_total}\n`)
} catch (e) {
  fail(e.message)
  await page.screenshot({ path: join(work, 'fail.png'), fullPage: true }).catch(() => {})
  console.log(`  screenshot: ${work}/fail.png`)
} finally {
  await browser.close()
  server.kill()
}

if (process.env.DEBUG_E2E) {
  const report = readFileSync(ledger).length
  console.log(`ledger bytes after run: ${report}`)
}
