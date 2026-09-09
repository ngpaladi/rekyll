# rekyll

Jekyll, but make it Rust.

A from-scratch reimplementation of the Jekyll static site generator that
produces **byte-identical output from identical input**. It is not
plugin-compatible; it is output-compatible.

Correctness here is measured, not asserted. Jekyll 4.3.2 is installed
alongside, and every layer is diffed against the real implementation rather
than against expectations.

```
$ ./tests/harness/all.sh
=== rekyll differential harness (11 fixtures) ===
  [ OK ] 01-static          static files, include/exclude rules
  [ OK ] 02-page-layout     layouts, layout chains, CRLF front matter
  [ OK ] 03-posts           permalinks, categories, tags, publishing
  [ OK ] 04-includes        includes, collections, defaults, link tags
  [ OK ] 05-sass            Sass, libsass :compact output
  [ OK ] 06-highlight       the highlight tag, with and without linenos
  [ OK ] 07-blog            a realistic blog: markdown, excerpts, a feed
  [ OK ] 08-timezone        America/New_York, DST, unzoned dates
  [ OK ] 09-blank-template  Jekyll's own `jekyll new --blank` output
  [ OK ] 10-pages           page ordering and sequential content updates
  [ OK ] 11-docs            this project's own docs/ site
=== 11 passed, 0 failed ===
=== MARKDOWN IDENTICAL ===
=== FILTERS IDENTICAL ===
=== YAML SCALARS IDENTICAL ===
=== STRFTIME IDENTICAL ===
=== SERVE OK ===
```

## Usage

```
cargo install --path . --locked
rekyll build -s path/to/site -d path/to/_site
```

`--locked` matters: `cargo install` otherwise re-resolves dependencies and
picks a `kstring` that needs a newer rustc than this crate is built against.
The result is one self-contained executable — no Ruby, no gems, nothing
alongside it.

## Docs

`docs/` is a Jekyll site documenting rekyll, built by rekyll. It is also
fixture 11, so it is byte-identical to Jekyll's build of the same tree by
construction. To read it locally:

```
rekyll serve -s docs -d docs/_site          # http://127.0.0.1:4000
```

`serve` is a default Cargo feature (it pulls in `tiny_http` and
`mime_guess`); build with `--no-default-features` for a build-only binary. It watches the source and
rebuilds, and the served HTML carries a small polling script so the browser
reloads itself; `--no-watch` and `--no-livereload` turn those off. The script
is added to the response, never to the file on disk, so builds still compare
byte-for-byte against Jekyll. It is not meant to face a network.

## What matches Jekyll exactly

Verified byte-for-byte against Jekyll 4.3.2 on every fixture, plus a 1202-post
generated site:

- **Configuration** — the full `DEFAULTS` set, deep merge semantics, default
  collections and excludes, permalink styles.
- **YAML** — Ruby's Psych, which is YAML **1.1**: `yes`/`on` are booleans,
  `010` is octal, `1_000` and `1,000` are 1000, `1e3` is a *string*, quoted
  scalars are never type-resolved. Diffed scalar by scalar against Psych.
- **Front matter, pages, layouts** — layout chains, `layout: none`, nested
  layout data merging.
- **Posts and collections** — filename and front-matter dates in the site
  timezone, categories from directory path, permalink placeholders, the
  publisher's `future`/`published` filtering, `_posts` in any directory.
- **Includes and tags** — `include`, `include_relative`, `link`, `post_url`,
  `highlight`.
- **Front-matter defaults** — `scope`/`values` with Jekyll's precedence rules.
- **Timezones** — dates are resolved the way Psych and `Time#localtime` do, so
  an unzoned `date: 2020-01-02 03:04:05` under `America/New_York` publishes at
  `/2020/01/01/`, exactly as Jekyll does.
- **Liquid filters** — Jekyll's own set plus overrides where Ruby differs.
  110 expressions diffed against Ruby Liquid, on both 4.0.4 (what upstream
  Jekyll 4.3.2 resolves) and 5.4.0 (what Debian ships it with).
- **Markdown** — a Kramdown-compatible emitter (see below).
- **Sass** — including libsass's `:compact` output style.
- **Excerpts**, including the link-reference definitions Jekyll appends.

## What differs, and why

These are deliberate and documented, not unknowns.

