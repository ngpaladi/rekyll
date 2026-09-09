//! Jekyll's Liquid filters, plus overrides where Ruby's Liquid differs from
//! liquid-rust's stdlib.
//!
//! The divergences are small but visible: Ruby's `escape` emits `&#39;` where
//! liquid-rust emits `&#x27;`, `url_encode` maps a space to `+`, `capitalize`
//! downcases the remainder, `split` drops trailing empty fields, integer
//! division stays integral, and every float renders with a decimal point.

use crate::time::RTime;
use crate::value::Value as RValue;
use chrono_tz::Tz;
use liquid_core::parser::{FilterArguments, ParameterReflection};
use liquid_core::{
    Error, Expression, Filter, FilterReflection, ParseFilter, Result, Runtime, Value, ValueView,
};
use std::fmt;
use std::sync::Arc;

/// Site-level facts the filters need at render time.
pub struct FilterCtx {
    pub baseurl: String,
    pub url: String,
    pub timezone: Tz,
    pub smart_quotes: bool,
    pub site_time: RTime,
}

type FilterFn = fn(&dyn ValueView, &[Value], &FilterCtx) -> Result<Value>;

/// A filter defined by a name and a function, so the whole set can be
/// registered without a derive per filter.
#[derive(Clone)]
pub struct JekyllFilter {
    name: &'static str,
    func: FilterFn,
    ctx: Arc<FilterCtx>,
}

impl JekyllFilter {
    pub fn new(name: &'static str, func: FilterFn, ctx: Arc<FilterCtx>) -> Self {
        JekyllFilter { name, func, ctx }
    }
}

impl FilterReflection for JekyllFilter {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        ""
    }
    fn positional_parameters(&self) -> &'static [ParameterReflection] {
        &[]
    }
    fn keyword_parameters(&self) -> &'static [ParameterReflection] {
        &[]
    }
}

impl ParseFilter for JekyllFilter {
    fn parse(&self, arguments: FilterArguments) -> Result<Box<dyn Filter>> {
        let args: Vec<Expression> = arguments.positional.collect();
        Ok(Box::new(BoundFilter {
            name: self.name,
            func: self.func,
            ctx: self.ctx.clone(),
            args,
        }))
    }

    fn reflection(&self) -> &dyn FilterReflection {
        self
    }
}

struct BoundFilter {
    name: &'static str,
    func: FilterFn,
    ctx: Arc<FilterCtx>,
    args: Vec<Expression>,
}

impl fmt::Debug for BoundFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl fmt::Display for BoundFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl Filter for BoundFilter {
    fn evaluate(&self, input: &dyn ValueView, runtime: &dyn Runtime) -> Result<Value> {
        let mut args = Vec::with_capacity(self.args.len());
        for expr in &self.args {
            args.push(expr.evaluate(runtime)?.into_owned());
        }
        (self.func)(input, &args, &self.ctx)
    }
}

/// Every filter rekyll defines or overrides.
pub fn all() -> Vec<(&'static str, FilterFn)> {
    vec![
        // Jekyll's own filters.
        ("slugify", f_slugify),
        ("xml_escape", f_xml_escape),
        ("cgi_escape", f_cgi_escape),
        ("uri_escape", f_uri_escape),
        ("number_of_words", f_number_of_words),
        ("array_to_sentence_string", f_array_to_sentence_string),
        ("jsonify", f_jsonify),
        ("to_integer", f_to_integer),
        ("inspect", f_inspect),
        ("normalize_whitespace", f_normalize_whitespace),
        ("markdownify", f_markdownify),
        ("smartify", f_smartify),
        ("date_to_string", f_date_to_string),
        ("date_to_long_string", f_date_to_long_string),
        ("date_to_xmlschema", f_date_to_xmlschema),
        ("date_to_rfc822", f_date_to_rfc822),
        ("relative_url", f_relative_url),
        ("absolute_url", f_absolute_url),
        ("strip_index", f_strip_index),
        ("push", f_push),
        ("pop", f_pop),
        ("shift", f_shift),
        ("unshift", f_unshift),
        ("where", f_where),
        ("group_by", f_group_by),
        ("find", f_find),
        // Overrides of stdlib filters whose Ruby behaviour differs.
        ("date", f_date),
        ("escape", f_escape),
        ("escape_once", f_escape_once),
        ("url_encode", f_url_encode),
        ("url_decode", f_url_decode),
        ("capitalize", f_capitalize),
        ("split", f_split),
        ("divided_by", f_divided_by),
        ("sort", f_sort),
        ("map", f_map),
        // Jekyll filters rekyll does not implement. They are registered so
        // they warn rather than silently passing through: the unknown-filter
        // passthrough exists for filters plugin-less Jekyll also lacks, and
        // applying it to filters Jekyll *does* have would turn a missing
        // feature into a wrong answer. `{{ posts | where_exp: ... }}` would
        // quietly return every post.
        ("where_exp", f_unimplemented),
        ("group_by_exp", f_unimplemented),
        ("find_exp", f_unimplemented),
        ("sample", f_unimplemented),
        ("sassify", f_unimplemented),
        ("scssify", f_unimplemented),
    ]
}

