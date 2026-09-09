//! A static file server for previewing a build.
//!
//! Deliberately minimal and dependency-free: enough HTTP/1.1 to serve a built
//! site to a browser on localhost. It is a preview server, not a web server,
//! and it does not watch or rebuild.

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

/// Jekyll's defaults.
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 4000;

pub fn serve(root: &Path, host: &str, port: u16, baseurl: &str) -> Result<()> {
    let listener = TcpListener::bind((host, port))
        .with_context(|| format!("binding {host}:{port}"))?;

    let base = baseurl.trim_end_matches('/').to_string();
    println!("    Server address: http://{host}:{port}{base}/");
    println!("  Server running... press ctrl-c to stop.");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let root = root.to_path_buf();
                let base = base.clone();
                // One thread per connection keeps this simple; a preview
                // server never sees meaningful concurrency.
                std::thread::spawn(move || {
                    let _ = handle(stream, &root, &base);
                });
            }
            Err(e) => eprintln!("  connection error: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, root: &Path, baseurl: &str) -> Result<()> {
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

    let Some(file) = resolve(root, &decoded) else {
        let body = format!("404 Not Found\n\n{decoded}\n");
        return respond(&mut stream, 404, "text/plain; charset=utf-8", body.as_bytes(), false);
    };

    let mut bytes = Vec::new();
    std::fs::File::open(&file)?.read_to_end(&mut bytes)?;
    let mime = mime_for(&file);
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
