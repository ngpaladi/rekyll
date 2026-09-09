---
title: Testing
---

Jekyll 4.3.2 is installed alongside rekyll, and every layer is diffed against
it. Nothing is asserted that is not diffed.

## Requirements

- `jekyll` 4.3.2 on `PATH`.
- `ruby` with `kramdown`, `kramdown-parser-gfm` and `liquid`.

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

`diff.sh` builds each fixture with `jekyll build` and with `rekyll build`, then
compares the two `_site` trees with `diff -r`. Set `REKYLL_WORK` to change
where builds land, `REKYLL_BIN` to test a different binary.

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

Fixtures pin `timezone` and `time` in `_config.yml`. Without that, Jekyll's own
output depends on the wall clock and the local timezone.

## Adding a fixture

1. Create `tests/fixtures/NN-name/` with a `_config.yml` that pins `timezone` and `time`.
2. Run `./tests/harness/diff.sh NN-name`.
3. If it fails, the diff shows both trees.

## Scalar and date differentials

Two extra differentials cover rules that are dense and easy to get wrong:

```
ruby tests/harness/psych_ref.rb tests/harness/scalars.txt
cargo run --example scalars tests/harness/scalars.txt

ruby tests/harness/strftime_ref.rb tests/harness/strftime_fmts.txt
cargo run --example strftime_dump tests/harness/strftime_fmts.txt
```

Diff the two outputs of each pair.
