//! Permalink template expansion and URL sanitisation (`jekyll/url.rb`).

use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use regex::Regex;
use std::sync::OnceLock;

/// Characters Addressable leaves unescaped in a path: unreserved, sub-delims,
/// ":", "@" and "/". Everything outside that set is percent-encoded. Jekyll
/// then escapes "#" separately because Addressable treats it as a fragment.
const PATH_SAFE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'\\')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'%');

/// `URL.escape_path`.
pub fn escape_path(path: &str) -> String {
    static SIMPLE: OnceLock<Regex> = OnceLock::new();
    let simple = SIMPLE.get_or_init(|| Regex::new(r"^[a-zA-Z0-9./-]+$").unwrap());
    if path.is_empty() || simple.is_match(path) {
        return path.to_string();
    }
    utf8_percent_encode(path, PATH_SAFE).to_string().replace('#', "%23")
}

/// `URL.unescape_path`.
pub fn unescape_path(path: &str) -> String {
    if !path.contains('%') {
        return path.to_string();
    }
    percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// `URL#sanitize_url`: force a leading slash, neutralise "..", drop "./",
/// and squeeze runs of slashes down to one.
pub fn sanitize_url(s: &str) -> String {
    let mut result = format!("/{s}").replace("..", "/");
    result = result.replace("./", "");
    squeeze_slashes(&result)
}

fn squeeze_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for c in s.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(c);
            }
            prev_slash = true;
        } else {
            out.push(c);
            prev_slash = false;
        }
    }
    out
}

/// `URL#generate_url_from_hash`: substitute `:key` placeholders in order,
/// stopping early once no colon remains. A `nil` value removes the preceding
/// slash as well, so absent categories do not leave a double slash behind.
pub fn generate_url(template: &str, placeholders: &[(&str, Option<String>)]) -> String {
    let mut result = template.to_string();
    for (key, value) in placeholders {
        if !result.contains(':') {
            break;
        }
        match value {
            None => result = result.replace(&format!("/:{key}"), ""),
            Some(v) => result = result.replace(&format!(":{key}"), &escape_path(v)),
        }
    }
    result
}

/// `Utils.add_permalink_suffix`.
pub fn add_permalink_suffix(template: &str, permalink_style: &str) -> String {
    let mut t = template.to_string();
    match permalink_style {
        "pretty" => t.push('/'),
        "date" | "ordinal" | "none" => t.push_str(":output_ext"),
        other => {
            if other.ends_with('/') {
                t.push('/');
            }
            if other.ends_with(":output_ext") {
                t.push_str(":output_ext");
            }
        }
    }
    t
}
