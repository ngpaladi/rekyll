# 10-pages

Covers `site.pages` ordering, page URLs, and the sequential content updates
Jekyll performs as each page renders.

Page basenames are deliberately unique. Jekyll sorts `site.pages` with
`sort_by!(&:name)`, which is Ruby's *unstable* sort, over entries that
`Dir.entries` returned in filesystem order. Two pages sharing a basename —
`sub/index.html` and `other/index.html`, say — therefore come out in an order
that depends on directory inode layout, not on the source tree. The same
checkout cloned to a different path produces a different order from real
Jekyll. rekyll sorts stably, which is deterministic but cannot match an order
that is not a function of the input. See the README.
