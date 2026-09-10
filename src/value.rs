//! The universal document/config value type, and YAML loading into it.
//!
//! Mirrors the Ruby object graph Jekyll builds from YAML: insertion-ordered
//! hashes and integers distinct from floats. YAML is read with `yaml-rust2`'s
//! own (YAML 1.2) typing rather than Psych's 1.1 rules, so `yes` is a string
//! and `010` is ten; dates stay strings and are parsed where they are used.

use anyhow::{anyhow, Result};
use indexmap::IndexMap;
use std::fmt;
use yaml_rust2::{Yaml, YamlLoader};

pub type Object = IndexMap<String, Value>;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Object(Object),
}

impl Value {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
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
            Value::Array(a) => {
                // Ruby's Array#to_s is `inspect`, but Liquid joins with "".
                for v in a {
                    write!(f, "{v}")?;
                }
                Ok(())
            }
            // Ruby's Hash#to_s, only reached by pathological templates.
            Value::Object(o) => {
                let inner: Vec<String> = o.iter().map(|(k, v)| format!("{k:?} => {v}")).collect();
                write!(f, "{{{}}}", inner.join(", "))
            }
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

/// Load the first YAML document in `src`; an empty stream is `Null`.
pub fn load_yaml(src: &str) -> Result<Value> {
    let docs = YamlLoader::load_from_str(src).map_err(|e| anyhow!("YAML parse error: {e}"))?;
    Ok(docs.into_iter().next().map(from_yaml).unwrap_or(Value::Null))
}

fn from_yaml(y: Yaml) -> Value {
    match y {
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => Value::Null,
        Yaml::Boolean(b) => Value::Bool(b),
        Yaml::Integer(i) => Value::Int(i),
        Yaml::Real(r) => r.parse().map(Value::Float).unwrap_or(Value::Str(r)),
        Yaml::String(s) => Value::Str(s),
        Yaml::Array(a) => Value::Array(a.into_iter().map(from_yaml).collect()),
        Yaml::Hash(h) => {
            let mut out = Object::new();
            for (k, v) in h {
                // Jekyll only ever reads string keys; others are stringified.
                let key = from_yaml(k).to_string();
                if key == "<<" {
                    // A merge key splices in another mapping, or a list of
                    // them, at this position, as Ruby's YAML does.
                    let parts = match v { Yaml::Array(a) => a, other => vec![other] };
                    for m in parts {
                        if let Value::Object(o) = from_yaml(m) {
                            out.extend(o);
                        }
                    }
                    continue;
                }
                out.insert(key, from_yaml(v));
            }
            Value::Object(out)
        }
    }
}
