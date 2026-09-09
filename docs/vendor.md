---
title: Liquid patches
---

rekyll uses the `liquid` crate rather than reimplementing Liquid. Seven changes
to `liquid-core` 0.26.11 were needed for byte-identical output. They live in
`vendor/liquid-core`, are applied through `[patch.crates-io]`, and each is
marked with a `rekyll:` comment.

| # | change | why |
|---|--------|-----|
| 1 | `Object` uses `IndexMap` instead of `HashMap` | `HashMap` order is arbitrary and randomly seeded per process. Ruby hashes keep insertion order, and templates see it through `{% raw %}{% for %}{% endraw %}` over `site.tags`, `site.categories` and `site.data`. Without this the same input gave different output on consecutive runs. |
| 2 | `shift_remove` instead of `remove` | `IndexMap::remove` is `swap_remove`, which reorders the map change 1 exists to preserve. |
| 3 | `TagTokenIter::raw_markup()` | Exposes a tag's arguments as written, so a tag can parse its own syntax. |
| 4 | `UnstructuredToken` grammar fallback | Jekyll's tags do not follow Liquid's argument grammar. `{% raw %}{% include nav/menu.html a="b" %}{% endraw %}` has no colons and `{% raw %}{% post_url 2020-01-01-name %}{% endraw %}` is a bare path, so the pest grammar rejected them before any tag code ran. Tried only after every real production fails. |
| 5 | Unknown filters pass input through | Jekyll runs with `strict_filters: false`. Raising meant a site written for a plugin failed to build instead of rendering without it. |
| 6 | Ruby float formatting | `Float#to_s` keeps a decimal point, so `plus` on two floats is `3.0`, not `3`. |
| 7 | Borrowed variable lookup | `Runtime::get` called `into_owned()` on a `ValueCow` that `find()` had already returned borrowed, so every `{% raw %}{{ site.posts }}{% endraw %}` deep-cloned every post. |

Unknown variables needed no patch. `src/lax.rs` wraps the payload in a value
tree whose lookups always succeed with nil, matching `strict_variables: false`.
