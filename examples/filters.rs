//! Renders each line of a Liquid template through rekyll, in the same shape as
//! tests/harness/filters_ref.rb.

fn main() {
    let template = std::env::args().nth(1).expect("usage: filters <template>");
    let source = std::env::args().nth(2).unwrap_or_else(|| "tests/fixtures/01-static".into());

    let mut site = rekyll::site::Site::new(
        std::path::Path::new(&source),
        std::path::Path::new("/tmp/rekyll-diff/filters-site"),
    )
    .expect("site");
    // Match the reference script's configuration.
    site.config.0.insert("url".into(), rekyll::value::Value::str("https://example.com"));
    site.config.0.insert("baseurl".into(), rekyll::value::Value::str("/base"));
    site.timezone = chrono_tz::UTC;
    site.time = rekyll::time::parse_date("2030-01-01 00:00:00 +0000", chrono_tz::UTC).unwrap();
    site.read().expect("read");

    let renderer = rekyll::render::Renderer::new(&site).expect("renderer");
    let state = rekyll::render::RenderState::new();

    let text = std::fs::read_to_string(&template).expect("read template");
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let out = match renderer.render_string(&site, line, &state) {
            Ok(o) => o,
            Err(e) => format!("ERROR({e})"),
        };
        // Ruby's `puts` does not add a second newline when the string already
        // ends with one, so the reference script's lines must be matched.
        if out.ends_with('\n') {
            print!("{line}\t=>\t{out}");
        } else {
            println!("{line}\t=>\t{out}");
        }
    }
}
