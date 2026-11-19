//! M66-08's first milestone: serve the reader's own files directly, under a per-run capability.
//!
//! No relay, DHT, discovery, account link or global namespace: this is one directory, one URL, one conversation. By default the
//! listener is loopback only; `--lan` is an explicit choice to expose it. Every request must carry the random path token, and
//! every resolved path is canonicalised back under the chosen root.

use std::path::{Path, PathBuf};

use clap::Args;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::rec::{bad_request, environment, Failure};

#[derive(Debug, Clone, Args)]
pub struct ShareArgs {
    /// Directory whose files may be served. Nothing outside it can be reached.
    #[arg(long, value_name = "DIR")]
    pub dir: PathBuf,

    /// Bind on all interfaces. Without this the server is loopback only.
    #[arg(long)]
    pub lan: bool,

    /// Port to bind; 0 lets the operating system choose.
    #[arg(long, default_value_t = 0)]
    pub port: u16,

    /// Exit after one request, for one-shot handoffs and tests.
    #[arg(long)]
    pub once: bool,
}

fn random_token() -> Result<String, Failure> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| environment(format!("cannot create a share token: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn decode_path(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            ) {
                out.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "txt" | "md" | "csv" | "json" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Resolve a request path. The token is a bearer capability: no token, no answer, not even an index.
fn resolve(root: &Path, token: &str, target: &str) -> Option<PathBuf> {
    let prefix = format!("/{token}/");
    let rest = target.strip_prefix(&prefix)?;
    let decoded = decode_path(rest.split('?').next().unwrap_or_default());
    if decoded.starts_with('/') || decoded.split('/').any(|part| part == "..") {
        return None;
    }
    let candidate = if decoded.is_empty() {
        root.to_path_buf()
    } else {
        root.join(decoded)
    };
    let canonical = candidate.canonicalize().ok()?;
    canonical.starts_with(root).then_some(canonical)
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[derive(Debug, Serialize)]
struct SearchHit {
    path: String,
    line: usize,
    text: String,
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if out.len() >= 100 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        if out.len() >= 100 {
            return;
        }
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

/// A bounded scan of the reader's own files. Binary files are skipped rather than guessed at; a file that cannot be read is
/// not a failed search, because one unreadable file must not reveal or hide the others.
fn search(root: &Path, query: &str) -> Vec<SearchHit> {
    let mut files = Vec::new();
    collect_files(root, &mut files);
    let needle = query.to_lowercase();
    let mut hits = Vec::new();
    for path in files {
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(root) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&canonical) else {
            continue;
        };
        if bytes.len() > 1024 * 1024 {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                let relative = canonical
                    .strip_prefix(root)
                    .unwrap_or(&canonical)
                    .to_string_lossy()
                    .into_owned();
                hits.push(SearchHit {
                    path: relative,
                    line: index + 1,
                    text: line.trim().to_owned(),
                });
                break;
            }
        }
    }
    hits
}

async fn send(mut socket: tokio::net::TcpStream, status: &str, kind: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = socket.write_all(head.as_bytes()).await;
    let _ = socket.write_all(body).await;
    let _ = socket.shutdown().await;
}

async fn handle(socket: tokio::net::TcpStream, root: PathBuf, token: String) {
    let mut socket = socket;
    let mut buffer = [0u8; 8192];
    let read = match socket.read(&mut buffer).await {
        Ok(read) => read,
        Err(_) => return,
    };
    let request = String::from_utf8_lossy(&buffer[..read]);
    let mut parts = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" {
        send(
            socket,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"GET only\n",
        )
        .await;
        return;
    }
    let Some(path) = resolve(&root, &token, target) else {
        send(
            socket,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found\n",
        )
        .await;
        return;
    };
    if path.is_dir() {
        let query = target
            .split_once('?')
            .and_then(|(_, query)| query.strip_prefix("q="))
            .map(decode_path)
            .unwrap_or_default();
        if !query.is_empty() {
            let body = serde_json::to_vec(&serde_json::json!({
                "query": query,
                "matches": search(&root, &query),
            }))
            .unwrap_or_default();
            send(socket, "200 OK", "application/json", &body).await;
            return;
        }
        let mut names: Vec<String> = match std::fs::read_dir(&path) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        let items = names
            .iter()
            .map(|name| {
                format!(
                    "<li><a href=\"{}\">{}</a></li>\n",
                    percent_encode(name),
                    escape_html(name)
                )
            })
            .collect::<String>();
        let body = format!("<!doctype html><meta charset=utf-8><title>Shared files</title><h1>Shared files</h1><ul>\n{items}</ul>");
        send(
            socket,
            "200 OK",
            "text/html; charset=utf-8",
            body.as_bytes(),
        )
        .await;
        return;
    }
    match std::fs::read(&path) {
        Ok(body) => send(socket, "200 OK", content_type(&path), &body).await,
        Err(_) => {
            send(
                socket,
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"not found\n",
            )
            .await
        }
    }
}

fn percent_encode(raw: &str) -> String {
    raw.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

pub async fn run(args: &ShareArgs) -> Result<(), Failure> {
    if !args.dir.is_dir() {
        return Err(bad_request(format!(
            "--dir is not a directory: {}",
            args.dir.display()
        )));
    }
    let root = args
        .dir
        .canonicalize()
        .map_err(|error| environment(format!("cannot resolve --dir: {error}")))?;
    let token = random_token()?;
    let bind = if args.lan { "0.0.0.0" } else { "127.0.0.1" };
    let listener = TcpListener::bind(format!("{bind}:{}", args.port))
        .await
        .map_err(|error| environment(format!("cannot bind share listener: {error}")))?;
    let bound = listener
        .local_addr()
        .map_err(|error| environment(format!("cannot read share address: {error}")))?;
    println!("SHARE_BOUND={bound}");
    println!("SHARE_URL=http://{bound}/{token}/");
    println!("SHARE_MODE={}", if args.lan { "lan" } else { "loopback" });
    let _ = std::io::Write::flush(&mut std::io::stdout());

    loop {
        let (socket, _) = listener
            .accept()
            .await
            .map_err(|error| environment(format!("share listener: {error}")))?;
        handle(socket, root.clone(), token.clone()).await;
        if args.once {
            break;
        }
    }
    Ok(())
}
