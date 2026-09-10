//! Jekyll's Liquid filters, plus overrides where Ruby's Liquid differs from
//! liquid-rust's stdlib.
//!
//! The divergences are small but visible: Ruby's `escape` emits `&#39;` where
//! liquid-rust emits `&#x27;`, `url_encode` maps a space to `+`, `capitalize`
//! downcases the remainder, `split` drops trailing empty fields, integer
//! division stays integral, and every float renders with a decimal point.

use crate::lax::{LaxObject, LaxValue};
use crate::time::RTime;
use crate::value::Value as RValue;
use chrono_tz::Tz;
use liquid::model::Value as LValue;
use liquid_core::parser::{FilterArguments, ParameterReflection};
use liquid_core::{
    Error, Expression, Filter, FilterReflection, ParseFilter, Result, Runtime, Value, ValueView,
};
use regex::Regex;
use std::fmt;
use std::sync::{Arc, LazyLock};

/// Site-level facts the filters need at render time.
pub struct FilterCtx {
    pub baseurl: String,
    pub url: String,
    pub timezone: Tz,
    pub smart_quotes: bool,
    pub site_time: RTime,
    pub sass: crate::sass::Options,
    /// A second parser, used by `where_exp` and friends to evaluate their
    /// expression argument. It cannot be the parser these filters are being
    /// registered on, so it is built first with its own context and is `None`
    /// inside that build.
    pub expr_parser: Option<Arc<liquid::Parser>>,
}

type FilterFn = fn(&dyn ValueView, &[Value], &FilterCtx, &dyn Runtime) -> Result<Value>;

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
        (self.func)(input, &args, &self.ctx, runtime)
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
        ("escape", f_xml_escape),
        ("escape_once", f_escape_once),
        ("url_encode", f_cgi_escape),
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
        ("where_exp", f_where_exp),
        ("group_by_exp", f_group_by_exp),
        ("find_exp", f_find_exp),
        ("sample", f_sample),
        ("sassify", f_sassify),
        ("scssify", f_scssify),
    ]
}

/// Evaluate an expression against each item, with `variable` bound to it.
///
/// Jekyll turns the argument into a `Liquid::Condition` (for `where_exp` and
/// `find_exp`) or a `Liquid::Variable` (for `group_by_exp`) and evaluates it
/// per item against the current context. Here the expression is wrapped in a
/// tiny template and rendered by a second parser.
fn eval_expr(
    ctx: &FilterCtx,
    runtime: &dyn Runtime,
    variable: &str,
    expression: &str,
    items: &[Value],
    condition: bool,
) -> Option<Vec<String>> {
    let parser = ctx.expr_parser.as_ref()?;
    let template = if condition {
        // A condition renders "1" when true and nothing when false.
        format!("{{% if {expression} %}}1{{% endif %}}")
    } else {
        format!("{{{{ {expression} }}}}")
    };
    let template = parser.parse(&template).ok()?;

    // Only pull in the globals the expression actually names. Fetching them
    // all would materialise the whole site drop on every call.
    let mut base = LaxObject::new();
    for root in runtime.roots() {
        let name = root.into_string();
        if !mentions(expression, &name) {
            continue;
        }
        if let Some(v) = runtime.try_get(&[liquid_core::model::Scalar::new(name.to_string())]) {
            base.insert(name.to_string(), LaxValue::from_liquid(&v.into_owned()));
        }
    }

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let mut globals = base.clone();
        globals.insert(variable.to_string(), LaxValue::from_liquid(item));
        out.push(template.render(&globals).ok()?);
    }
    Some(out)
}

/// Does the expression reference this identifier as a whole word?
fn mentions(expression: &str, name: &str) -> bool {
    expression.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|word| word == name)
}

