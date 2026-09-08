//! Jekyll's custom Liquid tags.
//!
//! These do not follow Liquid's argument grammar — `{% include f.html a="b" %}`
//! has no colons, `{% link path %}` takes a bare path — so each parses the raw
//! tag markup itself, the way Jekyll's own tag classes do.

use liquid_core::runtime::StackFrame;
use liquid_core::{
    Error, Language, ParseTag, Renderable, Result, Runtime, TagReflection, TagTokenIter, ValueView,
};
use regex::Regex;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, OnceLock};

/// Maps a source-relative path to the URL it renders at, for `link` and
/// `post_url`. Built after reading, before rendering.
pub type UrlIndex = HashMap<String, String>;

fn valid_syntax() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"([\w-]+)\s*=\s*(?:"([^"\\]*(?:\\.[^"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)'|([\w.-]+))"#)
            .unwrap()
    })
}

#[derive(Debug, Clone)]
enum Param {
    Literal(String),
    /// A bare word is looked up in the current scope, as Jekyll's
    /// `context[variable]` does.
    Variable(String),
}

#[derive(Debug)]
struct IncludeArgs {
    file: String,
    params: Vec<(String, Param)>,
}

fn parse_include_markup(markup: &str) -> IncludeArgs {
    let markup = markup.trim();
    let (file, rest) = match markup.find(char::is_whitespace) {
        Some(i) => (markup[..i].to_string(), markup[i..].trim().to_string()),
        None => (markup.to_string(), String::new()),
    };

    let mut params = Vec::new();
    for caps in valid_syntax().captures_iter(&rest) {
        let key = caps[1].to_string();
        let value = if let Some(m) = caps.get(2) {
            Param::Literal(m.as_str().replace("\\\"", "\""))
        } else if let Some(m) = caps.get(3) {
            Param::Literal(m.as_str().replace("\\'", "'"))
        } else if let Some(m) = caps.get(4) {
            Param::Variable(m.as_str().to_string())
        } else {
            Param::Literal(String::new())
        };
        params.push((key, value));
    }
    IncludeArgs { file, params }
}

// -- include ----------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct IncludeTag {
    /// `include_relative` resolves against the including file's directory;
    /// plain `include` against `_includes`.
    relative: bool,
}

impl IncludeTag {
    pub fn new() -> Self {
        IncludeTag { relative: false }
    }
    pub fn relative() -> Self {
        IncludeTag { relative: true }
    }
}

impl TagReflection for IncludeTag {
    fn tag(&self) -> &'static str {
        if self.relative {
            "include_relative"
        } else {
            "include"
        }
    }
    fn description(&self) -> &'static str {
        "Includes a partial from _includes."
    }
}

impl ParseTag for IncludeTag {
    fn parse(&self, arguments: TagTokenIter<'_>, _options: &Language) -> Result<Box<dyn Renderable>> {
        let args = parse_include_markup(arguments.raw_markup());
        Ok(Box::new(Include { args, relative: self.relative }))
    }

    fn reflection(&self) -> &dyn TagReflection {
        self
    }
}

#[derive(Debug)]
struct Include {
    args: IncludeArgs,
    relative: bool,
}

impl Renderable for Include {
    fn render_to(&self, writer: &mut dyn Write, runtime: &dyn Runtime) -> Result<()> {
        // Build the `include` object the partial reads its parameters from.
        let mut include_obj = liquid_core::model::Object::new();
        for (key, value) in &self.args.params {
            let v = match value {
                Param::Literal(s) => liquid_core::model::Value::scalar(s.clone()),
                Param::Variable(name) => evaluate_bare(runtime, name),
            };
            include_obj.insert(key.clone().into(), v);
        }

        let mut scope_vars: HashMap<liquid_core::model::KStringRef<'_>, &dyn ValueView> =
            HashMap::new();
        let include_value = liquid_core::model::Value::Object(include_obj);
        scope_vars.insert("include".into(), &include_value as &dyn ValueView);
        let scope = StackFrame::new(runtime, &scope_vars);

        let key = if self.relative {
            format!("__relative__/{}", self.args.file)
        } else {
            self.args.file.clone()
        };

        match scope.partials().try_get(&key) {
            Some(partial) => partial.render_to(writer, &scope),
            None => Error::with_msg("Could not locate the included file")
                .context("file", self.args.file.clone())
                .into_err(),
        }
    }
}

/// Jekyll evaluates an unquoted include parameter as `context[variable]`, so
/// `n=3` is the number 3 and `x=page.title` walks the variable path.
fn evaluate_bare(runtime: &dyn Runtime, name: &str) -> liquid_core::model::Value {
    use liquid_core::model::{Scalar, Value};
    if let Ok(i) = name.parse::<i64>() {
        return Value::scalar(i);
    }
    if let Ok(f) = name.parse::<f64>() {
        return Value::scalar(f);
    }
    match name {
        "true" => return Value::scalar(true),
        "false" => return Value::scalar(false),
        "nil" | "null" => return Value::Nil,
        _ => {}
    }
    let path: Vec<Scalar> = name.split('.').map(|p| Scalar::new(p.to_owned())).collect();
    runtime.try_get(&path).map(|v| v.into_owned()).unwrap_or(Value::Nil)
}

// -- link and post_url ------------------------------------------------------

#[derive(Clone, Debug)]
pub struct LinkTag {
    urls: Arc<UrlIndex>,
    /// `post_url` matches by post basename rather than exact relative path.
    post: bool,
    /// Both tags emit `relative_url(item)`, so the baseurl is prepended.
    baseurl: Arc<String>,
}

