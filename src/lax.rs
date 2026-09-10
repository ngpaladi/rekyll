//! A Liquid value tree with Jekyll's lax lookup semantics.
//!
//! Jekyll runs Liquid with `strict_variables: false`, so `{{ nope }}` and
//! `{{ page.nope.deeper }}` render as empty rather than raising. liquid-rust
//! reports "Unknown variable"/"Unknown index" instead, and that difference
//! shows up in almost every real template. Wrapping the payload in these types
//! makes every lookup succeed, yielding nil where Jekyll would.

use crate::value::{Object as RObject, Value as RValue};
use liquid::model::{
    DisplayCow, KStringCow, Object as LObject, ObjectView, Scalar, ScalarCow, State, Value as LValue,
};
use liquid::ValueView;
use std::fmt;

#[derive(Debug, Clone)]
pub enum LaxValue {
    Nil,
    Scalar(Scalar),
    Array(Vec<LaxValue>),
    Object(LaxObject),
    /// A shared subtree. The `site` drop is large and identical across every
    /// page rendered in a pass, so it is built once and referenced rather than
    /// deep-cloned into each page's payload.
    Shared(std::sync::Arc<LaxValue>),
}

#[derive(Debug, Clone, Default)]
pub struct LaxObject(pub indexmap::IndexMap<String, LaxValue>);

static NIL: LaxValue = LaxValue::Nil;

impl LaxObject {
    pub fn new() -> Self {
        LaxObject(indexmap::IndexMap::new())
    }

    pub fn insert(&mut self, k: impl Into<String>, v: LaxValue) {
        self.0.insert(k.into(), v);
    }

    pub fn from_value_object(o: &RObject) -> Self {
        let mut out = LaxObject::new();
        for (k, v) in o {
            out.0.insert(k.clone(), LaxValue::from_value(v));
        }
        out
    }
}

impl LaxValue {
    pub fn from_value(v: &RValue) -> LaxValue {
        match v {
            RValue::Null => LaxValue::Nil,
            RValue::Bool(b) => LaxValue::Scalar(Scalar::new(*b)),
            RValue::Int(i) => LaxValue::Scalar(Scalar::new(*i)),
            RValue::Float(x) => LaxValue::Scalar(Scalar::new(*x)),
            RValue::Str(s) => LaxValue::Scalar(Scalar::new(s.clone())),
            RValue::Array(a) => LaxValue::Array(a.iter().map(LaxValue::from_value).collect()),
            RValue::Object(o) => LaxValue::Object(LaxObject::from_value_object(o)),
        }
    }

    pub fn str(s: impl Into<String>) -> LaxValue {
        LaxValue::Scalar(Scalar::new(s.into()))
    }

    pub fn from_liquid(v: &LValue) -> LaxValue {
        match v {
            LValue::Nil | LValue::State(_) => LaxValue::Nil,
            LValue::Scalar(s) => LaxValue::Scalar(s.clone().into_owned()),
            LValue::Array(a) => LaxValue::Array(a.iter().map(LaxValue::from_liquid).collect()),
            LValue::Object(o) => {
                let mut out = LaxObject::new();
                for (k, v) in o.iter() {
                    out.0.insert(k.to_string(), LaxValue::from_liquid(v));
                }
                LaxValue::Object(out)
            }
        }
    }
}

impl ValueView for LaxValue {
    fn as_debug(&self) -> &dyn fmt::Debug {
        self
    }

