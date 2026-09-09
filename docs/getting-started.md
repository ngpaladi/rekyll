---
title: Getting started
---

Let's build a site from an empty folder. If you already have a Jekyll site,
skip to the end: you just run `rekyll build` in it.

## 1. Install

```
git clone <this repo> rekyll && cd rekyll
cargo install --path . --locked
```

You need Rust 1.94 or newer. Keep the `--locked`, or cargo re-resolves the
dependencies and picks a `kstring` that wants a newer compiler. When it's
done you've got one binary, `rekyll`, and nothing else to install.

## 2. Make a folder and a config

```
mkdir notes && cd notes
```

Put this in `_config.yml`:

```
title: Field Notes
timezone: America/New_York
permalink: pretty
```

`timezone` matters more than it looks. Dates in your posts get resolved in
this zone, and the date decides the URL. `permalink: pretty` gives you
`/2026/09/01/hello/` instead of `/2026/09/01/hello.html`.

## 3. A layout

Layouts live in `_layouts`. Put this in `_layouts/default.html`:

```
{% raw %}<!DOCTYPE html>
<html>
<head><title>{{ page.title }} - {{ site.title }}</title></head>
<body>
{% include nav.html %}
{{ content }}
</body>
</html>{% endraw %}
```

`{% raw %}{{ content }}{% endraw %}` is where the page goes. The include
comes from `_includes/nav.html`:

```
{% raw %}<nav>{% for p in site.pages %}<a href="{{ p.url | relative_url }}">{{ p.title }}</a> {% endfor %}</nav>{% endraw %}
```

Anything in `site.*` is Jekyll's view of your whole site: `site.pages`,
`site.posts`, `site.data`, and everything from `_config.yml` (so `site.title`
is "Field Notes").

## 4. Two pages

A page is any file with front matter (the `---` block at the top). `index.md`:

```
{% raw %}---
title: Home
layout: default
---
# {{ site.title }}

{% for post in site.posts %}
- [{{ post.title }}]({{ post.url }}) &mdash; {{ post.date | date: "%b %-d, %Y" }}
{% endfor %}{% endraw %}
```

and `about.md`:

```
---
title: About
layout: default
---
Written with **rekyll**.
```

Liquid runs first, then Markdown, then the layout. So the `for` loop above
writes out Markdown list items, and kramdown turns them into a `<ul>`.

## 5. A post

Posts go in `_posts` and the filename has to start with a date:
`_posts/2026-09-01-hello.md`.

```
---
title: Hello
layout: default
date: 2026-09-01 22:30:00
---
First post. It's 10:30 at night in New York.
```

Here's the timezone thing. There's no zone on that `date:`, and Ruby's YAML
parser reads an unzoned time as UTC. Jekyll then converts it to your site's
zone, so this post's `date` is actually 6:30 pm Eastern. It's still September
1st, so the URL doesn't move, but a post at `01:00:00` would land on the
previous day. Write `2026-09-01 22:30:00 -0400` if you mean New York time.
rekyll does exactly what Jekyll does here, which is the whole point, but I'd
rather you didn't find out the way I did.

## 6. Build it

```
rekyll build
```

You get a `_site` folder:

```
_site/2026/09/01/hello/index.html
_site/about/index.html
_site/index.html
```

and `_site/index.html` looks like this:

```
<!DOCTYPE html>
<html>
<head><title>Home - Field Notes</title></head>
<body>
<nav><a href="/about/">About</a> <a href="/">Home</a> </nav>

<h1 id="field-notes">Field Notes</h1>

<ul>
  <li><a href="/2026/09/01/hello/">Hello</a> — Sep 1, 2026</li>
</ul>


</body>
</html>
```

Notice the two blank lines before `</body>` and the `id` on the heading.
Those aren't rekyll being sloppy; that's byte for byte what Jekyll writes,
and the whole test suite exists to keep it that way. Also notice the nav
lists About before Home: `site.pages` is sorted by filename, and `about.md`
sorts before `index.md`.

## 7. Look at it

```
rekyll serve
```

Open `http://127.0.0.1:4000`. Edit a file, save, and the page reloads itself.
`-P 8080` changes the port, `-H 0.0.0.0` binds every interface (but this is a
preview server, not something to put on the internet).

## 8. Add some data

Make `_data/links.yml`:

```
- name: rekyll
  url: https://github.com/ngpaladi/rekyll
```

and it's there as `site.data.links` in any template. Same for
`_data/authors/me.yml`, which becomes `site.data.authors.me`.

## 9. Add a stylesheet

`assets/css/main.scss` with two lines of front matter at the top (they can be
empty; they're what marks it as something to process):

```
---
---
@import "base";
```

`_sass/_base.scss` holds the actual styles. The output is
`assets/css/main.css`, in libsass's compact style, one rule per line.

## Already have a Jekyll site?

```
cd my-site
rekyll build
diff -r _site /path/to/jekyll/_site
```

If the diff is empty, you're done. If it isn't, check [Limits](/limits/)
first: gem themes (`theme: minima` in your config) are the usual reason,
and highlighted code blocks are the next one.
