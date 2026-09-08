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
use std::sync::OnceLock;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

struct Res {
    time: Regex,
    date: Regex,
    float: Regex,
    int_legacy: Regex,
    string_guard: Regex,
    base60_int: Regex,
    base60_float: Regex,
}

fn res() -> &'static Res {
    static R: OnceLock<Res> = OnceLock::new();
    R.get_or_init(|| Res {
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
    })
}

/// Psych::ScalarScanner#tokenize — resolve a *plain, untagged* scalar.
pub fn tokenize(s: &str) -> Value {
    if s.is_empty() {
        return Value::Null;
    }
    let r = res();

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
        let cleaned = s.replace([',', '_'], "");
        // Ruby strips a trailing "." before the exponent or end of string.
        let cleaned = Regex::new(r"\.([Ee]|$)").unwrap().replace(&cleaned, "$1").to_string();
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
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')) {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    s[..end].parse::<i64>().unwrap_or(0)
}

/// Psych::ScalarScanner#parse_time.
fn parse_time(s: &str) -> Option<DateTime<FixedOffset>> {
    let r = Regex::new(
        r"^(-?\d{4})-(\d{1,2})-(\d{1,2})[ tT](\d{1,2}):(\d\d):(\d\d)(?:\.(\d*))?\s*(Z|[-+]\d{1,2}:?(?:\d\d)?)?",
    )
    .ok()?;
    let c = r.captures(s)?;
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
        Some(tz) => {
            let tzr = Regex::new(r"^([+\-]?\d{1,2}):?(\d{1,2})?$").ok()?;
            let tc = tzr.captures(tz)?;
            let hh: i32 = tc[1].parse().ok()?;
            let mm: i32 = tc.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            if hh < 0 {
                hh * 3600 - mm * 60
            } else {
                hh * 3600 + mm * 60
            }
        }
    };
    let off = FixedOffset::east_opt(offset_secs)?;
    Some(off.from_utc_datetime(&(naive - chrono::Duration::seconds(offset_secs as i64))))
}

// ---------------------------------------------------------------------------
// Document loading
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Node {
    Scalar(Value),
    Seq(Vec<Node>),
    Map(Vec<(Node, Node)>),
}

impl Node {
    fn into_value(self) -> Value {
        match self {
            Node::Scalar(v) => v,
            Node::Seq(items) => Value::Array(items.into_iter().map(Node::into_value).collect()),
            Node::Map(pairs) => {
                let mut o = Object::new();
                for (k, v) in pairs {
                    // Ruby hash keys here are whatever YAML resolved them to;
                    // Jekyll only ever reads string keys, so stringify.
                    let key = match k.into_value() {
                        Value::Str(s) => s,
                        other => other.to_string(),
                    };
                    o.insert(key, v.into_value());
                }
                Value::Object(o)
            }
        }
    }
}

#[derive(Default)]
struct Loader {
    docs: Vec<Node>,
    stack: Vec<Node>,
    keys: Vec<Option<Node>>,
    anchors: std::collections::HashMap<usize, Node>,
    /// Anchor ids for containers that are open but not yet finished; the node
    /// only exists at its end event, but the id arrives at its start.
    open_anchors: Vec<usize>,
    error: Option<String>,
}

impl Loader {
    fn push(&mut self, node: Node, anchor: usize) {
        if anchor > 0 {
            self.anchors.insert(anchor, node.clone());
        }
        match self.stack.last_mut() {
            None => self.docs.push(node),
            Some(Node::Seq(items)) => items.push(node),
            Some(Node::Map(pairs)) => {
                let slot = self.keys.last_mut().expect("map without key slot");
                match slot.take() {
                    None => *slot = Some(node),
                    Some(key) => pairs.push((key, node)),
                }
            }
            Some(Node::Scalar(_)) => unreachable!("scalar cannot contain children"),
        }
    }

}

impl MarkedEventReceiver for Loader {
    fn on_event(&mut self, ev: Event, _mark: Marker) {
        if self.error.is_some() {
            return;
        }
        match ev {
            Event::Scalar(text, style, anchor, tag) => {
                let v = resolve_scalar(&text, style, tag.as_ref());
                self.push(Node::Scalar(v), anchor);
            }
            Event::SequenceStart(anchor, _) => {
                self.open_anchors.push(anchor);
                self.stack.push(Node::Seq(Vec::new()));
            }
            Event::MappingStart(anchor, _) => {
                self.open_anchors.push(anchor);
                self.stack.push(Node::Map(Vec::new()));
                self.keys.push(None);
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let node = match self.stack.pop() {
                    Some(n) => n,
                    None => return,
                };
                if matches!(node, Node::Map(_)) {
                    self.keys.pop();
                }
                let anchor = self.open_anchors.pop().unwrap_or(0);
                self.push(node, anchor);
            }
            Event::Alias(id) => {
                if let Some(n) = self.anchors.get(&id).cloned() {
                    self.push(n, 0);
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
    let mut parser = Parser::new_from_str(src);
    parser
        .load(&mut loader, true)
        .map_err(|e| anyhow!("YAML parse error: {e}"))?;
    if let Some(e) = loader.error {
        return Err(anyhow!(e));
    }
    Ok(loader.docs.into_iter().next().map(Node::into_value).unwrap_or(Value::Null))
}
