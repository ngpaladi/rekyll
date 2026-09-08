#!/usr/bin/env bash
# Differential for Liquid filters: real Jekyll + Liquid vs rekyll.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
TPL="${1:-tests/harness/filters.liquid}"
cargo build --quiet --example filters 2>/dev/null
ruby tests/harness/filters_ref.rb "$TPL" > /tmp/rekyll-diff/filters-ruby.txt 2>/dev/null
cargo run --quiet --example filters -- "$TPL" > /tmp/rekyll-diff/filters-rust.txt 2>/dev/null
diff -u /tmp/rekyll-diff/filters-ruby.txt /tmp/rekyll-diff/filters-rust.txt && echo "=== FILTERS IDENTICAL ==="
