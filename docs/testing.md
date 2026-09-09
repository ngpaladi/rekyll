---
title: Testing
---

There's a copy of Jekyll 4.3.2 sitting next to rekyll, and every layer gets
diffed against it. Nothing in the docs is claimed that isn't checked here.

## Requirements

You need `jekyll` 4.3.2 on your `PATH`, and a `ruby` with `kramdown`,
`kramdown-parser-gfm` and `liquid` available.

## Run everything

```
./tests/harness/all.sh
```

## Run one thing

```
./tests/harness/diff.sh                # all site fixtures
./tests/harness/diff.sh 08-timezone    # one fixture
./tests/harness/md_diff.sh             # Markdown corpus
./tests/harness/filters_diff.sh        # filter corpus
```

`diff.sh` builds every fixture twice, once with `jekyll build` and once with
`rekyll build`, then throws `diff -r` at the two `_site` trees. Set
`REKYLL_WORK` if you want the builds somewhere else, or `REKYLL_BIN` to point
it at a different binary.

## Fixtures

| fixture | covers |
|---------|--------|
| `01-static` | static files, include and exclude rules |
| `02-page-layout` | layouts, layout chains, CRLF front matter |
| `03-posts` | permalinks, categories, tags, publishing |
| `04-includes` | includes, collections, defaults, link tags |
| `05-sass` | Sass, libsass `:compact` output |
| `06-highlight` | the `highlight` tag, with and without `linenos` |
| `07-blog` | markdown posts, excerpts, an Atom feed |
| `08-timezone` | `America/New_York`, DST, unzoned dates |
| `09-blank-template` | `jekyll new --blank` output, unmodified |
| `10-pages` | page ordering, sequential content updates |
| `11-docs` | this site |

Every fixture pins `timezone` and `time` in its `_config.yml`. If you don't do
that, Jekyll's own output depends on the wall clock and your local timezone,
and you'll spend an afternoon chasing diffs that aren't real.

## Adding A Fixture

Make `tests/fixtures/NN-name/` with a `_config.yml` that pins `timezone` and
`time`, then run `./tests/harness/diff.sh NN-name`. If it fails you get the
diff of both trees printed straight out.

## Scalar And Date Differentials

Two more differentials cover rules that are dense enough to get subtly wrong
without noticing:

```
ruby tests/harness/psych_ref.rb tests/harness/scalars.txt
cargo run --example scalars tests/harness/scalars.txt

ruby tests/harness/strftime_ref.rb tests/harness/strftime_fmts.txt
cargo run --example strftime_dump tests/harness/strftime_fmts.txt
```

Diff each pair against each other.
