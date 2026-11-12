//! HTTP file server: GET/HEAD with directory listings and single byte
//! ranges (so downloads can resume), and PUT uploads when allowed (`curl -T
//! file http://host:port/`), optionally behind a Basic login. Plain HTTP:
//! the guide says so.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Full, StreamBody, combinators::BoxBody};
use hyper::body::{Frame, Incoming};
use hyper::header::{self, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;

use base64::Engine;

use crate::{
    Action, PathError, ServerError, Shared, constant_time_eq, display_path, resolve_existing,
    resolve_new,
};

type Body = BoxBody<Bytes, std::io::Error>;

/// Connections at once.
const MAX_CONNECTIONS: usize = 128;
/// Largest PUT body accepted.
const MAX_UPLOAD: u64 = 16 * 1024 * 1024 * 1024;

pub(crate) async fn serve(
    addr: SocketAddr,
    shared: Arc<Shared>,
    stop: CancellationToken,
    login: Option<(String, String)>,
) -> Result<(SocketAddr, JoinHandle<()>), ServerError> {
    // The credentials as a Basic token, compared with what clients send.
    let token: Option<Arc<str>> = login.map(|(user, password)| {
        base64::engine::general_purpose::STANDARD
            .encode(format!("{user}:{password}"))
            .into()
    });
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|source| ServerError::Listen { addr, source })?;
    let local = listener
        .local_addr()
        .map_err(|source| ServerError::Listen { addr, source })?;
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let task = tokio::spawn(async move {
        loop {
            let (stream, peer) = tokio::select! {
                () = stop.cancelled() => return,
                r = listener.accept() => match r {
                    Ok(r) => r,
                    Err(_) => continue,
                },
            };
            let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
                drop(stream);
                continue;
            };
            let (shared, stop, token) = (Arc::clone(&shared), stop.clone(), token.clone());
            tokio::spawn(async move {
                let _permit = permit;
                let service = hyper::service::service_fn(move |req| {
                    let (shared, token) = (Arc::clone(&shared), token.clone());
                    async move {
                        let resp = match authorize(&req, token.as_deref()) {
                            Auth::Ok => handle(req, peer, &shared).await,
                            Auth::Missing => unauthorized(),
                            Auth::Wrong => {
                                let shown = decode_path(req.uri().path())
                                    .map_or_else(|| "/".to_owned(), |p| display_path(&p));
                                shared.report(
                                    peer,
                                    Action::Login,
                                    &shown,
                                    0,
                                    Some("wrong user name or password".into()),
                                );
                                unauthorized()
                            }
                        };
                        Ok::<_, Infallible>(resp)
                    }
                });
                let conn = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service);
                tokio::select! {
                    () = stop.cancelled() => {}
                    _ = conn => {}
                }
            });
        }
    });
    Ok((local, task))
}

enum Auth {
    Ok,
    /// No credentials yet (a browser's first request): ask for them.
    Missing,
    Wrong,
}

/// Checks the `Authorization: Basic …` header against the expected token.
fn authorize<B>(req: &Request<B>, token: Option<&str>) -> Auth {
    let Some(token) = token else {
        return Auth::Ok;
    };
    let Some(value) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return Auth::Missing;
    };
    match value.trim().split_once(' ') {
        Some((scheme, given))
            if scheme.eq_ignore_ascii_case("basic")
                && constant_time_eq(given.trim().as_bytes(), token.as_bytes()) =>
        {
            Auth::Ok
        }
        _ => Auth::Wrong,
    }
}

fn unauthorized() -> Response<Body> {
    let mut r = full(StatusCode::UNAUTHORIZED, "user name and password needed");
    r.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"Termalith\", charset=\"UTF-8\""),
    );
    r
}

fn full(status: StatusCode, text: &str) -> Response<Body> {
    let mut r = Response::new(
        Full::new(Bytes::from(format!("{text}\n")))
            .map_err(|never| match never {})
            .boxed(),
    );
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    r
}

