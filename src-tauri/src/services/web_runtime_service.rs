use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    fs,
    hash::{Hash, Hasher},
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, OnceLock},
    thread,
};

const HTML_WALLPAPER_BRIDGE: &str = include_str!("html_wallpaper_bridge.js");

struct WebRuntimeServer {
    port: u16,
    roots: Arc<StdMutex<HashMap<String, PathBuf>>>,
}

static WEB_RUNTIME_SERVER: OnceLock<Result<WebRuntimeServer, String>> = OnceLock::new();

fn guess_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("txt") => "text/plain; charset=utf-8",
        Some("splat") => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

fn url_encode_component(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => {
                let _ = std::fmt::Write::write_fmt(&mut encoded, format_args!("%{byte:02X}"));
            }
        }
    }
    encoded
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hi = bytes[index + 1] as char;
            let lo = bytes[index + 2] as char;
            if let (Some(hi), Some(lo)) = (hi.to_digit(16), lo.to_digit(16)) {
                decoded.push(((hi << 4) + lo) as u8);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn write_http_response(
    mut stream: TcpStream,
    status_line: &str,
    content_type: &str,
    body: &[u8],
    send_body: bool,
) {
    let header = format!(
        "{status_line}\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    if send_body {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}

fn is_html_asset(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("html") | Some("htm")
    )
}

fn escape_inline_script(value: &str) -> String {
    value
        .replace("</script", "<\\/script")
        .replace("</SCRIPT", "<\\/script")
}

fn inject_html_wallpaper_bridge(html: &str) -> String {
    let bridge = format!(
        "<script>{}</script>",
        escape_inline_script(HTML_WALLPAPER_BRIDGE)
    );

    let lower = html.to_ascii_lowercase();
    if lower.contains("<head") {
        return inject_after_head_tag(html, &bridge);
    }

    if let Some(start) = lower.find("<html") {
        if let Some(close_offset) = lower[start..].find('>') {
            let insert_at = start + close_offset + 1;
            let mut injected = String::with_capacity(html.len() + bridge.len() + 13);
            injected.push_str(&html[..insert_at]);
            injected.push_str("<head>");
            injected.push_str(&bridge);
            injected.push_str("</head>");
            injected.push_str(&html[insert_at..]);
            return injected;
        }
    }

    format!("<!DOCTYPE html><html><head>{bridge}</head><body>{html}</body></html>")
}

fn inject_after_head_tag(html: &str, bridge: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if let Some(start) = lower.find("<head") {
        if let Some(close_offset) = lower[start..].find('>') {
            let insert_at = start + close_offset + 1;
            let mut injected = String::with_capacity(html.len() + bridge.len());
            injected.push_str(&html[..insert_at]);
            injected.push_str(bridge);
            injected.push_str(&html[insert_at..]);
            return injected;
        }
    }
    html.to_string()
}

fn prepare_web_runtime_body(path: &Path, body: Vec<u8>) -> Vec<u8> {
    if !is_html_asset(path) {
        return body;
    }

    match String::from_utf8(body) {
        Ok(html) => inject_html_wallpaper_bridge(&html).into_bytes(),
        Err(error) => error.into_bytes(),
    }
}

fn handle_web_runtime_connection(
    stream: TcpStream,
    roots: &Arc<StdMutex<HashMap<String, PathBuf>>>,
) -> Result<(), String> {
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("failed to clone stream: {error}"))?,
    );
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| format!("failed to read request line: {error}"))?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or("/");
    let send_body = method != "HEAD";
    if method != "GET" && method != "HEAD" {
        write_http_response(
            stream,
            "HTTP/1.1 405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
            send_body,
        );
        return Ok(());
    }

    let target = target.split('?').next().unwrap_or("/");
    let Some(path) = target.strip_prefix("/web-runtime/") else {
        write_http_response(
            stream,
            "HTTP/1.1 404 Not Found",
            "text/plain; charset=utf-8",
            b"Not Found",
            send_body,
        );
        return Ok(());
    };

    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let Some(token) = segments.next() else {
        write_http_response(
            stream,
            "HTTP/1.1 404 Not Found",
            "text/plain; charset=utf-8",
            b"Not Found",
            send_body,
        );
        return Ok(());
    };

    let root = {
        let roots = roots.lock().map_err(|error| error.to_string())?;
        roots.get(token).cloned()
    };
    let Some(root) = root else {
        write_http_response(
            stream,
            "HTTP/1.1 404 Not Found",
            "text/plain; charset=utf-8",
            b"Not Found",
            send_body,
        );
        return Ok(());
    };

    let relative_segments = segments.map(percent_decode).collect::<Vec<_>>();
    let relative = if relative_segments.is_empty() {
        PathBuf::from("index.html")
    } else {
        relative_segments
            .iter()
            .fold(PathBuf::new(), |mut path, segment| {
                path.push(segment);
                path
            })
    };

    let candidate = root.join(relative);
    let canonical = match candidate.canonicalize() {
        Ok(path) => path,
        Err(_) => {
            write_http_response(
                stream,
                "HTTP/1.1 404 Not Found",
                "text/plain; charset=utf-8",
                b"Not Found",
                send_body,
            );
            return Ok(());
        }
    };
    if !canonical.starts_with(&root) {
        write_http_response(
            stream,
            "HTTP/1.1 403 Forbidden",
            "text/plain; charset=utf-8",
            b"Forbidden",
            send_body,
        );
        return Ok(());
    }

    let body = match fs::read(&canonical) {
        Ok(body) => body,
        Err(_) => {
            write_http_response(
                stream,
                "HTTP/1.1 404 Not Found",
                "text/plain; charset=utf-8",
                b"Not Found",
                send_body,
            );
            return Ok(());
        }
    };
    let body = prepare_web_runtime_body(&canonical, body);
    let content_type = guess_content_type(&canonical);
    write_http_response(stream, "HTTP/1.1 200 OK", content_type, &body, send_body);
    Ok(())
}

