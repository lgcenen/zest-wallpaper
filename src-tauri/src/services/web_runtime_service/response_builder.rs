use std::{borrow::Cow, net::TcpStream, path::Path};

use super::{bridge_injection::prepare_web_runtime_body, path_resolution::guess_content_type};

pub(super) struct HttpResponse<'a> {
    status_line: &'a str,
    content_type: &'a str,
    body: Cow<'a, [u8]>,
}

impl<'a> HttpResponse<'a> {
    pub(super) fn method_not_allowed() -> Self {
        Self::plain_text("HTTP/1.1 405 Method Not Allowed", b"Method Not Allowed")
    }

    pub(super) fn not_found() -> Self {
        Self::plain_text("HTTP/1.1 404 Not Found", b"Not Found")
    }

    pub(super) fn forbidden() -> Self {
        Self::plain_text("HTTP/1.1 403 Forbidden", b"Forbidden")
    }

    pub(super) fn from_asset(path: &Path, body: Vec<u8>) -> Self {
        let body = prepare_web_runtime_body(path, body);
        Self {
            status_line: "HTTP/1.1 200 OK",
            content_type: guess_content_type(path),
            body: Cow::Owned(body),
        }
    }

    fn plain_text(status_line: &'a str, body: &'static [u8]) -> Self {
        Self {
            status_line,
            content_type: "text/plain; charset=utf-8",
            body: Cow::Borrowed(body),
        }
    }
}

pub(super) fn write_http_response(
    mut stream: TcpStream,
    response: HttpResponse<'_>,
    send_body: bool,
) {
    let header = format!(
        "{}\r\nContent-Length: {}\r\nContent-Type: {}\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        response.status_line,
        response.body.len(),
        response.content_type
    );
    let _ = std::io::Write::write_all(&mut stream, header.as_bytes());
    if send_body {
        let _ = std::io::Write::write_all(&mut stream, response.body.as_ref());
    }
    let _ = std::io::Write::flush(&mut stream);
}