async fn handle(req: Request<Incoming>, peer: SocketAddr, shared: &Shared) -> Response<Body> {
    let Some(path) = decode_path(req.uri().path()) else {
        return full(StatusCode::BAD_REQUEST, "bad path");
    };
    let shown = display_path(&path);
    match *req.method() {
        Method::GET | Method::HEAD => {
            let head = req.method() == Method::HEAD;
            let range = req
                .headers()
                .get(header::RANGE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            match get(&path, &shown, head, range.as_deref(), shared).await {
                Ok((resp, action, bytes)) => {
                    if !head {
                        shared.report(peer, action, &shown, bytes, None);
                    }
                    resp
                }
                Err((status, why)) => {
                    shared.report(peer, Action::Download, &shown, 0, Some(why.to_owned()));
                    full(status, why)
                }
            }
        }
        Method::PUT => {
            let result = put(req, &path, shared).await;
            match result {
                Ok(bytes) => {
                    shared.report(peer, Action::Upload, &shown, bytes, None);
                    full(StatusCode::CREATED, "uploaded")
                }
                Err((status, why)) => {
                    shared.report(peer, Action::Upload, &shown, 0, Some(why.to_owned()));
                    full(status, why)
                }
            }
        }
        _ => {
            let mut r = full(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
            r.headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD, PUT"));
            r
        }
    }
}

type Fail = (StatusCode, &'static str);

fn path_fail(e: PathError) -> Fail {
    match e {
        PathError::NotFound => (StatusCode::NOT_FOUND, "not found"),
        PathError::Outside => (StatusCode::FORBIDDEN, "forbidden"),
    }
}

async fn get(
    path: &str,
    shown: &str,
    head: bool,
    range: Option<&str>,
    shared: &Shared,
) -> Result<(Response<Body>, Action, u64), Fail> {
    let real = resolve_existing(&shared.root, path).map_err(path_fail)?;
    let meta = tokio::fs::metadata(&real)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "not found"))?;
    if meta.is_dir() {
        // Links in a listing are relative, so the folder URL needs a slash.
        if !path.ends_with('/') && !path.is_empty() {
            let mut r = full(StatusCode::MOVED_PERMANENTLY, "moved");
            let location = format!("{}/", encode_path(shown));
            if let Ok(v) = HeaderValue::from_str(&location) {
                r.headers_mut().insert(header::LOCATION, v);
            }
            return Ok((r, Action::List, 0));
        }
        let html = listing(&real, shown, shared.allow_uploads)
            .await
            .map_err(|_| (StatusCode::FORBIDDEN, "cannot list the folder"))?;
        let len = html.len() as u64;
        let mut r = Response::new(if head {
            empty()
        } else {
            Full::new(Bytes::from(html))
                .map_err(|never| match never {})
                .boxed()
        });
        let h = r.headers_mut();
        h.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
        return Ok((r, Action::List, 0));
    }
    if !meta.is_file() {
        return Err((StatusCode::FORBIDDEN, "not a file"));
    }
    let size = meta.len();
    let (status, start, end) = match range.map(|r| parse_range(r, size)) {
        None => (StatusCode::OK, 0, size),
        Some(Some((s, e))) => (StatusCode::PARTIAL_CONTENT, s, e),
        Some(None) => {
            let mut r = full(StatusCode::RANGE_NOT_SATISFIABLE, "range not satisfiable");
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{size}")) {
                r.headers_mut().insert(header::CONTENT_RANGE, v);
            }
            return Ok((r, Action::Download, 0));
        }
    };
    let len = end - start;
    let body = if head || len == 0 {
        empty()
    } else {
        let mut file = tokio::fs::File::open(&real)
            .await
            .map_err(|_| (StatusCode::FORBIDDEN, "cannot read the file"))?;
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "read error"))?;
        let stream =
            ReaderStream::new(tokio::io::AsyncReadExt::take(file, len)).map_ok(Frame::data);
        StreamBody::new(stream).boxed()
    };
    let mut r = Response::new(body);
    *r.status_mut() = status;
    let h = r.headers_mut();
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&real)),
    );
    if status == StatusCode::PARTIAL_CONTENT
        && let Ok(v) = HeaderValue::from_str(&format!("bytes {start}-{}/{size}", end - 1))
    {
        h.insert(header::CONTENT_RANGE, v);
    }
    Ok((r, Action::Download, len))
}

fn empty() -> Body {
    Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed()
}