/// Pass the input through, but say so on stderr the first time, once per
/// filter name.
fn f_unimplemented(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    use std::sync::Mutex;
    static WARNED: Mutex<Option<std::collections::HashSet<&'static str>>> = Mutex::new(None);
    if let Ok(mut guard) = WARNED.lock() {
        let seen = guard.get_or_insert_with(std::collections::HashSet::new);
        if seen.insert("unimplemented") {
            eprintln!(
                "       Build Warning: a Jekyll filter rekyll does not implement was used \
                 (where_exp, group_by_exp, find_exp, sample, sassify or scssify). Its input \
                 was passed through unchanged, so the output differs from Jekyll's."
            );
        }
    }
    Ok(input.to_value())
}

// -- helpers ----------------------------------------------------------------

fn s(input: &dyn ValueView) -> String {
    input.to_kstr().into_owned().into_string()
}

fn arg_str(args: &[Value], i: usize) -> Option<String> {
    args.get(i).map(|v| v.to_kstr().into_owned().into_string())
}

fn array_of(input: &dyn ValueView) -> Vec<Value> {
    match input.as_array() {
        Some(a) => a.values().map(|v| v.to_value()).collect(),
        None => vec![input.to_value()],
    }
}

/// Ruby renders floats with a decimal point, so `3.0` never prints as `3`.
fn number_to_value(x: f64) -> Value {
    Value::scalar(x)
}

fn to_rvalue(v: &dyn ValueView) -> RValue {
    crate::liquid_bridge::from_liquid(&v.to_value())
}

// -- Jekyll filters ---------------------------------------------------------

fn f_slugify(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let mode = arg_str(args, 0).unwrap_or_else(|| "default".into());
    Ok(Value::scalar(crate::slug::slugify(&s(input), &mode, false)))
}

fn f_xml_escape(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    // Ruby's CGI.escapeHTML.
    Ok(Value::scalar(
        s(input)
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;"),
    ))
}

fn f_escape(input: &dyn ValueView, a: &[Value], c: &FilterCtx) -> Result<Value> {
    f_xml_escape(input, a, c)
}

/// `escape_once` leaves existing entity references alone.
fn f_escape_once(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    let text = s(input);
    let re = regex::Regex::new(r"&(?:[a-zA-Z][a-zA-Z0-9]*|#[0-9]+|#[xX][0-9a-fA-F]+);").unwrap();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in re.find_iter(&text) {
        out.push_str(&escape_html_ruby(&text[last..m.start()]));
        out.push_str(m.as_str());
        last = m.end();
    }
    out.push_str(&escape_html_ruby(&text[last..]));
    Ok(Value::scalar(out))
}

