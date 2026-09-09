---
title: Testing
---

There's a copy of Jekyll 4.3.2 sitting next to rekyll, and every layer gets
diffed against it. Nothing in the docs is claimed that isn't checked here.

## Requirements

You need `jekyll` 4.3.2 on your `PATH`, and a `ruby` with `kramdown`,
`kramdown-parser-gfm` and `liquid` available. `tests/harness/Gemfile` pins
the exact versions, so if you'd rather not trust whatever your distro
installed:

```
cd tests/harness && bundle install && cd ../..
BUNDLE_GEMFILE=tests/harness/Gemfile bundle exec ./tests/harness/all.sh
```

That's what CI runs on every push. One thing it taught me: upstream Jekyll
4.3.2 resolves Liquid 4.0.4, while Debian's package is patched to run on
Liquid 5.4. rekyll matches both.

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
./tests/harness/units_diff.sh          # YAML scalars and strftime
./tests/harness/serve_smoke.sh         # the preview server
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

`units_diff.sh` covers two rule sets dense enough to get subtly wrong
without noticing. Each is a Ruby script and a Rust example that print the
same tab-separated lines, diffed against each other:

```
ruby tests/harness/psych_ref.rb tests/harness/scalars.txt
cargo run --example scalars tests/harness/scalars.txt

ruby tests/harness/strftime_ref.rb tests/harness/strftime_fmts.txt
cargo run --example strftime_dump tests/harness/strftime_fmts.txt
```

Add a line to `scalars.txt` or `strftime_fmts.txt` and both sides pick it
up. That's how the `strftime-ruby` crate earned its place: I added
`%-10A`, `%^#p` and `%+` to the list and the hand-written version got all
three wrong.

## The Server

`serve_smoke.sh` starts `rekyll serve` on port 4321 against this docs site
and checks with `curl` that pages route (`/features` finds
`features.html`), that `../` and `%2e%2e` get a 404, that the reload script
is in served HTML and not in served CSS or the file on disk, and that
`--no-livereload` leaves it out.