/// A single `bytes=` range as `[start, end)`; `None` if it cannot be served.
/// Several ranges are not supported (the whole range header is then refused).
fn parse_range(header: &str, size: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let (start, end) = if a.is_empty() {
        // Suffix: the last `b` bytes.
        let n: u64 = b.parse().ok()?;
        if n == 0 {
            return None;
        }
        (size.saturating_sub(n), size)
    } else {
        let s: u64 = a.parse().ok()?;
        let e = if b.is_empty() {
            size
        } else {
            b.parse::<u64>().ok()?.saturating_add(1).min(size)
        };
        (s, e)
    };
    (start < end && start < size).then_some((start, end))
}

async fn put(req: Request<Incoming>, path: &str, shared: &Shared) -> Result<u64, Fail> {
    if !shared.allow_uploads {
        return Err((StatusCode::FORBIDDEN, "uploads are turned off"));
    }
    if req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
        .is_some_and(|n| n > MAX_UPLOAD)
    {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "file too large"));
    }
    let target = resolve_new(&shared.root, path).map_err(path_fail)?;
    let mut file = match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .await
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err((StatusCode::CONFLICT, "file already exists"));
        }
        Err(_) => return Err((StatusCode::FORBIDDEN, "cannot create the file")),
    };
    let mut body = req.into_body();
    let mut written = 0u64;
    let result: Result<(), Fail> = async {
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| (StatusCode::BAD_REQUEST, "upload interrupted"))?;
            if let Ok(data) = frame.into_data() {
                written += data.len() as u64;
                if written > MAX_UPLOAD {
                    return Err((StatusCode::PAYLOAD_TOO_LARGE, "file too large"));
                }
                file.write_all(&data)
                    .await
                    .map_err(|_| (StatusCode::INSUFFICIENT_STORAGE, "write error"))?;
            }
        }
        file.flush()
            .await
            .map_err(|_| (StatusCode::INSUFFICIENT_STORAGE, "write error"))
    }
    .await;
    if let Err(e) = result {
        drop(file);
        let _ = tokio::fs::remove_file(&target).await;
        return Err(e);
    }
    Ok(written)
}

/// Percent-decodes a URL path; `None` for invalid escapes or UTF-8.
fn decode_path(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Percent-encodes a path for a link (keeps `/` and unreserved characters).
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn listing(dir: &Path, shown: &str, uploads: bool) -> std::io::Result<String> {
    let mut entries = Vec::new();
    let mut rd = tokio::fs::read_dir(dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let name = e.file_name().to_string_lossy().into_owned();
        let meta = e.metadata().await.ok();
        let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
        let size = meta.as_ref().filter(|m| m.is_file()).map(|m| m.len());
        entries.push((is_dir, name, size));
    }
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    let title = escape_html(shown);
    let mut html = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
         <title>{title}</title><style>body{{font:14px system-ui,sans-serif;margin:24px}}\
         td{{padding:2px 16px 2px 0}}td.s{{text-align:right;color:#666}}</style>\
         <h1>{title}</h1><table>"
    );
    if shown != "/" {
        html.push_str("<tr><td><a href=\"../\">../</a></td><td></td></tr>");
    }
    for (is_dir, name, size) in entries {
        let slash = if is_dir { "/" } else { "" };
        let href = format!("{}{slash}", encode_path(&name));
        let size = size.map(|s| s.to_string()).unwrap_or_default();
        html.push_str(&format!(
            "<tr><td><a href=\"{href}\">{}{slash}</a></td><td class=s>{size}</td></tr>",
            escape_html(&name)
        ));
    }
    html.push_str("</table>");
    if uploads {
        html.push_str("<p>Upload with <code>curl -T FILE URL</code>.</p>");
    }
    Ok(html)
}

fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "txt" | "log" | "cfg" | "conf" | "ini" | "md" | "csv" => "text/plain; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" => "text/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "iso" => "application/x-iso9660-image",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_parse() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 100)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 1000)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 1000)));
        assert_eq!(parse_range("bytes=990-2000", 1000), Some((990, 1000)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=0-1,5-9", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }

    #[test]
    fn paths_decode_and_encode() {
        assert_eq!(decode_path("/a%20b/%D8%B3.txt").unwrap(), "/a b/س.txt");
        assert_eq!(decode_path("/%zz"), None);
        assert_eq!(decode_path("/%ff"), None, "not UTF-8");
        assert_eq!(encode_path("/a b/<x>"), "/a%20b/%3Cx%3E");
        assert_eq!(
            escape_html("<a href=\"x\">&"),
            "&lt;a href=&quot;x&quot;&gt;&amp;"
        );
    }
}
