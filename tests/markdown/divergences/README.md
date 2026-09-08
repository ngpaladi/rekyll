# Known Markdown divergences

Cases where rekyll's output differs from kramdown 2.4 by design. Everything in
`tests/markdown/*.md` is byte-identical; these are not.

## `highlighted-code.md` — Rouge token spans

A fenced block with a language tag is tokenized by Rouge, which emits
`<span class="k">`, `<span class="nb">` and so on per token. Reproducing that
means porting Rouge's lexers, one per language.

rekyll emits the surrounding structure exactly —

    <div class="language-ruby highlighter-rouge"><div class="highlight"><pre
    class="highlight"><code>…</code></pre></div></div>

— with the code HTML-escaped but not tokenized. Blocks with no language tag,
and indented code blocks, use Rouge's `plaintext` lexer, which does exactly
this, so those *are* byte-identical.

## `merged-lists.md` — adjacent list merging

Kramdown merges two lists separated by a blank line into one, even when they
use different bullet markers or a different ordinal start, and then decides
tight/loose per item. CommonMark (and so pulldown-cmark) starts a new list at
a marker change and preserves `start=`. rekyll follows CommonMark.

Lists separated by any other block, and nested, tight and loose lists, match
kramdown exactly.
