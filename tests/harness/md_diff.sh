#!/usr/bin/env bash
# Differential for the Markdown converter alone: kramdown vs rekyll.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
mkdir -p "${REKYLL_WORK:-/tmp/rekyll-diff}"
export REKYLL_WORK="${REKYLL_WORK:-/tmp/rekyll-diff}"
files=("$@")
[ ${#files[@]} -eq 0 ] && mapfile -t files < <(ls -1 tests/markdown/*.md | sort)
cargo build --quiet --example md 2>/dev/null
ruby tests/harness/kramdown_ref.rb "${files[@]}" > $REKYLL_WORK/md-ruby.txt 2>/dev/null
cargo run --quiet --example md -- "${files[@]}" > $REKYLL_WORK/md-rust.txt 2>/dev/null
diff -u $REKYLL_WORK/md-ruby.txt $REKYLL_WORK/md-rust.txt && echo "=== MARKDOWN IDENTICAL ==="
