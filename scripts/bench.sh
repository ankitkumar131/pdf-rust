#!/usr/bin/env bash
# Headless performance report: startup, first paint and steady-state frame cost.
#
#   scripts/bench.sh [pdf]
#
# Without an argument a synthetic 200-page document is generated into a
# temporary directory (tests/gen_fixtures.rs) and measured three times.
set -euo pipefail

cd "$(dirname "$0")/.."
pdf=${1:-}

if [ -z "$pdf" ]; then
  dir=${TMPDIR:-/tmp}/pdf-rust-bench
  mkdir -p "$dir"
  cargo test --offline --quiet --test gen_fixtures -- --ignored --nocapture >/dev/null
  pdf="$dir/benchmark-200.pdf"
fi

cargo build --offline --release
bin=./target/release/pdf-rust

echo "document: $pdf ($(du -h "$pdf" | cut -f1), $(pdfinfo "$pdf" 2>/dev/null | awk '/^Pages/{print $2}') pages)"
for run in 1 2 3; do
  echo "--- run $run"
  "$bin" --selftest --no-db "$pdf" | sed 's/^/    /'
done
