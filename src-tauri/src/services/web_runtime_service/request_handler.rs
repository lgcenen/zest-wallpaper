use std::{
    fs,
    io::{BufRead, BufReader},
    net::TcpStream,
};

use super::{
    path_resolution::{parse_runtime_request_target, resolve_runtime_asset, AssetResolutionError},
    response_builder::{write_http_response, HttpResponse},
    root_registry::RuntimeRootRegistry,
};

pub(super) fn handle_web_runtime_connection(
    stream: TcpStream,
    roots: &RuntimeRootRegistry,
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
        write_http_response(stream, HttpResponse::method_not_allowed(), send_body);
        return Ok(());
    }

    let Some(request) = parse_runtime_request_target(target) else {
        write_http_response(stream, HttpResponse::not_found(), send_body);
        return Ok(());
    };

    let Some(root) = roots.resolve(&request.token)? else {
        write_http_response(stream, HttpResponse::not_found(), send_body);
        return Ok(());
    };

    let canonical = match resolve_runtime_asset(&root, &request.relative_path) {
        Ok(path) => path,
        Err(AssetResolutionError::Forbidden) => {
            write_http_response(stream, HttpResponse::forbidden(), send_body);
            return Ok(());
        }
        Err(AssetResolutionError::NotFound) => {
            write_http_response(stream, HttpResponse::not_found(), send_body);
            return Ok(());
        }
    };

    let body = match fs::read(&canonical) {
        Ok(body) => body,
        Err(_) => {
            write_http_response(stream, HttpResponse::not_found(), send_body);
            return Ok(());
        }
    };
    write_http_response(
        stream,
        HttpResponse::from_asset(&canonical, body),
        send_body,
    );
    Ok(())
}