fn ensure_web_runtime_server() -> Result<&'static WebRuntimeServer, String> {
    let stored = WEB_RUNTIME_SERVER.get_or_init(|| {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("failed to bind web runtime server: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("failed to inspect web runtime port: {error}"))?
            .port();
        let roots = Arc::new(StdMutex::new(HashMap::new()));
        let thread_roots = Arc::clone(&roots);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let roots = Arc::clone(&thread_roots);
                thread::spawn(move || {
                    let _ = handle_web_runtime_connection(stream, &roots);
                });
            }
        });
        Ok(WebRuntimeServer { port, roots })
    });

    match stored {
        Ok(server) => Ok(server),
        Err(error) => Err(error.clone()),
    }
}

pub fn get_web_runtime_url(path: &str) -> Result<String, String> {
    let entry_path = PathBuf::from(path);
    let root = entry_path
        .parent()
        .ok_or_else(|| "web wallpaper entry path is missing a parent directory".to_string())?
        .canonicalize()
        .map_err(|error| format!("failed to resolve web runtime root: {error}"))?;
    let entry_name = entry_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "web wallpaper entry path is missing a file name".to_string())?;

    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    let token = format!("{:016x}", hasher.finish());

    let server = ensure_web_runtime_server()?;
    {
        let mut roots = server.roots.lock().map_err(|error| error.to_string())?;
        roots.insert(token.clone(), root);
    }

    Ok(format!(
        "http://127.0.0.1:{}/web-runtime/{}/{}",
        server.port,
        token,
        url_encode_component(entry_name)
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        net::TcpStream,
    };

    use tempfile::tempdir;

    use super::{
        ensure_web_runtime_server, get_web_runtime_url, inject_html_wallpaper_bridge,
        prepare_web_runtime_body, HTML_WALLPAPER_BRIDGE,
    };

    #[test]
    fn injects_bridge_into_html_document() {
        let html = "<html><head><title>Demo</title></head><body>Hello</body></html>";
        let injected = inject_html_wallpaper_bridge(html);
        assert!(injected.contains("wallpaperPropertyListener"));
        assert!(injected.contains("__wallpaperApplyRuntimeMessage"));
        assert!(injected.contains("wallpaperRegisterAudioListener"));
        assert!(injected.contains("Hello"));
        assert!(injected.contains("Demo"));
    }

    #[test]
    fn does_not_modify_non_html_assets() {
        let original = b"gaussian splat payload".to_vec();
        let prepared =
            prepare_web_runtime_body(std::path::Path::new("test.splat"), original.clone());
        assert_eq!(prepared, original);
    }

    #[test]
    fn runtime_server_injects_html_and_serves_relative_splat_asset() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(
            root.join("index.html"),
            "<html><head><title>Demo</title></head><body>ok</body></html>",
        )
        .expect("index");
        fs::write(root.join("test.splat"), b"splat-payload").expect("splat");

        let runtime_url =
            get_web_runtime_url(root.join("index.html").to_str().expect("entry path"))
                .expect("runtime url");
        let server = ensure_web_runtime_server().expect("runtime server");

        let index_path = runtime_url
            .strip_prefix(&format!("http://127.0.0.1:{}", server.port))
            .expect("runtime path");
        let index_response = get_http_response(server.port, index_path);
        assert!(index_response.contains("HTTP/1.1 200 OK"));
        assert!(index_response.contains("wallpaperPropertyListener"));
        assert!(index_response.contains("__wallpaperApplyRuntimeMessage"));
        assert!(index_response.contains("wallpaperRegisterAudioListener"));
        assert!(!index_response.contains("blob:"));
        assert!(index_response.contains("Demo"));

        let splat_path = index_path
            .rsplit_once('/')
            .map(|(prefix, _)| format!("{prefix}/test.splat"))
            .expect("splat path");
        let splat_response = get_http_response(server.port, &splat_path);
        assert!(splat_response.contains("HTTP/1.1 200 OK"));
        assert!(splat_response.ends_with("splat-payload"));
        assert!(!splat_response.contains(HTML_WALLPAPER_BRIDGE.trim()));
    }

    #[test]
    fn runtime_server_blocks_path_traversal() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("index.html"), "<html><body>ok</body></html>").expect("index");

        let runtime_url =
            get_web_runtime_url(root.join("index.html").to_str().expect("entry path"))
                .expect("runtime url");
        let server = ensure_web_runtime_server().expect("runtime server");
        let index_path = runtime_url
            .strip_prefix(&format!("http://127.0.0.1:{}", server.port))
            .expect("runtime path");
        let traversal_path = index_path
            .rsplit_once('/')
            .map(|(prefix, _)| format!("{prefix}/../secret.txt"))
            .expect("traversal path");

        let response = get_http_response(server.port, &traversal_path);
        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden")
                || response.starts_with("HTTP/1.1 404 Not Found")
        );
    }

    fn get_http_response(port: u16, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        response
    }
}
