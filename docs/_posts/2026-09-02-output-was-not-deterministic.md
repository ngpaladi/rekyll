---
title: Output was not deterministic
summary: The same input produced different output on consecutive runs.
tags: [determinism, liquid]
---

The requirement is *identical output from identical input*. Early on, rekyll
failed that against itself:

```
cats: blog(1) news(1) updates(1)
cats: updates(1) blog(1) news(1)
cats: news(1) updates(1) blog(1)
```

Three consecutive builds, same source.

The cause was not in rekyll. `liquid-core` backs its `Object` with
`std::collections::HashMap`, whose iteration order is arbitrary and seeded
randomly *per process*. Jekyll exposes Ruby hashes, and Ruby hashes preserve
insertion order — which templates observe directly:

```
{% raw %}{% for tag in site.tags %}{{ tag[0] }}{% endfor %}{% endraw %}
```

There is no configuration for this; the map type is hardcoded. So
`vendor/liquid-core` swaps it for `IndexMap`, and a second change follows
immediately: `IndexMap::remove` is `swap_remove`, which would reorder the map
the first change exists to preserve.

The lesson worth keeping: this was found by running the same build five times
and diffing, not by a test. A correctness suite that runs each case once
cannot see it.
