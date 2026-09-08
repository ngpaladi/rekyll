//! Conversion between rekyll's `Value` and the `liquid` crate's model.

use crate::value::{Object, Value};
use liquid::model::{Object as LObject, Value as LValue};
use liquid::ValueView;

pub fn to_liquid(v: &Value) -> LValue {
    match v {
        Value::Null => LValue::Nil,
        Value::Bool(b) => LValue::scalar(*b),
        Value::Int(i) => LValue::scalar(*i),
        Value::Float(x) => LValue::scalar(*x),
        Value::Str(s) => LValue::scalar(s.to_owned()),
        // Liquid's own date type cannot represent Ruby's Time faithfully
        // (offsets, `date_only`), and Jekyll's date filters do their own
        // formatting, so times cross the boundary as their Ruby `to_s` form
        // and are re-parsed by the filters that need them.
        Value::Date { .. } => LValue::scalar(v.to_string()),
        Value::Array(a) => LValue::Array(a.iter().map(to_liquid).collect()),
        Value::Object(o) => LValue::Object(object_to_liquid(o)),
    }
}

pub fn object_to_liquid(o: &Object) -> LObject {
    let mut out = LObject::new();
    for (k, v) in o {
        out.insert(k.clone().into(), to_liquid(v));
    }
    out
}

pub fn from_liquid(v: &LValue) -> Value {
    match v {
        LValue::Nil | LValue::State(_) => Value::Null,
        LValue::Scalar(s) => {
            // Order matters: a scalar holding "1" answers to_integer, so ask
            // for the narrowest type first and fall back to the source text.
            if let Some(b) = s.to_bool() {
                Value::Bool(b)
            } else if let Some(i) = s.to_integer() {
                Value::Int(i)
            } else if let Some(f) = s.to_float() {
                Value::Float(f)
            } else {
                Value::Str(s.to_kstr().to_string())
            }
        }
        LValue::Array(a) => Value::Array(a.iter().map(from_liquid).collect()),
        LValue::Object(o) => {
            let mut out = Object::new();
            for (k, v) in o.iter() {
                out.insert(k.to_string(), from_liquid(v));
            }
            Value::Object(out)
        }
    }
}
