---
title: Fidelity
summary: What matches Jekyll exactly, what deliberately differs, and what is not implemented.
---

Boundaries are worth stating precisely, because the useful question is not
"does it work" but "where does it stop, and how will I notice".

## Matches exactly

Verified byte-for-byte on every fixture and on a generated 1202-post site.

- **Configuration** — the full `DEFAULTS` set, deep-merge semantics, default
  collections and excludes, all five permalink styles.
- **YAML** — Psych's 1.1 scalar rules, diffed scalar by scalar.
- **Front matter, pages, layouts** — layout chains, `layout: none`, nested
  layout data merging, CRLF sources.
- **Posts and collections** — filename and front-matter dates in the site
  timezone, categories from directory path, permalink placeholders, the
  publisher's `future` and `published` filtering.
- **Includes and tags** — `include`, `include_relative`, `link`, `post_url`,
  `highlight`.
- **Front-matter defaults** — `scope`/`values` with Jekyll's precedence rules.
- **Filters** — Jekyll's set, plus overrides wherever Ruby's Liquid differs
  from liquid-rust.
- **Markdown** — a Kramdown-compatible emitter.
- **Sass** — including libsass's `:compact` output style.
- **Excerpts**, including the link-reference definitions Jekyll appends so
  `[text][ref]` still resolves once the tail is gone.

## Deliberate divergences

These are known and bounded, not unknowns.

### Syntax-highlighted code

{% include verdict.html state="documented-divergence" text="Wrapper identical; Rouge's per-token spans are not reproduced." %}

A fenced block with a language tag is tokenized by Rouge into
`<span class="k">`-style spans. Reproducing that means porting Rouge's lexers,
one per language. rekyll emits the exact surrounding structure with the code
escaped but untokenized.

Blocks with **no** language tag, indented blocks, inline code, and
`{% raw %}{% highlight text %}{% endraw %}` all use Rouge's `plaintext` lexer,
which does exactly this — so those *are* byte-identical.

### Kramdown-only syntax

Inline attribute lists (`{:.class}`), definition lists, abbreviations and math
are not implemented. Kramdown also merges two adjacent lists separated by a
blank line even when they use different bullet markers or a different ordinal
start; rekyll follows CommonMark and keeps them separate.

### Sass source maps

Jekyll writes a `.css.map` and appends a `sourceMappingURL` comment. rekyll
does not generate source maps. With `sass: {sourcemap: never}` the CSS is
byte-identical — which is what this site uses.

### Page order for pages sharing a basename

{% include verdict.html state="documented-divergence" text="Not reproducible in principle: Jekyll's own order is not a function of the input." %}

Jekyll sorts `site.pages` with `sort_by!(&:name)` — Ruby's *unstable* sort —
over entries `Dir.entries` returned in filesystem order. Two pages named
`index.html` in different directories therefore come out in an order set by
directory inode layout. The same commit cloned to another path builds a
different order **in real Jekyll**:

```
this checkout:  other/index.html, sub/index.html
fresh clone:    sub/index.html, other/index.html
```

That order is not a function of the source tree, so it cannot be reproduced.
rekyll sorts stably, which is at least deterministic. Pages with distinct
basenames match exactly.

## Not implemented

### Gem themes — the gap you will hit first

{% include verdict.html state="documented-divergence" text="Builds successfully with the wrong output. A warning is printed per missing layout." %}

A stock `jekyll new` site uses the `minima` theme, whose layouts and includes
live inside a gem. rekyll warns for each missing layout and emits bare
content — it does not error. That is worse than failing, so it is worth
knowing before you point it at a real site. `jekyll new --blank` has no theme
and is covered by a fixture.

### Filters Jekyll has and rekyll lacks

`where_exp`, `group_by_exp`, `find_exp`, `sample`, `sassify`, `scssify`.

These need Liquid expression evaluation inside a filter argument. They pass
their input through and **print a build warning**, because silence here would
turn a missing feature into a wrong answer: `{% raw %}{{ site.posts | where_exp: "p", "..." }}{% endraw %}`
would quietly return every post.

Note that `sample` is irreproducible in Jekyll too — it calls Ruby's unseeded
`Array#sample`.

### Everything else

Drafts (`_drafts`), pagination, `site.related_posts`, CoffeeScript, TOML
config, non-YAML data files, incremental builds, and `serve`/`watch`.

Unknown filters that Jekyll *also* lacks — plugin filters like
`jekyll-seo-tag`'s — pass through silently and correctly, because that is what
plugin-less Jekyll does with `strict_filters: false`.
