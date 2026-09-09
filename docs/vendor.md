---
title: Liquid patches
---

rekyll uses the `liquid` crate instead of reimplementing Liquid, but getting
byte-identical output needed seven changes to `liquid-core` 0.26.11. The
source of truth is `vendor/rekyll-liquid-core.patch`, about 250 lines, and
each change is marked with a `rekyll:` comment saying why it's there.
`vendor/liquid-core` is the patched copy, checked in so builds work offline
and wired in through `[patch.crates-io]` in `Cargo.toml`. To rebuild it from
the pristine crates.io tarball plus the patch:

```
./scripts/vendor.sh
```

`git status` should come back clean afterwards; if it doesn't, the patch and
the copy have drifted.

| # | change | why |
|---|--------|-----|
| 1 | `Object` uses `IndexMap` instead of `HashMap` | `HashMap` order is arbitrary and seeded randomly per process. Ruby hashes keep insertion order, and your templates can see it any time you loop over `site.tags`, `site.categories` or `site.data`. Without this, the same input gave different output on consecutive runs. |
| 2 | `shift_remove` instead of `remove` | `IndexMap::remove` is `swap_remove`, which reorders the map change 1 exists to preserve. |
| 3 | `TagTokenIter::raw_markup()` | Exposes a tag's arguments as written, so a tag can parse its own syntax. |
| 4 | `UnstructuredToken` grammar fallback | Jekyll's tags do not follow Liquid's argument grammar. `{% raw %}{% include nav/menu.html a="b" %}{% endraw %}` has no colons and `{% raw %}{% post_url 2020-01-01-name %}{% endraw %}` is a bare path, so the pest grammar rejected them before any tag code ran. Tried only after every real production fails. |
| 5 | Unknown filters pass input through | Jekyll runs with `strict_filters: false`. Erroring instead meant a site written for a plugin failed to build rather than just rendering without it. |
| 6 | Ruby float formatting | `Float#to_s` keeps a decimal point, so `plus` on two floats is `3.0`, not `3`. |
| 7 | Borrowed variable lookup | `Runtime::get` called `into_owned()` on a `ValueCow` that `find()` had already handed back borrowed, so every `{% raw %}{{ site.posts }}{% endraw %}` deep-cloned every post you have. |

Unknown variables didn't need a patch. `src/lax.rs` wraps the payload in a
value tree where every lookup succeeds and returns nil if there's nothing
there, which is what `strict_variables: false` gets you.
