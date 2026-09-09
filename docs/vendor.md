---
title: Vendored Liquid
summary: Liquid is a dependency, not a rewrite — but seven changes to liquid-core were necessary.
---

The brief said not to rebuild Liquid if it already exists, and rekyll doesn't:
it uses the `liquid` crate. But byte-identical output needed seven changes to
`liquid-core` 0.26.11, kept in `vendor/liquid-core` and wired in through
`[patch.crates-io]`. Each is marked with a `rekyll:` comment explaining why.

## 1. `Object` uses `IndexMap`

Upstream backs `Object` with `std::HashMap`, whose iteration order is
arbitrary and randomly seeded per process. Jekyll exposes Ruby hashes whose
insertion order is observable through `{% raw %}{% for %}{% endraw %}` —
`site.tags`, `site.categories`, `site.data`. Without this, the same input gave
different output on consecutive runs. See
[the determinism note](/notes/output-was-not-deterministic/).

## 2. `shift_remove` instead of `remove`

`IndexMap::remove` is `swap_remove`, which reorders the map that change 1
exists to preserve.

## 3. `TagTokenIter::raw_markup()`

Exposes a tag's arguments exactly as written.

## 4. An `UnstructuredToken` grammar fallback

Jekyll's tags do not follow Liquid's argument grammar —
`{% raw %}{% include nav/menu.html a="b" %}{% endraw %}` has no colons, and
`{% raw %}{% post_url 2020-01-01-name %}{% endraw %}` is a bare path. The pest
grammar rejected them before any tag code ran. The fallback is tried only
after every real production fails, so tags that do use Liquid syntax are
unaffected; they now report their own error rather than a parse error.

## 5. Unknown filters pass their input through

Jekyll runs with `strict_filters: false`, where an unknown filter returns its
input unchanged. Raising instead meant any site written for a plugin rekyll
does not implement failed to build rather than rendering without it.

Unknown *variables* needed no patch: `src/lax.rs` wraps the payload in a value
tree whose lookups always succeed with nil, matching `strict_variables: false`.

## 6. Ruby float formatting

Ruby's `Float#to_s` always keeps a decimal point, so
`{% raw %}{{ 1.5 | plus: 1.5 }}{% endraw %}` renders `3.0` in Jekyll and `3`
in liquid-rust. Values reach templates through `ValueView`, so the formatting
belongs there.

## 7. Borrowed variable lookup

`Runtime::get` called `into_owned()` on a `ValueCow` that `find()` had already
returned borrowed. Every `{% raw %}{{ site.posts | size }}{% endraw %}`
therefore deep-cloned every post. This was one of two causes of a large site
building 16× slower than Jekyll.