/// The `variable, expression` arguments of the `_exp` filters, evaluated
/// against each input item: `(item, rendered expression)` pairs.
fn exp_pairs(
    input: &dyn ValueView,
    args: &[Value],
    c: &FilterCtx,
    r: &dyn Runtime,
    condition: bool,
) -> Option<Vec<(Value, String)>> {
    let (variable, expression) = (arg_str(args, 0)?, arg_str(args, 1)?);
    let items = array_of(input);
    let results = eval_expr(c, r, &variable, &expression, &items, condition)?;
    Some(items.into_iter().zip(results).collect())
}

fn f_where_exp(input: &dyn ValueView, args: &[Value], c: &FilterCtx, r: &dyn Runtime) -> Result<Value> {
    Ok(match exp_pairs(input, args, c, r, true) {
        Some(pairs) => Value::Array(pairs.into_iter().filter(|(_, r)| !r.is_empty()).map(|(i, _)| i).collect()),
        None => input.to_value(),
    })
}

fn f_find_exp(input: &dyn ValueView, args: &[Value], c: &FilterCtx, r: &dyn Runtime) -> Result<Value> {
    Ok(match exp_pairs(input, args, c, r, true) {
        Some(pairs) => pairs.into_iter().find(|(_, r)| !r.is_empty()).map(|(i, _)| i).unwrap_or(Value::Nil),
        None => input.to_value(),
    })
}

/// `group_by_exp`: group by the rendered expression, in first-seen order.
fn f_group_by_exp(input: &dyn ValueView, args: &[Value], c: &FilterCtx, r: &dyn Runtime) -> Result<Value> {
    Ok(match exp_pairs(input, args, c, r, false) {
        Some(pairs) => grouped_array(pairs.into_iter()),
        None => input.to_value(),
    })
}

/// `sample`: Ruby's `Array#sample`, which is unseeded and therefore not
/// reproducible in Jekyll either.
fn f_sample(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let items = array_of(input);
    if items.is_empty() {
        return Ok(Value::Nil);
    }
    let n = args.first().and_then(|v| v.as_scalar()).and_then(|s| s.to_integer());
    match n {
        None => Ok(items[random_below(items.len())].clone()),
        Some(n) if n <= 0 => Ok(Value::Array(Vec::new())),
        Some(n) => {
            // Ruby samples without replacement.
            let mut pool = items;
            let mut picked = Vec::new();
            for _ in 0..(n as usize).min(pool.len()) {
                picked.push(pool.remove(random_below(pool.len())));
            }
            Ok(Value::Array(picked))
        }
    }
}

/// xorshift64*, seeded from the clock. Good enough to shuffle a list.
fn random_below(len: usize) -> usize {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x2545F4914F6CDD1D)
            | 1;
    }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    STATE.store(x, Ordering::Relaxed);
    (x.wrapping_mul(0x2545F4914F6CDD1D) >> 33) as usize % len
}

fn f_sassify(input: &dyn ValueView, _a: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    compile_sass(&s(input), c, true)
}

fn f_scssify(input: &dyn ValueView, _a: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    compile_sass(&s(input), c, false)
}

fn compile_sass(source: &str, c: &FilterCtx, indented: bool) -> Result<Value> {
    crate::sass::compile_with(&c.sass, source, indented)
        .map(Value::scalar)
        .map_err(|e| Error::with_msg(e.to_string()))
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

fn to_rvalue(v: &dyn ValueView) -> RValue {
    from_liquid(&v.to_value())
}

// -- Jekyll filters ---------------------------------------------------------

fn f_slugify(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let mode = arg_str(args, 0).unwrap_or_else(|| "default".into());
    Ok(Value::scalar(crate::url::slugify(&s(input), &mode, false)))
}

fn f_xml_escape(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(Value::scalar(escape_html_ruby(&s(input))))
}

/// `escape_once` leaves existing entity references alone.
fn f_escape_once(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    static ENTITY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"&(?:[a-zA-Z][a-zA-Z0-9]*|#[0-9]+|#[xX][0-9a-fA-F]+);").unwrap());
    let text = s(input);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in ENTITY.find_iter(&text) {
        out.push_str(&escape_html_ruby(&text[last..m.start()]));
        out.push_str(m.as_str());
        last = m.end();
    }
    out.push_str(&escape_html_ruby(&text[last..]));
    Ok(Value::scalar(out))
}

