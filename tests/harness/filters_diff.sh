#!/usr/bin/env bash
# Differential for Liquid filters: real Jekyll + Liquid vs rekyll.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
mkdir -p "${REKYLL_WORK:-/tmp/rekyll-diff}"
export REKYLL_WORK="${REKYLL_WORK:-/tmp/rekyll-diff}"
TPL="${1:-tests/harness/filters.liquid}"
cargo build --quiet --example filters 2>/dev/null
ruby tests/harness/filters_ref.rb "$TPL" > $REKYLL_WORK/filters-ruby.txt 2>/dev/null
cargo run --quiet --example filters -- "$TPL" > $REKYLL_WORK/filters-rust.txt 2>/dev/null
diff -u $REKYLL_WORK/filters-ruby.txt $REKYLL_WORK/filters-rust.txt && echo "=== FILTERS IDENTICAL ==="
