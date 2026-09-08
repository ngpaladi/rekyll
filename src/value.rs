//! The universal document/config value type.
//!
//! Mirrors the Ruby object graph Jekyll builds from YAML: insertion-ordered
//! hashes, integers distinct from floats, and `Time` as a first-class scalar.

use chrono::{DateTime, FixedOffset};
use indexmap::IndexMap;
use std::fmt;

pub type Object = IndexMap<String, Value>;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// A YAML timestamp. `date_only` records whether the source scalar carried
    /// only a date, which changes how Ruby renders it via `to_s`.
    Date { at: DateTime<FixedOffset>, date_only: bool },
    Array(Vec<Value>),
    Object(Object),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|o| o.get(key))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Ruby truthiness: only `nil` and `false` are falsy. Notably `""` and `0`
    /// are truthy, which Liquid inherits.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Null | Value::Bool(false))
    }

    pub fn object(pairs: Vec<(&str, Value)>) -> Value {
        let mut o = Object::new();
        for (k, v) in pairs {
            o.insert(k.to_string(), v);
        }
        Value::Object(o)
    }

    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }
}

/// Renders a value the way Ruby's `to_s` would, which is what Liquid
/// interpolation ultimately emits.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => Ok(()),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{}", ruby_float_to_s(*x)),
            Value::Str(s) => write!(f, "{s}"),
            Value::Date { at, date_only } => write!(f, "{}", ruby_time_to_s(at, *date_only)),
            Value::Array(a) => {
                // Ruby's Array#to_s is `inspect`, but Liquid joins with "".
                for v in a {
                    write!(f, "{v}")?;
                }
                Ok(())
            }
            Value::Object(_) => write!(f, "{}", ruby_hash_to_s(self)),
        }
    }
}

/// Ruby's `Float#to_s`: always keeps a decimal point (`3.0`, not `3`) and uses
/// the shortest representation that round-trips.
pub fn ruby_float_to_s(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    // Rust's `{}` for f64 is already shortest-round-trip, but drops the
    // fractional part for integral values where Ruby keeps ".0".
    let s = format!("{x}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

/// Ruby `Time#to_s` => "2020-01-02 03:04:05 +0000"; `Date#to_s` => "2020-01-02".
pub fn ruby_time_to_s(at: &DateTime<FixedOffset>, date_only: bool) -> String {
    if date_only {
        at.format("%Y-%m-%d").to_string()
    } else {
        at.format("%Y-%m-%d %H:%M:%S %z").to_string()
    }
}

fn ruby_hash_to_s(v: &Value) -> String {
    // Only reached for pathological templates; Ruby renders `{"k" => v}`.
    match v {
        Value::Object(o) => {
            let inner: Vec<String> =
                o.iter().map(|(k, val)| format!("{:?} => {}", k, val)).collect();
            format!("{{{}}}", inner.join(", "))
        }
        _ => String::new(),
    }
}