fn escape_html_ruby(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// `cgi_escape` / Liquid's `url_encode`: CGI.escape, where a space is `+`.
fn f_cgi_escape(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    Ok(Value::scalar(cgi_escape(&s(input))))
}

fn f_url_encode(input: &dyn ValueView, a: &[Value], c: &FilterCtx) -> Result<Value> {
    f_cgi_escape(input, a, c)
}

fn cgi_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn f_url_decode(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    let text = s(input).replace('+', " ");
    Ok(Value::scalar(
        percent_encoding::percent_decode_str(&text)
            .decode_utf8_lossy()
            .to_string(),
    ))
}

/// `uri_escape`: Addressable's normalize, which leaves sub-delimiters alone.
fn f_uri_escape(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    const KEEP: &str = "!#$&'()*+,-./:;=?@_~";
    let mut out = String::new();
    for ch in s(input).chars() {
        if ch.is_ascii_alphanumeric() || KEEP.contains(ch) {
            out.push(ch);
        } else {
            let mut buf = [0u8; 4];
            for b in ch.encode_utf8(&mut buf).as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    Ok(Value::scalar(out))
}

fn f_number_of_words(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    Ok(Value::scalar(s(input).split_whitespace().count() as i64))
}

fn f_array_to_sentence_string(
    input: &dyn ValueView,
    args: &[Value],
    _c: &FilterCtx,
) -> Result<Value> {
    let connector = arg_str(args, 0).unwrap_or_else(|| "and".into());
    let items: Vec<String> = array_of(input)
        .iter()
        .map(|v| v.to_kstr().into_owned().into_string())
        .collect();
    let out = match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} {} {}", items[0], connector, items[1]),
        n => format!("{}, {} {}", items[..n - 1].join(", "), connector, items[n - 1]),
    };
    Ok(Value::scalar(out))
}

fn f_jsonify(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    Ok(Value::scalar(to_json(&to_rvalue(input))))
}

fn to_json(v: &RValue) -> String {
    match v {
        RValue::Null => "null".into(),
        RValue::Bool(b) => b.to_string(),
        RValue::Int(i) => i.to_string(),
        RValue::Float(x) => crate::value::ruby_float_to_s(*x),
        RValue::Str(s) => json_string(s),
        RValue::Date { .. } => json_string(&v.to_string()),
        RValue::Array(a) => {
            format!("[{}]", a.iter().map(to_json).collect::<Vec<_>>().join(","))
        }
        RValue::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, val)| format!("{}:{}", json_string(k), to_json(val)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `to_integer`: Ruby's `to_i`, which truncates and tolerates junk.
fn f_to_integer(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    let v = to_rvalue(input);
    let n = match v {
        RValue::Int(i) => i,
        RValue::Float(x) => x.trunc() as i64,
        RValue::Bool(true) => 1,
        RValue::Bool(false) => 0,
        other => {
            let text = other.to_string();
            leading_number(&text).map(|x| x.trunc() as i64).unwrap_or(0)
        }
    };
    Ok(Value::scalar(n))
}

fn leading_number(s: &str) -> Option<f64> {
    let t = s.trim_start();
    let mut end = 0;
    let mut seen_dot = false;
    for (i, c) in t.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')) {
            end = i + c.len_utf8();
        } else if c == '.' && !seen_dot {
            seen_dot = true;
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    t[..end].parse::<f64>().ok()
}

/// `inspect`: Ruby's `Object#inspect`, then HTML-escaped by Jekyll.
fn f_inspect(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    Ok(Value::scalar(escape_html_ruby(&ruby_inspect(&to_rvalue(input)))))
}

fn ruby_inspect(v: &RValue) -> String {
    match v {
        RValue::Null => "nil".into(),
        RValue::Bool(b) => b.to_string(),
        RValue::Int(i) => i.to_string(),
        RValue::Float(x) => crate::value::ruby_float_to_s(*x),
        RValue::Str(s) => format!("{s:?}"),
        RValue::Date { .. } => v.to_string(),
        RValue::Array(a) => {
            format!("[{}]", a.iter().map(ruby_inspect).collect::<Vec<_>>().join(", "))
        }
        RValue::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, val)| format!("{:?}=>{}", k, ruby_inspect(val)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn f_normalize_whitespace(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    Ok(Value::scalar(s(input).split_whitespace().collect::<Vec<_>>().join(" ")))
}

fn f_markdownify(input: &dyn ValueView, _a: &[Value], c: &FilterCtx) -> Result<Value> {
    Ok(Value::scalar(crate::markdown::convert_opts(&s(input), c.smart_quotes)))
}

fn f_smartify(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    Ok(Value::scalar(crate::markdown::smartify_text(&s(input))))
}

// -- dates ------------------------------------------------------------------

/// Liquid's `to_date` input rules: "now"/"today" is the current time, an
/// integer is a Unix timestamp, and anything else is parsed as a time.
fn to_time(input: &dyn ValueView, c: &FilterCtx) -> Option<RTime> {
    let v = to_rvalue(input);
    match &v {
        RValue::Int(i) => Some(crate::time::in_zone(
            chrono::DateTime::from_timestamp(*i, 0)?.naive_utc(),
            c.timezone,
        )),
        RValue::Null => None,
        _ => {
            let text = v.to_string();
            match text.as_str() {
                // Liquid's `to_date` resolves "now"/"today" against the wall
                // clock, not site.time.
                "now" | "today" => {
                    Some(crate::time::in_zone(chrono::Utc::now().naive_utc(), c.timezone))
                }
                "" => None,
                _ => crate::time::parse_date(&text, c.timezone).ok(),
            }
        }
    }
}

fn f_date(input: &dyn ValueView, args: &[Value], c: &FilterCtx) -> Result<Value> {
    let format = match arg_str(args, 0) {
        Some(f) if !f.is_empty() => f,
        // Liquid returns the input untouched when no format is given.
        _ => return Ok(input.to_value()),
    };
    match to_time(input, c) {
        Some(t) => Ok(Value::scalar(t.format(&format))),
        None => Ok(input.to_value()),
    }
}

fn date_with(input: &dyn ValueView, c: &FilterCtx, fmt: &str) -> Result<Value> {
    match to_time(input, c) {
        Some(t) => Ok(Value::scalar(t.format(fmt))),
        None => Ok(input.to_value()),
    }
}

/// `date_to_string`: "%d %b %Y", or ordinal form when asked.
fn f_date_to_string(input: &dyn ValueView, args: &[Value], c: &FilterCtx) -> Result<Value> {
    date_to_format(input, args, c, "%b")
}

fn f_date_to_long_string(input: &dyn ValueView, args: &[Value], c: &FilterCtx) -> Result<Value> {
    date_to_format(input, args, c, "%B")
}

fn date_to_format(
    input: &dyn ValueView,
    args: &[Value],
    c: &FilterCtx,
    month: &str,
) -> Result<Value> {
    let style = arg_str(args, 0).unwrap_or_default();
    let t = match to_time(input, c) {
        Some(t) => t,
        None => return Ok(input.to_value()),
    };
    let day = if style == "ordinal" {
        ordinal(t.at_day())
    } else {
        t.format("%d")
    };
    Ok(Value::scalar(format!("{} {} {}", day, t.format(month), t.format("%Y"))))
}

fn ordinal(day: u32) -> String {
    let suffix = match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

fn f_date_to_xmlschema(input: &dyn ValueView, _a: &[Value], c: &FilterCtx) -> Result<Value> {
    date_with(input, c, "%Y-%m-%dT%H:%M:%S%:z")
}

fn f_date_to_rfc822(input: &dyn ValueView, _a: &[Value], c: &FilterCtx) -> Result<Value> {
    date_with(input, c, "%a, %d %b %Y %H:%M:%S %z")
}

// -- URLs -------------------------------------------------------------------

fn f_relative_url(input: &dyn ValueView, _a: &[Value], c: &FilterCtx) -> Result<Value> {
    if input.is_nil() {
        return Ok(Value::Nil);
    }
    Ok(Value::scalar(crate::urlfilters::relative_url(&s(input), &c.baseurl)))
}

fn f_absolute_url(input: &dyn ValueView, _a: &[Value], c: &FilterCtx) -> Result<Value> {
    if input.is_nil() {
        return Ok(Value::Nil);
    }
    Ok(Value::scalar(crate::urlfilters::absolute_url(&s(input), &c.url, &c.baseurl)))
}

fn f_strip_index(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    if input.is_nil() {
        return Ok(Value::Nil);
    }
    Ok(Value::scalar(crate::urlfilters::strip_index(&s(input))))
}

// -- arrays -----------------------------------------------------------------

fn f_push(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let mut a = array_of(input);
    if let Some(v) = args.first() {
        a.push(v.clone());
    }
    Ok(Value::Array(a))
}

fn f_pop(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    let mut a = array_of(input);
    a.pop();
    Ok(Value::Array(a))
}

fn f_shift(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    let mut a = array_of(input);
    if !a.is_empty() {
        a.remove(0);
    }
    Ok(Value::Array(a))
}

fn f_unshift(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let mut a = array_of(input);
    if let Some(v) = args.first() {
        a.insert(0, v.clone());
    }
    Ok(Value::Array(a))
}

/// `where`: select items whose property equals the given value, comparing as
/// strings the way Jekyll does.
fn f_where(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let key = arg_str(args, 0).unwrap_or_default();
    let want = args.get(1);
    let out: Vec<Value> = array_of(input)
        .into_iter()
        .filter(|item| property_matches(item, &key, want))
        .collect();
    Ok(Value::Array(out))
}

fn f_find(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let key = arg_str(args, 0).unwrap_or_default();
    let want = args.get(1);
    Ok(array_of(input)
        .into_iter()
        .find(|item| property_matches(item, &key, want))
        .unwrap_or(Value::Nil))
}

fn property_matches(item: &Value, key: &str, want: Option<&Value>) -> bool {
    let got = item.as_object().and_then(|o| o.get(key)).map(|v| v.to_value());
    match (got, want) {
        (Some(g), Some(w)) => {
            // Jekyll compares the rendered forms, so 1 matches "1".
            g.to_kstr() == w.to_kstr()
                || g.as_array().map(|a| a.values().any(|x| x.to_kstr() == w.to_kstr()))
                    == Some(true)
        }
        (Some(g), None) => !g.is_nil(),
        _ => false,
    }
}

/// `group_by`: groups preserve first-seen order, and each group is
/// `{"name" => value, "items" => [...], "size" => n}`.
fn f_group_by(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let key = arg_str(args, 0).unwrap_or_default();
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<Value>> = std::collections::HashMap::new();

    for item in array_of(input) {
        let name = item
            .as_object()
            .and_then(|o| o.get(&key))
            .map(|v| v.to_kstr().into_owned().into_string())
            .unwrap_or_default();
        if !groups.contains_key(&name) {
            order.push(name.clone());
        }
        groups.entry(name).or_default().push(item);
    }

    let out: Vec<Value> = order
        .into_iter()
        .map(|name| {
            let items = groups.remove(&name).unwrap_or_default();
            let mut o = liquid_core::model::Object::new();
            o.insert("name".into(), Value::scalar(name));
            o.insert("size".into(), Value::scalar(items.len() as i64));
            o.insert("items".into(), Value::Array(items));
            Value::Object(o)
        })
        .collect();
    Ok(Value::Array(out))
}

/// `sort`: by a property when given one, else by rendered value. Ruby's sort
/// puts nil first.
fn f_sort(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let key = arg_str(args, 0);
    let mut items = array_of(input);
    items.sort_by(|a, b| match &key {
        None => compare_values(a, b),
        Some(k) => {
            let av = a.as_object().and_then(|o| o.get(k)).map(|v| v.to_value());
            let bv = b.as_object().and_then(|o| o.get(k)).map(|v| v.to_value());
            match (av, bv) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(x), Some(y)) => compare_values(&x, &y),
            }
        }
    });
    Ok(Value::Array(items))
}

fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    let (an, bn) = (a.as_scalar().and_then(|s| s.to_float()), b.as_scalar().and_then(|s| s.to_float()));
    match (an, bn) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.to_kstr().cmp(&b.to_kstr()),
    }
}

/// Ruby's `map` indexes each item with the property. For a String that is
/// `String#[]`, a substring search, so a non-matching property yields nil
/// rather than dropping the item.
fn f_map(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let key = arg_str(args, 0).unwrap_or_default();
    let out: Vec<Value> = array_of(input)
        .into_iter()
        .map(|item| match item.as_object() {
            Some(o) => o.get(&key).map(|v| v.to_value()).unwrap_or(Value::Nil),
            None => match item.as_scalar() {
                // "abc"["b"] is "b"; anything not present is nil.
                Some(sc) if sc.to_kstr().contains(&key) && !key.is_empty() => {
                    Value::scalar(key.clone())
                }
                _ => Value::Nil,
            },
        })
        .collect();
    Ok(Value::Array(out))
}

// -- overrides --------------------------------------------------------------

/// Ruby's `String#capitalize` downcases everything after the first character.
fn f_capitalize(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx) -> Result<Value> {
    let text = s(input);
    let mut chars = text.chars();
    let out = match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    };
    Ok(Value::scalar(out))
}

/// Ruby's `String#split` drops trailing empty fields.
fn f_split(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let text = s(input);
    let sep = arg_str(args, 0).unwrap_or_default();
    let mut parts: Vec<String> = if sep.is_empty() {
        text.chars().map(|c| c.to_string()).collect()
    } else {
        text.split(sep.as_str()).map(str::to_string).collect()
    };
    while parts.last().map(|p| p.is_empty()) == Some(true) {
        parts.pop();
    }
    Ok(Value::Array(parts.into_iter().map(Value::scalar).collect()))
}

/// Integer division stays integral in Ruby; a float on either side makes it
/// floating point.
fn f_divided_by(input: &dyn ValueView, args: &[Value], _c: &FilterCtx) -> Result<Value> {
    let a = to_rvalue(input);
    let b = args.first().map(|v| crate::liquid_bridge::from_liquid(v)).unwrap_or(RValue::Int(1));
    let both_int = matches!(a, RValue::Int(_)) && matches!(b, RValue::Int(_));
    let (x, y) = (numeric(&a), numeric(&b));
    if y == 0.0 {
        return Error::with_msg("divided by 0").into_err();
    }
    if both_int {
        Ok(Value::scalar((x as i64).div_euclid(y as i64)))
    } else {
        Ok(number_to_value(x / y))
    }
}

fn numeric(v: &RValue) -> f64 {
    match v {
        RValue::Int(i) => *i as f64,
        RValue::Float(x) => *x,
        RValue::Str(s) => leading_number(s).unwrap_or(0.0),
        RValue::Bool(true) => 1.0,
        _ => 0.0,
    }
}