/// Ruby's `CGI.escapeHTML`: note `&#39;` where liquid-rust writes `&#x27;`.
fn escape_html_ruby(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// `cgi_escape` / Liquid's `url_encode`: CGI.escape, where a space is `+`.
fn f_cgi_escape(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(Value::scalar(cgi_escape(&s(input))))
}

fn cgi_escape(input: &str) -> String {
    // Everything but unreserved characters is encoded, then a space is `+`.
    const ESCAPE: &percent_encoding::AsciiSet =
        &percent_encoding::NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');
    percent_encoding::utf8_percent_encode(input, ESCAPE).to_string().replace("%20", "+")
}

fn f_url_decode(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let text = s(input).replace('+', " ");
    Ok(Value::scalar(
        percent_encoding::percent_decode_str(&text)
            .decode_utf8_lossy()
            .to_string(),
    ))
}

/// `uri_escape`: Addressable's normalize, which leaves sub-delimiters alone.
fn f_uri_escape(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    const ESCAPE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'!').remove(b'#').remove(b'$').remove(b'&').remove(b'\'').remove(b'(').remove(b')')
        .remove(b'*').remove(b'+').remove(b',').remove(b'-').remove(b'.').remove(b'/').remove(b':')
        .remove(b';').remove(b'=').remove(b'?').remove(b'@').remove(b'_').remove(b'~');
    Ok(Value::scalar(percent_encoding::utf8_percent_encode(&s(input), ESCAPE).to_string()))
}

fn f_number_of_words(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(Value::scalar(s(input).split_whitespace().count() as i64))
}