impl LinkTag {
    pub fn new(urls: Arc<UrlIndex>, baseurl: Arc<String>) -> Self {
        LinkTag { urls, post: false, baseurl }
    }
    pub fn post_url(urls: Arc<UrlIndex>, baseurl: Arc<String>) -> Self {
        LinkTag { urls, post: true, baseurl }
    }
}

impl TagReflection for LinkTag {
    fn tag(&self) -> &'static str {
        if self.post {
            "post_url"
        } else {
            "link"
        }
    }
    fn description(&self) -> &'static str {
        "Resolves a source path to its output URL."
    }
}

impl ParseTag for LinkTag {
    fn parse(&self, arguments: TagTokenIter<'_>, _options: &Language) -> Result<Box<dyn Renderable>> {
        let target = arguments.raw_markup().trim().trim_matches(['"', '\'']).to_string();
        Ok(Box::new(Link {
            target,
            urls: self.urls.clone(),
            post: self.post,
            baseurl: self.baseurl.clone(),
        }))
    }

    fn reflection(&self) -> &dyn TagReflection {
        self
    }
}

#[derive(Debug)]
struct Link {
    target: String,
    urls: Arc<UrlIndex>,
    post: bool,
    baseurl: Arc<String>,
}

impl Renderable for Link {
    fn render_to(&self, writer: &mut dyn Write, _runtime: &dyn Runtime) -> Result<()> {
        let found = if self.post {
            // `{% post_url 2020-01-01-name %}` names a post without its
            // directory or extension.
            self.urls.iter().find_map(|(path, url)| {
                let base = path.rsplit('/').next().unwrap_or(path);
                let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
                (stem == self.target && path.contains("_posts/")).then(|| url.clone())
            })
        } else {
            self.urls.get(&self.target).cloned()
        };

        match found {
            Some(url) => {
                let url = crate::urlfilters::relative_url(&url, &self.baseurl);
                write!(writer, "{url}").map_err(|e| Error::with_msg(e.to_string()))?;
                Ok(())
            }
            None => Error::with_msg(if self.post {
                "Could not find post"
            } else {
                "Could not find document"
            })
            .context("target", self.target.clone())
            .into_err(),
        }
    }
}

// -- highlight --------------------------------------------------------------

/// `{% highlight lang [linenos] %}…{% endhighlight %}`.
///
/// Jekyll passes the block to Rouge and wraps the result in a `<figure>`.
/// rekyll reproduces the wrapper exactly and escapes the code; it does not
/// emit Rouge's per-token `<span>`s, which would mean porting Rouge's lexers.
/// For `text`/`plaintext` — Rouge's pass-through lexer — the output is
/// identical.
#[derive(Clone, Debug, Default)]
pub struct HighlightTag;

impl HighlightTag {
    pub fn new() -> Self {
        HighlightTag
    }
}

impl liquid_core::BlockReflection for HighlightTag {
    fn start_tag(&self) -> &str {
        "highlight"
    }
    fn end_tag(&self) -> &str {
        "endhighlight"
    }
    fn description(&self) -> &str {
        "Syntax-highlights a block of code."
    }
}

impl liquid_core::ParseBlock for HighlightTag {
    fn parse(
        &self,
        arguments: TagTokenIter<'_>,
        mut tokens: liquid_core::TagBlock<'_, '_>,
        _options: &Language,
    ) -> Result<Box<dyn Renderable>> {
        let markup = arguments.raw_markup().trim().to_string();
        let mut parts = markup.split_whitespace();
        let lang = parts.next().unwrap_or("").to_string();
        let linenos = parts.any(|p| p == "linenos");

        // The block body is code, so Liquid must not interpret it.
        let content = tokens.escape_liquid(false)?.to_owned();
        tokens.assert_empty();

        Ok(Box::new(Highlight { lang, linenos, content }))
    }

    fn reflection(&self) -> &dyn liquid_core::BlockReflection {
        self
    }
}

#[derive(Debug)]
struct Highlight {
    lang: String,
    linenos: bool,
    content: String,
}

impl Renderable for Highlight {
    fn render_to(&self, writer: &mut dyn Write, _runtime: &dyn Runtime) -> Result<()> {
        // Jekyll strips the surrounding blank lines before highlighting.
        let code = self.content.trim_matches('\n');
        let escaped = code
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");

        let lang_attrs = if self.lang.is_empty() {
            String::new()
        } else {
            format!(
                " class=\"language-{}\" data-lang=\"{}\"",
                self.lang.replace('"', "&quot;"),
                self.lang.replace('"', "&quot;")
            )
        };

        let body = if self.linenos {
            let lines: Vec<&str> = escaped.split('\n').collect();
            let gutter: String =
                (1..=lines.len()).map(|n| format!("{n}\n")).collect::<Vec<_>>().join("");
            format!(
                "<table class=\"rouge-table\"><tbody><tr>\
                 <td class=\"gutter gl\"><pre class=\"lineno\">{gutter}</pre></td>\
                 <td class=\"code\"><pre>{escaped}\n</pre></td></tr></tbody></table>"
            )
        } else {
            escaped
        };

        write!(
            writer,
            "<figure class=\"highlight\"><pre><code{lang_attrs}>{body}</code></pre></figure>"
        )
        .map_err(|e| Error::with_msg(e.to_string()))?;
        Ok(())
    }
}
