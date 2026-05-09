use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex, OnceLock},
    thread,
};

use super::{
    bridge_injection::prepare_web_runtime_body,
    request_resolution::{
        guess_content_type, parse_runtime_request_target, resolve_runtime_asset,
        AssetResolutionError,
    },
};

pub(super) struct WebRuntimeServer {
    pub(super) port: u16,
    pub(super) roots: Arc<StdMutex<HashMap<String, PathBuf>>>,
}

static WEB_RUNTIME_SERVER: OnceLock<Result<WebRuntimeServer, String>> = OnceLock::new();

pub(super) fn ensure_web_runtime_server() -> Result<&'static WebRuntimeServer, String> {
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

    let Some(request) = parse_runtime_request_target(target) else {
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
        roots.get(&request.token).cloned()
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

    let canonical = match resolve_runtime_asset(&root, &request.relative_path) {
        Ok(path) => path,
        Err(AssetResolutionError::Forbidden) => {
            write_http_response(
                stream,
                "HTTP/1.1 403 Forbidden",
                "text/plain; charset=utf-8",
                b"Forbidden",
                send_body,
            );
            return Ok(());
        }
        Err(AssetResolutionError::NotFound) => {
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
