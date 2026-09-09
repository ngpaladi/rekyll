---
title: A timezone can move a post to the wrong day
summary: Under America/New_York, an unzoned date publishes a day earlier.
tags: [dates, fidelity]
---

Every fixture used `timezone: UTC` for a long time. That hid an entire class
of bug.

Jekyll normalises document dates through `Utils.parse_date`, which is
`Time.parse(input).localtime`. Psych has already read an *unzoned* front-matter
timestamp as a UTC instant. `localtime` then re-expresses that instant in the
site's timezone — which changes the wall-clock date.

With `timezone: America/New_York`, this front matter:

```
---
title: Unzoned
date: 2020-01-02 03:04:05
---
```

publishes at:

```
/2020/01/01/unzoned.html
```

A day earlier than written, because 03:04 UTC is 22:04 the previous evening in
New York. The date-based permalink follows the shifted date, so the *file lands
somewhere else on disk*.

A generator that resolved the timestamp in the site's timezone directly — the
obvious implementation — would put the post on January 2nd, and no UTC test
would ever catch it. The fixture that covers this also checks a zoned date, a
filename-only date, a daylight-saving gap, `%Z` abbreviations, and
`date_to_xmlschema`.
