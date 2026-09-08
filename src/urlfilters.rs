//! `Jekyll::Filters::URLFilters`, shared by the filters and the link tags.

/// `relative_url`: prepend the site's baseurl unless the input is already
/// absolute (has a scheme and authority).
pub fn relative_url(input: &str, baseurl: &str) -> String {
    if is_absolute(input) {
        return input.to_string();
    }
    let base = baseurl.trim_end_matches('/');
    normalize(&format!("{}{}", ensure_leading_slash(base), ensure_leading_slash(input)))
}

/// `absolute_url`: site.url followed by the relative URL.
pub fn absolute_url(input: &str, site_url: &str, baseurl: &str) -> String {
    if is_absolute(input) {
        return input.to_string();
    }
    if site_url.is_empty() {
        return relative_url(input, baseurl);
    }
    normalize(&format!("{}{}", site_url, relative_url(input, baseurl)))
}

/// `strip_index`.
pub fn strip_index(input: &str) -> String {
    if let Some(stripped) = input.strip_suffix("/index.html") {
        return format!("{stripped}/");
    }
    if let Some(stripped) = input.strip_suffix("/index.htm") {
        return format!("{stripped}/");
    }
    input.to_string()
}

fn ensure_leading_slash(input: &str) -> String {
    if input.is_empty() || input.starts_with('/') {
        input.to_string()
    } else {
        format!("/{input}")
    }
}

/// A URI is absolute for Addressable's purposes when it has a scheme.
fn is_absolute(input: &str) -> bool {
    match input.find(':') {
        None => false,
        Some(i) => {
            let scheme = &input[..i];
            !scheme.is_empty()
                && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        }
    }
}

/// `Addressable::URI#normalize` as it affects these paths: resolve "." and
/// ".." segments. Percent-encoding is left alone since Jekyll's URLs are
/// already escaped by `URL.escape_path`.
fn normalize(url: &str) -> String {
    let (prefix, path) = match url.find("://") {
        Some(i) => match url[i + 3..].find('/') {
            Some(j) => url.split_at(i + 3 + j),
            None => return url.to_string(),
        },
        None => ("", url),
    };

    let trailing_slash = path.ends_with('/') && path.len() > 1;
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let mut joined = format!("/{}", out.join("/"));
    if trailing_slash && !joined.ends_with('/') {
        joined.push('/');
    }
    format!("{prefix}{joined}")
}
