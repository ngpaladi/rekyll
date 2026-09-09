---
title: Limits
---

## Not supported

- **Gem themes.** A site with `theme: minima` builds without erroring and without layouts. rekyll prints a build warning per missing layout. `jekyll new --blank` has no theme and works.
- **Plugins.** No `_plugins`, no gem plugins.
- **Drafts.** `_drafts` and `--drafts` are ignored.
- **Pagination.** No `paginate`, no `paginator`.
- **Watch and livereload.** `rekyll serve` serves a build but never rebuilds. Re-run it after editing.
- **Incremental builds.** Every build is a full build.
- **TOML config**, non-YAML data files (`.csv`, `.json`, `.tsv`), and CoffeeScript.
- **`site.related_posts`.**

## Filters Jekyll has that rekyll does not

`where_exp`, `group_by_exp`, `find_exp`, `sample`, `sassify`, `scssify`

These pass their input through and print a build warning. Without the warning
a missing filter would look like a wrong answer:
`{% raw %}{{ site.posts | where_exp: "p", "..." }}{% endraw %}` would silently
return every post.

`sample` is not reproducible in Jekyll either. It calls Ruby's unseeded
`Array#sample`.

## Known output differences

### Syntax-highlighted code

A fenced block with a language tag is tokenized by Rouge into per-token
`<span>` elements. rekyll emits the same wrapper with the code escaped but not
tokenized.

Unlabelled fenced blocks, indented blocks, inline code and
`{% raw %}{% highlight text %}{% endraw %}` use Rouge's `plaintext` lexer and
are byte-identical.

### Kramdown-only Markdown syntax

Not implemented: inline attribute lists (`{:.class}`), definition lists,
abbreviations, math.

Inside a table cell, kramdown keeps a backslash-escaped pipe literally when it
sits in a code span, so `` `x \| y` `` stays `x \| y`. GFM unescapes it to
`x | y`. In plain table text both unescape.

Kramdown merges two adjacent lists separated by a blank line even when they
use different bullet markers or a different ordinal start. rekyll follows
CommonMark and keeps them separate.

### Sass source maps

Jekyll writes a `.css.map` and appends a `sourceMappingURL` comment. rekyll
writes neither. Set `sass: {sourcemap: never}` and the CSS matches exactly.

### Page order for pages sharing a basename

Jekyll sorts `site.pages` with `sort_by!(&:name)`, an unstable sort, over
entries `Dir.entries` returned in filesystem order. Two pages named
`index.html` in different directories come out in an order set by directory
inode layout, so the same commit cloned to another path gives a different
order in Jekyll itself. rekyll sorts stably. Pages with distinct basenames
match exactly.
