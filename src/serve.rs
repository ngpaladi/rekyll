//! A static file server for previewing a build, with optional watch and
//! live reload.
//!
//! Deliberately dependency-free: enough HTTP/1.1 to serve a built site to a
//! browser on localhost, a poll-based watcher, and a generation counter the
//! page checks to know when to reload. It is a preview server, not a web
//! server.

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
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
}

/// Bumped on every successful rebuild. The page polls it and reloads when the
/// number it sees changes.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn serve(opts: Options) -> Result<()> {
    let listener = TcpListener::bind((opts.host, opts.port))
        .with_context(|| format!("binding {}:{}", opts.host, opts.port))?;

    let base = opts.baseurl.trim_end_matches('/').to_string();
    let root = opts.root.to_path_buf();
    let livereload = opts.livereload;

    if let Some(watch) = opts.watch {
        spawn_watcher(watch.source.to_path_buf(), watch.destination.to_path_buf());
    }

    println!("    Server address: http://{}:{}{base}/", opts.host, opts.port);
    if livereload {
        println!("       Live reload: enabled");
    }
    println!("  Server running... press ctrl-c to stop.");

    let base = Arc::new(base);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let root = root.clone();
                let base = base.clone();
                // One thread per connection keeps this simple; a preview
                // server never sees meaningful concurrency.
                std::thread::spawn(move || {
                    let _ = handle(stream, &root, &base, livereload);
                });
            }
            Err(e) => eprintln!("  connection error: {e}"),
        }
    }
    Ok(())
}

/// Poll the source tree and rebuild when anything changes.
///
/// Polling rather than inotify keeps this dependency-free, and a preview
/// server can afford a walk twice a second.
fn spawn_watcher(source: PathBuf, destination: PathBuf) {
    std::thread::spawn(move || {
        let mut previous = fingerprint(&source, &destination);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let current = fingerprint(&source, &destination);
            if current == previous {
                continue;
            }
            previous = current;
            match crate::build::build(&source, &destination) {
                Ok(()) => {
                    GENERATION.fetch_add(1, Ordering::SeqCst);
                    println!("      Regenerated: {}", now_hms());
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

fn now_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02} UTC")
}

fn handle(mut stream: TcpStream, root: &Path, baseurl: &str, livereload: bool) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }

    // Drain the headers so the client sees a clean response.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method != "GET" && method != "HEAD" {
        return respond(&mut stream, 405, "text/plain; charset=utf-8", b"Method Not Allowed", false);
    }

    // Strip the query string, then the configured baseurl.
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let path = match path.strip_prefix(baseurl) {
        Some(rest) if !baseurl.is_empty() => rest,
        _ => path,
    };
    let decoded = crate::url::unescape_path(path);

    // The page polls this for the build generation.
    if decoded == LIVE_PATH {
        let body = GENERATION.load(Ordering::SeqCst).to_string();
        return respond(&mut stream, 200, "text/plain; charset=utf-8", body.as_bytes(), false);
    }

    let Some(file) = resolve(root, &decoded) else {
        let body = format!("404 Not Found\n\n{decoded}\n");
        return respond(&mut stream, 404, "text/plain; charset=utf-8", body.as_bytes(), false);
    };

    let mut bytes = Vec::new();
    std::fs::File::open(&file)?.read_to_end(&mut bytes)?;
    let mime = mime_for(&file);
    if livereload && mime.starts_with("text/html") {
        bytes = inject_reload_script(bytes);
    }
    println!("  GET {decoded} -> {} ({} bytes)", file.display(), bytes.len());
    respond(&mut stream, 200, mime, &bytes, method == "HEAD")
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

fn respond(
    stream: &mut TcpStream,
    status: u16,
    mime: &str,
    body: &[u8],
    head_only: bool,
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {mime}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n",
        body.len()
    )?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()?;
    Ok(())
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" | "atom" | "rss" => "application/xml; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
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
