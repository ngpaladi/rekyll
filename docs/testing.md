---
title: Testing
summary: The harnesses are the specification. Nothing is asserted that isn't diffed.
---

Jekyll 4.3.2 is installed alongside rekyll, so every layer is compared against
the real implementation rather than against expectations. The harness was
written **before** any generator code.

```
./tests/harness/all.sh
```

| script | what it diffs |
|--------|---------------|
| `diff.sh [fixture…]` | full `_site` trees, `jekyll build` vs `rekyll build` |
| `md_diff.sh` | the Markdown corpus against kramdown 2.4 + GFM |
| `filters_diff.sh` | ~100 filter expressions against Ruby Liquid 5.4 |
| `all.sh` | all of the above |

## Fixtures

Each was written to probe an untested surface, not to confirm behaviour that
already worked.

| fixture | covers |
|---------|--------|
| `01-static` | static files, include/exclude rules |
| `02-page-layout` | layouts, layout chains, CRLF front matter |
| `03-posts` | permalinks, categories, tags, publishing |
| `04-includes` | includes, collections, defaults, link tags |
| `05-sass` | Sass, libsass `:compact` output |
| `06-highlight` | the `highlight` tag, with and without `linenos` |
| `07-blog` | a realistic blog: markdown, excerpts, an Atom feed |
| `08-timezone` | `America/New_York`, DST, unzoned dates |
| `09-blank-template` | Jekyll's own `jekyll new --blank` output |
| `10-pages` | page ordering, sequential content updates |
| `11-docs` | this site |

Fixtures pin `timezone` and `time` in `_config.yml`. Without that, Jekyll's own
output depends on the wall clock and the local timezone, and the diff chases
phantoms.

## Two more differentials

Kept from development, because they pin down rules that are dense and easy to
get subtly wrong:

- `examples/scalars.rs` against `psych_ref.rb` — 52 YAML scalars.
- `examples/strftime_dump.rs` against `strftime_ref.rb` — 18 format strings
  across 4 timezone-varied times.

## The fixture that mattered most

`08-timezone` is the one that would have caught the most real-world breakage,
and everything before it was UTC. It is highly discriminating: under
`America/New_York`, a post with `date: 2020-01-02 03:04:05` publishes at
`/2020/01/01/`, because Psych reads an unzoned time as a UTC instant that
`Time#localtime` then shifts. A generator that got this wrong would put posts
on the wrong day and never notice under UTC.
