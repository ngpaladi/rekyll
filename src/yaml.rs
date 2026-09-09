//! YAML loading with Psych (YAML 1.1) scalar semantics.
//!
//! Rust YAML crates resolve scalars per YAML 1.2, where `yes` is a string and
//! `010` is ten. Ruby's Psych resolves per 1.1: `yes`/`on` are booleans and
//! leading-zero integers are octal. Jekyll's output depends on the difference,
//! so we parse to raw events and run Psych's own resolution rules over the
//! scalar text ourselves.
//!
//! Rules ported from `psych/scalar_scanner.rb` (Ruby 3.2 / Psych 4).

use crate::value::{Object, Value};
use anyhow::{anyhow, Result};
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};
use regex::Regex;
use std::sync::LazyLock;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// Psych's scalar patterns, in the order `tokenize` tries them.
struct Res {
    time: Regex,
    date: Regex,
    float: Regex,
    int_legacy: Regex,
    string_guard: Regex,
    base60_int: Regex,
    base60_float: Regex,
    trailing_dot: Regex,
    time_parts: Regex,
}

static RES: LazyLock<Res> = LazyLock::new(|| {
    Res {
        time: Regex::new(
            r"^-?\d{4}-\d{1,2}-\d{1,2}(?:[Tt]|\s+)\d{1,2}:\d\d:\d\d(?:\.\d*)?(?:\s*(?:Z|[-+]\d{1,2}:?(?:\d\d)?))?$",
        )
        .unwrap(),
        date: Regex::new(r"^\d{4}-(?:1[012]|0\d|\d)-(?:[12]\d|3[01]|0\d|\d)$").unwrap(),
        // Psych's FLOAT, base-10 branch only; inf/nan handled separately.
        float: Regex::new(r"^(?:[-+]?([0-9][0-9_,]*)?\.[0-9]*([eE][-+][0-9]+)?)$").unwrap(),
        int_legacy: Regex::new(
            r"^(?:[-+]?0b[0-1_,]+|[-+]?0[0-7_,]+|[-+]?(?:0|[1-9](?:[0-9]|,[0-9]|_[0-9])*)|[-+]?0x[0-9a-fA-F_,]+)$",
        )
        .unwrap(),
        // The "looks like prose, not a number" guard that runs first in Psych.
        string_guard: Regex::new(r#"^[^\d.:\-]?[[:alpha:]_\s!@#$%\^&*(){}<>|/\\~;=]+"#).unwrap(),
        base60_int: Regex::new(r"^[-+]?[0-9][0-9_]*(:[0-5]?[0-9]){1,2}$").unwrap(),
        base60_float: Regex::new(r"^[-+]?[0-9][0-9_]*(:[0-5]?[0-9]){1,2}\.[0-9_]*$").unwrap(),
        // Ruby strips a trailing "." before the exponent or end of string.
        trailing_dot: Regex::new(r"\.([Ee]|$)").unwrap(),
        time_parts: Regex::new(
            r"^(-?\d{4})-(\d{1,2})-(\d{1,2})[ tT](\d{1,2}):(\d\d):(\d\d)(?:\.(\d*))?\s*(Z|[-+]\d{1,2}:?(?:\d\d)?)?",
        )
        .unwrap(),
    }
});

/// Psych::ScalarScanner#tokenize — resolve a *plain, untagged* scalar.
pub fn tokenize(s: &str) -> Value {
    if s.is_empty() {
        return Value::Null;
    }
    let r = &*RES;

    // Guard against hash keys / prose being read as numbers. Psych checks this
    // first and short-circuits anything longer than five characters.
    if r.string_guard.is_match(s) || s.contains('\n') {
        if s.chars().count() > 5 {
            return Value::Str(s.to_string());
        }
        let lower = s.to_ascii_lowercase();
        let first = lower.chars().next().unwrap_or(' ');
        if !"ytonf~".contains(first) {
            return Value::Str(s.to_string());
        }
        return match lower.as_str() {
            "~" | "null" => Value::Null,
            "yes" | "true" | "on" => Value::Bool(true),
            "no" | "false" | "off" => Value::Bool(false),
            _ => Value::Str(s.to_string()),
        };
    }

    if r.time.is_match(s) {
        if let Some(t) = parse_time(s) {
            return Value::Date { at: t, date_only: false };
        }
        return Value::Str(s.to_string());
    }
    if r.date.is_match(s) {
        if let Some(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok() {
            let at = FixedOffset::east_opt(0)
                .unwrap()
                .from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap());
            return Value::Date { at, date_only: true };
        }
        return Value::Str(s.to_string());
    }

    let lower = s.to_ascii_lowercase();
    if lower == ".inf" || lower == "+.inf" {
        return Value::Float(f64::INFINITY);
    }
    if lower == "-.inf" {
        return Value::Float(f64::NEG_INFINITY);
    }
    if lower == ".nan" {
        return Value::Float(f64::NAN);
    }

    // Sexagesimal (base-60) forms, e.g. "1:30" => 90.
    if r.base60_int.is_match(s) {
        return Value::Int(base60(s, false) as i64);
    }
    if r.base60_float.is_match(s) {
        return Value::Float(base60(s, true));
    }

    if r.float.is_match(s) {
        if s == "." || s == "-." || s == "+." {
            return Value::Str(s.to_string());
        }
        let cleaned = r.trailing_dot.replace(&s.replace([',', '_'], ""), "$1").to_string();
        return cleaned.parse::<f64>().map(Value::Float).unwrap_or(Value::Str(s.to_string()));
    }

    if r.int_legacy.is_match(s) {
        if let Some(i) = parse_int(&s.replace([',', '_'], "")) {
            return Value::Int(i);
        }
    }

    Value::Str(s.to_string())
}

/// Ruby's `Integer()`: honours 0b/0x/0-octal prefixes and a leading sign.
fn parse_int(s: &str) -> Option<i64> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let v = if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok()?
    } else if let Some(b) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
        i64::from_str_radix(b, 2).ok()?
    } else if body.len() > 1 && body.starts_with('0') {
        i64::from_str_radix(&body[1..], 8).ok()?
    } else {
        body.parse::<i64>().ok()?
    };
    Some(if neg { -v } else { v })
}

