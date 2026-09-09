---
title: Limits
---

Here's what rekyll won't do, and where its output differs from Jekyll's on
purpose.

## Not Supported

**Gem themes.** If your `_config.yml` says `theme: minima`, the layouts live
inside a gem and rekyll can't see them. It won't error, it'll just build your
pages without layouts and print a warning for each missing one. That's worse
than failing outright, so it's worth knowing before you point it at a real
site. A `jekyll new --blank` site has no theme and works fine.

**Plugins**, either from `_plugins` or from gems.

**Drafts.** `_drafts` and `--drafts` are ignored.

**Pagination.** No `paginate`, no `paginator`.

**Watch and livereload.** `rekyll serve` will serve a build but it never
rebuilds. Re-run it after you edit something.

**Incremental builds.** Every build is a full build.

**TOML config**, data files that aren't YAML (`.csv`, `.json`, `.tsv`),
CoffeeScript, and `site.related_posts`.

## Filters Jekyll Has That rekyll Doesn't

`where_exp`, `group_by_exp`, `find_exp`, `sample`, `sassify`, `scssify`

These pass their input straight through and print a build warning. The warning
matters, because without it a missing filter looks like a wrong answer instead
of a missing feature: `{% raw %}{{ site.posts | where_exp: "p", "..." }}{% endraw %}`
would quietly hand you every post.

`sample` isn't reproducible in Jekyll either, since it calls Ruby's unseeded
`Array#sample`.

## Output That Differs On Purpose

### Highlighted Code

Put a language on a fenced block and Jekyll runs it through Rouge, which wraps
every token in its own `<span>`. Matching that means porting Rouge's lexers,
one per language, so rekyll emits the identical wrapper with the code escaped
but not tokenized.

Fenced blocks with no language, indented blocks, inline code and
`{% raw %}{% highlight text %}{% endraw %}` all use Rouge's `plaintext` lexer,
which does exactly that anyway, so those come out identical.

### Kramdown-Only Markdown

Inline attribute lists (`{:.class}`), definition lists, abbreviations and math
aren't implemented.

Kramdown also merges two adjacent lists separated by a blank line even when
they use different bullet markers or start at a different number. rekyll
follows CommonMark and keeps them apart.

Inside a table cell, kramdown keeps a backslash-escaped pipe as-is when it's
in a code span, so `` `x \| y` `` stays `x \| y`, while GFM unescapes it to
`x | y`. In plain table text both unescape it.

### Sass Source Maps

Jekyll writes a `.css.map` next to your stylesheet and appends a
`sourceMappingURL` comment. rekyll writes neither. Set
`sass: {sourcemap: never}` and the CSS matches exactly.

### Page Order When Basenames Collide

Jekyll sorts `site.pages` with `sort_by!(&:name)`, which is an unstable sort,
over entries `Dir.entries` handed back in filesystem order. So if you've got
`sub/index.html` and `other/index.html`, the order you get depends on directory
inode layout. Clone the same commit somewhere else and Jekyll itself gives you
a different order. There's nothing to match there, so rekyll sorts stably and
at least stays put. Pages with different basenames match exactly.