    fn render(&self) -> DisplayCow<'_> {
        DisplayCow::Owned(Box::new(self.to_kstr().into_owned().into_string()))
    }

    fn source(&self) -> DisplayCow<'_> {
        DisplayCow::Owned(Box::new(self.to_value().source().to_string()))
    }

    fn type_name(&self) -> &'static str {
        match self {
            LaxValue::Nil => "nil",
            LaxValue::Scalar(s) => s.type_name(),
            LaxValue::Array(_) => "array",
            LaxValue::Object(_) => "object",
            LaxValue::Shared(v) => v.type_name(),
        }
    }

    fn query_state(&self, state: State) -> bool {
        match self {
            LaxValue::Nil => match state {
                State::Truthy => false,
                State::DefaultValue | State::Empty | State::Blank => true,
            },
            LaxValue::Scalar(s) => s.query_state(state),
            LaxValue::Array(a) => match state {
                State::Truthy => true,
                State::DefaultValue | State::Empty | State::Blank => a.is_empty(),
            },
            LaxValue::Object(o) => match state {
                State::Truthy => true,
                State::DefaultValue | State::Empty | State::Blank => o.0.is_empty(),
            },
            LaxValue::Shared(v) => v.query_state(state),
        }
    }

    fn to_kstr(&self) -> KStringCow<'_> {
        match self {
            LaxValue::Nil => KStringCow::from_static(""),
            LaxValue::Scalar(s) => s.to_kstr(),
            // Liquid joins array elements with no separator when rendering.
            LaxValue::Array(a) => {
                let joined: String = a.iter().map(|v| v.to_kstr().into_owned().into_string()).collect();
                KStringCow::from_string(joined)
            }
            LaxValue::Object(_) => KStringCow::from_string(self.to_value().to_kstr().to_string()),
            LaxValue::Shared(v) => KStringCow::from_string(v.to_kstr().to_string()),
        }
    }

    fn to_value(&self) -> LValue {
        match self {
            LaxValue::Nil => LValue::Nil,
            LaxValue::Scalar(s) => LValue::Scalar(s.clone()),
            LaxValue::Array(a) => LValue::Array(a.iter().map(|v| v.to_value()).collect()),
            LaxValue::Object(o) => {
                let mut out = LObject::new();
                for (k, v) in &o.0 {
                    out.insert(k.clone().into(), v.to_value());
                }
                LValue::Object(out)
            }
            LaxValue::Shared(v) => v.to_value(),
        }
    }

    fn as_scalar(&self) -> Option<ScalarCow<'_>> {
        match self {
            LaxValue::Scalar(s) => Some(s.clone()),
            LaxValue::Shared(v) => v.as_scalar(),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&dyn liquid::model::ArrayView> {
        match self {
            LaxValue::Array(a) => Some(a),
            LaxValue::Shared(v) => v.as_array(),
            _ => None,
        }
    }

    fn as_object(&self) -> Option<&dyn ObjectView> {
        match self {
            LaxValue::Object(o) => Some(o),
            LaxValue::Shared(v) => v.as_object(),
            _ => None,
        }
    }

    fn is_nil(&self) -> bool {
        match self {
            LaxValue::Nil => true,
            LaxValue::Shared(v) => v.is_nil(),
            _ => false,
        }
    }
}

impl ValueView for LaxObject {
    fn as_debug(&self) -> &dyn fmt::Debug {
        self
    }
    fn render(&self) -> DisplayCow<'_> {
        DisplayCow::Owned(Box::new(self.to_value().render().to_string()))
    }
    fn source(&self) -> DisplayCow<'_> {
        DisplayCow::Owned(Box::new(self.to_value().source().to_string()))
    }
    fn type_name(&self) -> &'static str {
        "object"
    }
    fn query_state(&self, state: State) -> bool {
        match state {
            State::Truthy => true,
            State::DefaultValue | State::Empty | State::Blank => self.0.is_empty(),
        }
    }
    fn to_kstr(&self) -> KStringCow<'_> {
        KStringCow::from_string(self.to_value().to_kstr().to_string())
    }
    fn to_value(&self) -> LValue {
        let mut out = LObject::new();
        for (k, v) in &self.0 {
            out.insert(k.clone().into(), v.to_value());
        }
        LValue::Object(out)
    }
    fn as_object(&self) -> Option<&dyn ObjectView> {
        Some(self)
    }
}

impl ObjectView for LaxObject {
    fn as_value(&self) -> &dyn ValueView {
        self
    }

    fn size(&self) -> i64 {
        self.0.len() as i64
    }

    fn keys<'k>(&'k self) -> Box<dyn Iterator<Item = KStringCow<'k>> + 'k> {
        Box::new(self.0.keys().map(|k| KStringCow::from_ref(k.as_str())))
    }

    fn values<'k>(&'k self) -> Box<dyn Iterator<Item = &'k dyn ValueView> + 'k> {
        Box::new(self.0.values().map(|v| v as &dyn ValueView))
    }

    fn iter<'k>(&'k self) -> Box<dyn Iterator<Item = (KStringCow<'k>, &'k dyn ValueView)> + 'k> {
        Box::new(
            self.0
                .iter()
                .map(|(k, v)| (KStringCow::from_ref(k.as_str()), v as &dyn ValueView)),
        )
    }

    /// Report every key as present. Liquid consults this before `get`, and
    /// answering `false` would resurrect the strict-lookup error.
    fn contains_key(&self, _index: &str) -> bool {
        true
    }

    /// Missing keys resolve to nil, which is what Jekyll's lax mode renders.
    fn get<'s>(&'s self, index: &str) -> Option<&'s dyn ValueView> {
        match self.0.get(index) {
            Some(v) => Some(v as &dyn ValueView),
            None => Some(&NIL as &dyn ValueView),
        }
    }
}