**Syntax-highlighted code.** A fenced block with a language tag is tokenized by
Rouge into per-token `<span>`s. Reproducing that means porting Rouge's lexers,
one per language. rekyll emits the exact wrapper with escaped but untokenized
code. Blocks with no language, indented blocks, inline code, and
`{% highlight text %}` all use Rouge's `plaintext` lexer and **are** identical.

**Kramdown-only Markdown syntax.** Inline attribute lists (`{:.class}`),
definition lists, abbreviations and math are not implemented. Kramdown also
merges adjacent lists that use different bullet markers, where rekyll follows
CommonMark. See `tests/markdown/divergences/`.

**`site.pages` order for pages sharing a basename.** Jekyll sorts
`site.pages` with `sort_by!(&:name)` — Ruby's *unstable* sort — over entries
returned by `Dir.entries` in filesystem order. So `sub/index.html` and
`other/index.html` come out in an order determined by directory inode layout,
not by the source tree: the same commit cloned to a different path builds a
different `site.pages` order in real Jekyll. That order is not a function of
the input, so it cannot be reproduced. rekyll sorts stably by name, which is
deterministic. Pages with distinct basenames match exactly.

**Sass source maps.** Jekyll writes a `.css.map` and appends a
`sourceMappingURL` comment. rekyll does not generate source maps; with
`sass: {sourcemap: never}` the CSS is byte-identical.

**Gem themes are not supported**, and this is the gap you are most likely to
hit first. A stock `jekyll new` site uses the `minima` theme, whose layouts and
includes live inside a gem. rekyll will build it without erroring and without
layouts, which is worse than failing. `jekyll new --blank` has no theme and is
covered by fixture 09.

**Also not implemented**: drafts (`_drafts`), pagination, `site.related_posts`,
CoffeeScript, TOML config, non-YAML data files, and incremental builds.
`sample` works but cannot match Jekyll, whose own is unseeded.

## Packages

Every published release carries a `.deb` (Ubuntu/Debian), an `.rpm`
(Fedora), the static binary as a tarball, and `SHA256SUMS`, built by
`.github/workflows/release.yml`. Both packages hold the musl binary, so they
depend on nothing, and the workflow installs each on its distro and builds a
fixture with it before attaching anything to the release. To build them
locally:

```
cargo install cargo-deb cargo-generate-rpm
cargo build --release --target x86_64-unknown-linux-musl
cargo deb --no-build --target x86_64-unknown-linux-musl
cargo generate-rpm --target x86_64-unknown-linux-musl
```

`.github/workflows/ci.yml` runs the whole differential suite on every push
against a pinned Jekyll 4.3.2 (`tests/harness/Gemfile`), builds `docs/` with
rekyll, and publishes it to GitHub Pages from `main`.

## Distributing the binary

The release binary links only libc, so it runs on any glibc Linux. For a
fully static one (no libc either, so it also runs on Alpine or in a
`FROM scratch` container):

```
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

That binary is about 9.4 MiB, `ldd` reports it statically linked, and it is
byte-identical to the glibc build on every fixture. Everything compiled into it is MIT, Apache-2.0, BSD, Zlib or
Unlicense, and `THIRD_PARTY_LICENSES.md` lists each crate with its license
text. That file is embedded, so a shipped binary carries its own attribution:

```
rekyll licenses
```

Regenerate it after changing `Cargo.toml` with `scripts/licenses.py`.

## Determinism

Identical input must give identical output on every run, which turned out to
be the first thing to get wrong rather than a given. liquid-core's `Object` is
a `std::HashMap`, whose iteration order is arbitrary and randomly seeded per
process, while Jekyll exposes Ruby hashes whose insertion order is visible
through `{% for %}`. Early builds produced:

```
cats: blog(1) news(1) updates(1)
cats: updates(1) blog(1) news(1)
```

from the same source. `vendor/liquid-core` fixes this at the root.

## `vendor/liquid-core`

Liquid itself is a dependency, not a rewrite — but seven changes to
liquid-core 0.26.11 were needed. `vendor/rekyll-liquid-core.patch` (about 250
lines) is the source of truth; `scripts/vendor.sh` rebuilds the checked-in
`vendor/liquid-core` from the pristine crates.io tarball plus that patch, and
`[patch.crates-io]` wires it in. Each change is marked with a `rekyll:`
comment:

1. **`Object` uses `IndexMap`** instead of `HashMap`, so hash iteration order
   is stable and matches Ruby's insertion order.
2. **`shift_remove` instead of `remove`**, since IndexMap's `remove` is
   `swap_remove` and would reorder the map that change 1 exists to preserve.
3. **`TagTokenIter::raw_markup()`** exposes a tag's arguments as written.
   Jekyll's tags do not follow Liquid's argument grammar — `{% include
   nav/menu.html a="b" %}` has no colons — so they must parse their own markup.
4. **An `UnstructuredToken` grammar fallback**, tried only after every real
   production fails, so such tags tokenize at all.
5. **Unknown filters pass their input through.** Jekyll runs with
   `strict_filters: false`, so `{{ x | some_plugin_filter }}` renders `x`.
   Raising instead means a site written for plugins fails to build rather than
   rendering without them.
6. **Ruby float formatting.** `Float#to_s` keeps a decimal point, so
   `1.5 | plus: 1.5` renders `3.0`, not `3`.
