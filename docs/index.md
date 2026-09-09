---
title: rekyll
summary: A from-scratch Jekyll reimplementation in Rust that produces byte-identical output from identical input.
---

This site is built by rekyll, from the sources in `docs/`. It is also
[a test fixture](/testing/): the same tree is built by real Jekyll on every
run and the two `_site` trees are compared byte for byte. If they ever differ,
the build fails.

## The guarantee, stated precisely

**Identical input produces identical output**, byte for byte, against Jekyll
4.3.2 — for the feature set listed under [Fidelity](/fidelity/). rekyll is not
plugin-compatible. It is *output*-compatible.

{% include verdict.html state="identical" text="10 site fixtures, a Markdown corpus against kramdown 2.4, and ~100 filter expressions against Ruby Liquid 5.4." %}

Correctness here is measured rather than asserted. Jekyll is installed
alongside, and every layer is diffed against the real implementation instead of
against what the implementation was assumed to do. That distinction found
things that were not guessable:

- Ruby's YAML is **1.1**, so `yes` is a boolean and `010` is octal — but
  `1e3` is a *string*.
- The first working version was **not deterministic**: it produced different
  output from the same input on consecutive runs.
- Under a non-UTC timezone, an unzoned `date:` **shifts the permalink** by a
  day.

Each has a note below.

## Using it

```
cargo install --path . --locked
rekyll build -s path/to/site -d path/to/_site
```

One self-contained executable. No Ruby, no gems, nothing to install alongside
it.

## Speed

Same machine, same generated site, identical output at both sizes.

| posts | Jekyll 4.3.2 | rekyll |
|-------|--------------|--------|
| 302   | 0.58s        | 0.17s  |
| 1202  | 1.43s        | 0.66s  |

The first version was 16× *slower* than Jekyll. Profiling, not intuition,
found why.

## Notes

{% for post in site.posts %}
- [{{ post.title }}]({{ post.url | relative_url }}) — {{ post.summary }}
{% endfor %}