fn base60(s: &str, float: bool) -> f64 {
    let parts: Vec<&str> = s.split(':').collect();
    let mut total = 0f64;
    for (e, n) in parts.iter().enumerate() {
        let pow = 60f64.powi((e as i32 - 2).abs());
        let n = n.replace('_', "");
        let v = if float {
            n.parse::<f64>().unwrap_or(0.0)
        } else {
            // Ruby's String#to_i stops at the first non-digit.
            leading_i64(&n) as f64
        };
        total += v * pow;
    }
    total
}

fn leading_i64(s: &str) -> i64 {
    let end = s.char_indices().find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && "+-".contains(c)))).map_or(s.len(), |(i, _)| i);
    s[..end].parse().unwrap_or(0)
}

/// Psych::ScalarScanner#parse_time.
fn parse_time(s: &str) -> Option<DateTime<FixedOffset>> {
    let c = RES.time_parts.captures(s)?;
    let (y, mo, d) = (c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?);
    let (h, mi, sec) = (c[4].parse().ok()?, c[5].parse().ok()?, c[6].parse().ok()?);
    let nanos = c
        .get(7)
        .map(|m| {
            let frac = format!("0.{}", m.as_str());
            (frac.parse::<f64>().unwrap_or(0.0) * 1e9) as u32
        })
        .unwrap_or(0);

    let date = NaiveDate::from_ymd_opt(y, mo, d)?;
    let naive = date.and_hms_nano_opt(h, mi, sec, nanos)?;

    // No zone marker means UTC in Psych (it builds a UTC Time, then `Time.at`).
    let offset_secs = match c.get(8).map(|m| m.as_str()) {
        None | Some("Z") => 0,
        Some(tz) => crate::time::parse_offset(tz).ok()?,
    };
    let off = FixedOffset::east_opt(offset_secs)?;
    Some(off.from_utc_datetime(&(naive - chrono::Duration::seconds(offset_secs as i64))))
}

// ---------------------------------------------------------------------------
// Document loading
// ---------------------------------------------------------------------------