fn f_array_to_sentence_string(
    input: &dyn ValueView,
    args: &[Value],
    _c: &FilterCtx,
    _r: &dyn Runtime,
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

fn f_jsonify(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(Value::scalar(render_value(&to_rvalue(input), true)))
}

/// Render a value as JSON (`jsonify`) or as Ruby's `inspect`. The walks are
/// the same shape; only string quoting and the hash separator differ.
fn render_value(v: &RValue, json: bool) -> String {
    let sep = if json { ":" } else { "=>" };
    match v {
        RValue::Null => if json { "null" } else { "nil" }.to_string(),
        RValue::Bool(b) => b.to_string(),
        RValue::Int(i) => i.to_string(),
        RValue::Float(x) => crate::value::ruby_float_to_s(*x),
        RValue::Str(s) => json_string(s),
        RValue::Array(a) => {
            let items: Vec<String> = a.iter().map(|x| render_value(x, json)).collect();
            format!("[{}]", items.join(if json { "," } else { ", " }))
        }
        RValue::Object(o) => {
            let items: Vec<String> = o
                .iter()
                .map(|(k, val)| format!("{}{sep}{}", json_string(k), render_value(val, json)))
                .collect();
            format!("{{{}}}", items.join(if json { "," } else { ", " }))
        }
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
fn f_to_integer(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let v = to_rvalue(input);
    let n = match v {
        RValue::Int(i) => i,
        RValue::Float(x) => x.trunc() as i64,
        RValue::Bool(true) => 1,
        RValue::Bool(false) => 0,
        other => leading_number(&other.to_string()).map(|x| x.trunc() as i64).unwrap_or(0),
    };
    Ok(Value::scalar(n))
}

/// Ruby's `String#to_f`: the leading number, ignoring whatever follows.
fn leading_number(s: &str) -> Option<f64> {
    static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[-+]?\d*\.?\d*").unwrap());
    NUMBER.find(s).and_then(|m| m.as_str().trim().parse::<f64>().ok())
}

/// `inspect`: Ruby's `Object#inspect`, then HTML-escaped by Jekyll.
fn f_inspect(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(Value::scalar(escape_html_ruby(&render_value(&to_rvalue(input), false))))
}

fn f_normalize_whitespace(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(Value::scalar(s(input).split_whitespace().collect::<Vec<_>>().join(" ")))
}

fn f_markdownify(input: &dyn ValueView, _a: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(Value::scalar(crate::markdown::convert_opts(&s(input), c.smart_quotes)))
}

fn f_smartify(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
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

fn f_date(input: &dyn ValueView, args: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    match arg_str(args, 0) {
        Some(f) if !f.is_empty() => date_with(input, c, &f),
        // Liquid returns the input untouched when no format is given.
        _ => Ok(input.to_value()),
    }
}

/// Format the input as a time, or hand it back if it is not one.
fn date_with(input: &dyn ValueView, c: &FilterCtx, fmt: &str) -> Result<Value> {
    match to_time(input, c) {
        Some(t) => t.try_format(fmt).map(Value::scalar).map_err(|e| Error::with_msg(e.to_string())),
        None => Ok(input.to_value()),
    }
}

/// `date_to_string`: "%d %b %Y", or ordinal form when asked.
fn f_date_to_string(input: &dyn ValueView, args: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    date_to_format(input, args, c, "%b")
}

fn f_date_to_long_string(input: &dyn ValueView, args: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
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

fn f_date_to_xmlschema(input: &dyn ValueView, _a: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    date_with(input, c, "%Y-%m-%dT%H:%M:%S%:z")
}

fn f_date_to_rfc822(input: &dyn ValueView, _a: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    date_with(input, c, "%a, %d %b %Y %H:%M:%S %z")
}

// -- URLs -------------------------------------------------------------------

// The URL filters pass nil through untouched.
fn f_relative_url(input: &dyn ValueView, _a: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(unless_nil(input, |s| crate::url::relative_url(s, &c.baseurl)))
}

fn f_absolute_url(input: &dyn ValueView, _a: &[Value], c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(unless_nil(input, |s| crate::url::absolute_url(s, &c.url, &c.baseurl)))
}

fn f_strip_index(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    Ok(unless_nil(input, crate::url::strip_index))
}

fn unless_nil(input: &dyn ValueView, f: impl Fn(&str) -> String) -> Value {
    if input.is_nil() { Value::Nil } else { Value::scalar(f(&s(input))) }
}

// -- arrays -----------------------------------------------------------------

// Ruby's Array#push/pop/shift/unshift on a copy of the input.
fn array_op(input: &dyn ValueView, op: impl FnOnce(&mut Vec<Value>)) -> Result<Value> {
    let mut a = array_of(input);
    op(&mut a);
    Ok(Value::Array(a))
}

fn f_push(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    array_op(input, |a| a.extend(args.first().cloned()))
}

fn f_pop(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    array_op(input, |a| drop(a.pop()))
}

fn f_shift(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    array_op(input, |a| drop(a.drain(..1.min(a.len()))))
}

fn f_unshift(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    array_op(input, |a| a.splice(0..0, args.first().cloned()).for_each(drop))
}

/// `where`: select items whose property equals the given value, comparing as
/// strings the way Jekyll does.
fn f_where(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let key = arg_str(args, 0).unwrap_or_default();
    let want = args.get(1);
    let out: Vec<Value> = array_of(input)
        .into_iter()
        .filter(|item| property_matches(item, &key, want))
        .collect();
    Ok(Value::Array(out))
}

fn f_find(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let key = arg_str(args, 0).unwrap_or_default();
    let want = args.get(1);
    Ok(array_of(input)
        .into_iter()
        .find(|item| property_matches(item, &key, want))
        .unwrap_or(Value::Nil))
}

fn property_matches(item: &Value, key: &str, want: Option<&Value>) -> bool {
    match (property(item, key), want) {
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
fn f_group_by(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let key = arg_str(args, 0).unwrap_or_default();
    let pairs = array_of(input).into_iter().map(|item| {
        let name = property(&item, &key).map(|v| v.to_kstr().into_owned().into_string()).unwrap_or_default();
        (item, name)
    });
    Ok(grouped_array(pairs))
}

/// `item[key]` for an object item, as Jekyll's `item_property` reads it.
fn property(item: &Value, key: &str) -> Option<Value> {
    item.as_object().and_then(|o| o.get(key)).map(|v| v.to_value())
}

/// `sort`: by a property when given one, else by rendered value. Ruby's sort
/// puts nil first.
fn f_sort(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let key = arg_str(args, 0);
    let mut items = array_of(input);
    items.sort_by(|a, b| match &key {
        None => compare_values(a, b),
        Some(k) => {
            match (property(a, k), property(b, k)) {
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
fn f_map(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let key = arg_str(args, 0).unwrap_or_default();
    let out: Vec<Value> = array_of(input)
        .into_iter()
        .map(|item| match item.as_object() {
            Some(_) => property(&item, &key).unwrap_or(Value::Nil),
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
fn f_capitalize(input: &dyn ValueView, _a: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let text = s(input);
    let mut chars = text.chars();
    let out = match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    };
    Ok(Value::scalar(out))
}

/// Ruby's `String#split` drops trailing empty fields.
fn f_split(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
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
fn f_divided_by(input: &dyn ValueView, args: &[Value], _c: &FilterCtx, _r: &dyn Runtime) -> Result<Value> {
    let a = to_rvalue(input);
    let b = args.first().map(|v| from_liquid(v)).unwrap_or(RValue::Int(1));
    let both_int = matches!(a, RValue::Int(_)) && matches!(b, RValue::Int(_));
    let (x, y) = (numeric(&a), numeric(&b));
    if y == 0.0 {
        return Error::with_msg("divided by 0").into_err();
    }
    if both_int {
        Ok(Value::scalar((x as i64).div_euclid(y as i64)))
    } else {
        Ok(Value::scalar(x / y))
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

/// `Jekyll::Filters::GroupingFilters#grouped_array`: one entry per distinct
/// name, in first-seen order, each `{"name", "items", "size"}`.
fn grouped_array(pairs: impl Iterator<Item = (Value, String)>) -> Value {
    let mut groups: indexmap::IndexMap<String, Vec<Value>> = indexmap::IndexMap::new();
    for (item, name) in pairs {
        groups.entry(name).or_default().push(item);
    }
    let out: Vec<Value> = groups
        .into_iter()
        .map(|(name, items)| {
            let mut o = liquid_core::model::Object::new();
            o.insert("name".into(), Value::scalar(name));
            o.insert("size".into(), Value::scalar(items.len() as i64));
            o.insert("items".into(), Value::Array(items));
            Value::Object(o)
        })
        .collect();
    Value::Array(out)
}

/// A Liquid value as a rekyll `Value`, for filters that reuse site code.
fn from_liquid(v: &LValue) -> RValue {
    match v {
        LValue::Nil | LValue::State(_) => RValue::Null,
        LValue::Scalar(s) => {
            // Order matters: a scalar holding "1" answers to_integer, so ask
            // for the narrowest type first and fall back to the source text.
            if let Some(b) = s.to_bool() {
                RValue::Bool(b)
            } else if let Some(i) = s.to_integer() {
                RValue::Int(i)
            } else if let Some(f) = s.to_float() {
                RValue::Float(f)
            } else {
                RValue::Str(s.to_kstr().to_string())
            }
        }
        LValue::Array(a) => RValue::Array(a.iter().map(from_liquid).collect()),
        LValue::Object(o) => {
            let mut out = crate::value::Object::new();
            for (k, v) in o.iter() {
                out.insert(k.to_string(), from_liquid(v));
            }
            RValue::Object(out)
        }
    }
}
