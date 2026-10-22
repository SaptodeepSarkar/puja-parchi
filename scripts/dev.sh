#!/usr/bin/env bash
# Start the puja desk: build the phone app if needed, then run the server.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ ! -f app/dist/index.html ]; then
  echo "building the phone app…"
  (cd app && npm install --no-audit --no-fund && npm run build)
fi

if [ ! -f data/ledger/"Durga Puja 2026.xlsx" ] && [ -z "${PUJA_LEDGER_PATH:-}" ]; then
  echo
  echo "  No ledger at data/ledger/Durga Puja 2026.xlsx"
  echo "  Copy your workbook there, or set PUJA_LEDGER_PATH in .env"
  echo
fi

exec cargo run --manifest-path server/Cargo.toml "$@"