7. **Borrowed variable lookup.** `Runtime::get` deep-cloned a value that
   `find()` had already returned borrowed, which made `{{ site.posts }}` cost
   O(posts) per access.

Unknown *variables* needed no patch: `src/lax.rs` wraps the payload in a value
tree whose lookups always succeed with nil, matching `strict_variables: false`.

## Performance

A generated site, built on the same machine:

| posts | Jekyll 4.3.2 | rekyll | output |
|-------|--------------|--------|--------|
| 302   | 0.58s        | 0.17s  | identical |
| 1202  | 1.43s        | 0.66s  | identical |

## Testing

The harnesses are the specification.

| script | what it diffs |
|--------|---------------|
| `tests/harness/all.sh` | everything below |
| `tests/harness/diff.sh 08-timezone` | dates outside UTC — the most error-prone area |
| `tests/harness/diff.sh 11-docs` | this project's own `docs/` site |
| `tests/harness/diff.sh [fixture…]` | full `_site` trees, `jekyll build` vs `rekyll build` |
| `tests/harness/md_diff.sh` | the Markdown corpus against kramdown 2.4 + GFM |
| `tests/harness/filters_diff.sh` | filter expressions against Ruby Liquid 5.4 |
| `tests/harness/units_diff.sh` | YAML scalars against Psych; strftime against Ruby's `Time#strftime` |
| `tests/harness/serve_smoke.sh` | `rekyll serve`: routing, traversal refusal, reload injection |

Requires `jekyll` (4.3.2) on `PATH`; `tests/harness/Gemfile` pins the exact
stack, so `BUNDLE_GEMFILE=tests/harness/Gemfile bundle exec ./tests/harness/all.sh`
runs it against upstream's resolution. Fixtures pin `timezone` and `time` in
`_config.yml`, without which Jekyll's own output depends on the wall clock and
the local timezone.

## Layout

In the order a build touches them:

```
src/main.rs       the CLI: build, serve, licenses
src/build.rs      read, render, clean, write
src/config.rs     Jekyll's DEFAULTS, merge order, front-matter defaults
src/value.rs      the YAML/Ruby value type (ordered hashes, Time)
src/yaml.rs       YAML 1.1 scalar resolution, ported from Psych
src/site.rs       reading: front matter, entry filtering, pages, URLs
src/document.rs   collections, posts, permalink placeholders
src/url.rs        permalink escaping, relative_url/absolute_url, slugify
src/time.rs       Ruby Time semantics; strftime via the strftime-ruby crate
src/render.rs     payload assembly, Liquid, converters, layout chain, excerpts
src/lax.rs        Liquid values with Jekyll's lax lookup semantics
src/filters.rs    Jekyll's filters and Ruby-behaviour overrides
src/tags.rs       include, include_relative, link, post_url, highlight
src/markdown.rs   Kramdown-compatible emitter over pulldown-cmark
src/sass.rs       Sass via grass, reformatted to libsass :compact
src/serve.rs      the preview server (tiny_http), watcher, live reload
```

`docs/how-it-works/` walks through the same list stage by stage.

## License

MIT. `vendor/liquid-core` is liquid-core 0.26.11 (MIT OR Apache-2.0) with the
changes listed above. Third-party licenses: `THIRD_PARTY_LICENSES.md`, or
`rekyll licenses`.
