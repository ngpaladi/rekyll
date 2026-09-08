//! Front matter extraction, matching `Document::YAML_FRONT_MATTER_REGEXP`.

use crate::value::{Object, Value};
use regex::Regex;
use std::sync::OnceLock;

pub struct Parsed {
    pub data: Object,
    pub content: String,
    /// False when the file had no front matter at all, which makes it a static
    /// file rather than a page.
    pub has_front_matter: bool,
}

fn fm_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // (?s) = Ruby's /m (dot matches newline); (?m) enables ^/$ per line, which
    // Ruby applies by default.
    // Jekyll's regex uses \s, which also matches \r, so CRLF files parse.
    R.get_or_init(|| Regex::new(r"(?sm)\A(---\s*?\n.*?\n?)^((---|\.\.\.)\s*?$\n?)").unwrap())
}

/// True if the file starts with a front-matter marker, the same cheap check
/// `Utils.has_yaml_header?` performs before deciding a file is a page.
pub fn has_yaml_header(text: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A---\s*\r?\n").unwrap()).is_match(text)
}

pub fn parse(text: &str) -> Parsed {
    if let Some(caps) = fm_regex().captures(text) {
        let yaml_src = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let end = caps.get(0).unwrap().end();
        let data = crate::yaml::load(yaml_src)
            .ok()
            .and_then(|v| v.as_object().cloned())
            // A front-matter block holding only comments parses to nil, which
            // Jekyll turns into an empty hash rather than an error.
            .unwrap_or_default();
        return Parsed { data, content: text[end..].to_string(), has_front_matter: true };
    }
    Parsed { data: Object::new(), content: text.to_string(), has_front_matter: false }
}

/// Helper for reading a scalar out of a data hash.
pub fn data_str<'a>(data: &'a Object, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Value::as_str)
}