/// Builds a `Value` directly from the parser's event stream. `Value::Object`
/// is insertion-ordered, so no intermediate tree is needed.
#[derive(Default)]
struct Loader {
    docs: Vec<Value>,
    /// Containers currently open, innermost last.
    stack: Vec<Value>,
    /// Pending mapping key for each open mapping.
    keys: Vec<Option<Value>>,
    anchors: std::collections::HashMap<usize, Value>,
    /// Anchor ids for containers that are open but not yet finished; the id
    /// arrives at the start event, the finished value only at the end.
    open_anchors: Vec<usize>,
    /// Psych refuses to build a Symbol under `safe_load`, so `:name` fails a
    /// Jekyll build. Remembered here because the event callback cannot fail.
    symbol: Option<String>,
}

impl Loader {
    fn push(&mut self, value: Value, anchor: usize) {
        if anchor > 0 {
            self.anchors.insert(anchor, value.clone());
        }
        match self.stack.last_mut() {
            None => self.docs.push(value),
            Some(Value::Array(items)) => items.push(value),
            Some(Value::Object(pairs)) => {
                let slot = self.keys.last_mut().expect("map without key slot");
                match slot.take() {
                    None => *slot = Some(value),
                    Some(key) => {
                        // Jekyll only ever reads string keys.
                        let key = match key {
                            Value::Str(s) => s,
                            other => other.to_string(),
                        };
                        pairs.insert(key, value);
                    }
                }
            }
            Some(_) => unreachable!("scalar cannot contain children"),
        }
    }

    fn open(&mut self, container: Value, anchor: usize) {
        self.open_anchors.push(anchor);
        if matches!(container, Value::Object(_)) {
            self.keys.push(None);
        }
        self.stack.push(container);
    }

    fn close(&mut self) {
        let Some(value) = self.stack.pop() else { return };
        if matches!(value, Value::Object(_)) {
            self.keys.pop();
        }
        let anchor = self.open_anchors.pop().unwrap_or(0);
        self.push(value, anchor);
    }
}

impl MarkedEventReceiver for Loader {
    fn on_event(&mut self, ev: Event, _mark: Marker) {
        match ev {
            Event::Scalar(text, style, anchor, tag) => {
                if style == TScalarStyle::Plain && tag.is_none() && text.len() > 1 && text.starts_with(':') {
                    self.symbol.get_or_insert(text.to_string());
                }
                let v = resolve_scalar(&text, style, tag.as_ref());
                self.push(v, anchor);
            }
            Event::SequenceStart(anchor, _) => self.open(Value::Array(Vec::new()), anchor),
            Event::MappingStart(anchor, _) => self.open(Value::Object(Object::new()), anchor),
            Event::SequenceEnd | Event::MappingEnd => self.close(),
            Event::Alias(id) => {
                if let Some(v) = self.anchors.get(&id).cloned() {
                    self.push(v, 0);
                }
            }
            _ => {}
        }
    }
}

fn resolve_scalar(text: &str, style: TScalarStyle, tag: Option<&yaml_rust2::parser::Tag>) -> Value {
    // Psych: quoted scalars are always strings, never type-resolved.
    if style != TScalarStyle::Plain {
        return Value::Str(text.to_string());
    }
    if let Some(tag) = tag {
        if tag.handle == "tag:yaml.org,2002:" {
            return match tag.suffix.as_str() {
                "str" => Value::Str(text.to_string()),
                "bool" => Value::Bool(matches!(
                    text.to_ascii_lowercase().as_str(),
                    "yes" | "true" | "on"
                )),
                "int" => tokenize(text),
                "float" => match tokenize(text) {
                    Value::Int(i) => Value::Float(i as f64),
                    other => other,
                },
                "null" => Value::Null,
                _ => tokenize(text),
            };
        }
        return Value::Str(text.to_string());
    }
    tokenize(text)
}

/// Load a single YAML document, returning `Value::Null` for an empty stream.
pub fn load(src: &str) -> Result<Value> {
    let mut loader = Loader::default();
    Parser::new_from_str(src)
        .load(&mut loader, true)
        .map_err(|e| anyhow!("YAML parse error: {e}"))?;
    if let Some(sym) = loader.symbol {
        return Err(anyhow!("Tried to load unspecified class: Symbol ({sym})"));
    }
    Ok(loader.docs.into_iter().next().unwrap_or(Value::Null))
}
