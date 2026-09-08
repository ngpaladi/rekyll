#!/usr/bin/env bash
# Differential for the Markdown converter alone: kramdown vs rekyll.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
files=("$@")
[ ${#files[@]} -eq 0 ] && mapfile -t files < <(ls -1 tests/markdown/*.md | sort)
cargo build --quiet --example md 2>/dev/null
ruby tests/harness/kramdown_ref.rb "${files[@]}" > /tmp/rekyll-diff/md-ruby.txt 2>/dev/null
cargo run --quiet --example md -- "${files[@]}" > /tmp/rekyll-diff/md-rust.txt 2>/dev/null
diff -u /tmp/rekyll-diff/md-ruby.txt /tmp/rekyll-diff/md-rust.txt && echo "=== MARKDOWN IDENTICAL ==="
