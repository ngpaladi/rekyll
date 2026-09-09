//! A static file server for previewing a build, with optional watch and
//! live reload.
//!
//! `tiny_http` does the HTTP, `mime_guess` the content types; what is left is
//! mapping URLs onto the build directory, a poll-based watcher, and a
//! generation counter the page checks to know when to reload. It is a preview
//! server, not a web server.

use anyhow::{anyhow, Result};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Jekyll's defaults.
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 4000;

/// The endpoint the injected script polls, and the script itself.
const LIVE_PATH: &str = "/__rekyll_live";

pub struct Options<'a> {
    pub root: &'a Path,
    pub host: &'a str,
    pub port: u16,
    pub baseurl: &'a str,
    /// Rebuild when a source file changes.
    pub watch: Option<Watch<'a>>,
    /// Inject the reload script into served HTML.
    pub livereload: bool,
}

pub struct Watch<'a> {
    pub source: &'a Path,
    pub destination: &'a Path,
    pub baseurl: Option<String>,
}

/// Bumped on every successful rebuild. The page polls it and reloads when the
/// number it sees changes.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn serve(opts: Options) -> Result<()> {
    let server = tiny_http::Server::http((opts.host, opts.port))
        .map_err(|e| anyhow!("binding {}:{}: {e}", opts.host, opts.port))?;

    let base = opts.baseurl.trim_end_matches('/').to_string();
    let root = opts.root.to_path_buf();
    let livereload = opts.livereload;

    if let Some(watch) = opts.watch {
        spawn_watcher(watch.source.to_path_buf(), watch.destination.to_path_buf(), watch.baseurl);
    }

    println!("    Server address: http://{}:{}{base}/", opts.host, opts.port);
    if livereload {
        println!("       Live reload: enabled");
    }
    println!("  Server running... press ctrl-c to stop.");

    let base = Arc::new(base);
    for request in server.incoming_requests() {
        let (root, base) = (root.clone(), base.clone());
        // One thread per request keeps this simple; a preview server never
        // sees meaningful concurrency.
        std::thread::spawn(move || {
            let resp = response(&request, &root, &base, livereload);
            let _ = request.respond(resp);
        });
    }
    Ok(())
}

/// Poll the source tree and rebuild when anything changes.
///
/// Polling rather than inotify keeps this dependency-free, and a preview
/// server can afford a walk twice a second.
fn spawn_watcher(source: PathBuf, destination: PathBuf, baseurl: Option<String>) {
    std::thread::spawn(move || {
        let mut previous = fingerprint(&source, &destination);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let current = fingerprint(&source, &destination);
            if current == previous {
                continue;
            }
            match crate::build::build(&source, &destination, baseurl.as_deref()) {
                Ok(()) => {
                    GENERATION.fetch_add(1, Ordering::SeqCst);
                    println!("      Regenerated: {}", chrono::Utc::now().format("%H:%M:%S UTC"));
                }
                Err(e) => eprintln!("       Build Error: {e:#}"),
            }
            // A build takes time and touches files; re-read so the build's own
            // output never looks like a fresh change.
            previous = fingerprint(&source, &destination);
        }
    });
}

/// A cheap summary of the source tree: how many files, and the newest mtime.
fn fingerprint(source: &Path, destination: &Path) -> (usize, u64) {
    let dest = destination.canonicalize().ok();
    let mut count = 0usize;
    let mut newest = 0u64;

    let walker = walkdir::WalkDir::new(source).into_iter().filter_entry(|e| {
        // Never descend into the destination, or the build would chase itself.
        if let Some(dest) = &dest {
            if e.path().canonicalize().ok().as_ref() == Some(dest) {
                return false;
            }
        }
        !matches!(e.file_name().to_str(), Some(".git") | Some(".jekyll-cache"))
    });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        count += 1;
        if let Some(t) = entry.metadata().ok().and_then(|m| m.modified().ok()) {
            let secs = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            newest = newest.max(secs);
        }
    }
    (count, newest)
}

/// Build the response for one request: a file under `root`, the generation
/// counter, or a 404. `tiny_http` drops the body itself for HEAD.
fn response(req: &tiny_http::Request, root: &Path, baseurl: &str, livereload: bool) -> Response {
    let text = |status: u16, body: String| {
        tiny_http::Response::from_data(body.into_bytes())
            .with_status_code(status)
            .with_header(header("Content-Type", "text/plain; charset=utf-8"))
    };
    if !matches!(req.method(), tiny_http::Method::Get | tiny_http::Method::Head) {
        return text(405, "Method Not Allowed".into());
    }

    // Strip the query string, then the configured baseurl.
    let path = req.url().split(['?', '#']).next().unwrap_or("/");
    let path = match path.strip_prefix(baseurl) {
        Some(rest) if !baseurl.is_empty() => rest,
        _ => path,
    };
    let decoded = crate::url::unescape_path(path);

    // The page polls this for the build generation.
    if decoded == LIVE_PATH {
        return text(200, GENERATION.load(Ordering::SeqCst).to_string());
    }
    let Some(file) = resolve(root, &decoded) else {
        return text(404, format!("404 Not Found\n\n{decoded}\n"));
    };
    let Ok(mut bytes) = std::fs::read(&file) else {
        return text(500, format!("could not read {}", file.display()));
    };

    let mime = mime_guess::from_path(&file).first_or_octet_stream();
    let is_text = mime.type_() == "text" || matches!(mime.subtype().as_str(), "json" | "javascript" | "xml");
    let content_type = if is_text { format!("{mime}; charset=utf-8") } else { mime.to_string() };
    if livereload && mime == mime_guess::mime::TEXT_HTML {
        bytes = inject_reload_script(bytes);
    }
    println!("  GET {decoded} -> {} ({} bytes)", file.display(), bytes.len());
    tiny_http::Response::from_data(bytes)
        .with_header(header("Content-Type", &content_type))
        .with_header(header("Cache-Control", "no-store"))
}

type Response = tiny_http::Response<std::io::Cursor<Vec<u8>>>;

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("ascii header")
}

/// Map a URL path to a file inside `root`, refusing anything that escapes it.
fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for segment in url_path.split('/') {
        match Path::new(segment).components().next() {
            None => continue,
            Some(Component::Normal(s)) => path.push(s),
            // "..", "." and absolute roots never reach the filesystem.
            Some(_) => return None,
        }
    }

    if path.is_dir() {
        let index = path.join("index.html");
        return index.is_file().then_some(index);
    }
    if path.is_file() {
        return Some(path);
    }
    // Jekyll's server also answers /about for /about.html.
    let html = path.with_extension("html");
    html.is_file().then_some(html)
}

/// Append the reload script to an HTML response.
///
/// Only what the server sends is changed; the file on disk is untouched, so a
/// byte-for-byte comparison against Jekyll still compares the real build.
fn inject_reload_script(body: Vec<u8>) -> Vec<u8> {
    let script = format!(
        "\n<script>(function(){{\
         var seen=null;\
         setInterval(function(){{\
           fetch('{LIVE_PATH}',{{cache:'no-store'}})\
             .then(function(r){{return r.text()}})\
             .then(function(g){{\
               if(seen===null){{seen=g;return}}\
               if(g!==seen){{location.reload()}}\
             }}).catch(function(){{}});\
         }},500);\
       }})();</script>\n"
    );

    let text = String::from_utf8_lossy(&body).into_owned();
    // Prefer just before </body>, so the script runs after the page parses.
    let out = match text.rfind("</body>") {
        Some(i) => format!("{}{}{}", &text[..i], script, &text[i..]),
        None => format!("{text}{script}"),
    };
    out.into_bytes()
}
