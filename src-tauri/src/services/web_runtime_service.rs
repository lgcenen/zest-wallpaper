mod bridge_injection;
mod path_resolution;
mod request_handler;
mod response_builder;
mod root_registry;
mod server;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebRuntimeSession {
    runtime_id: String,
    runtime_url: String,
}

impl WebRuntimeSession {
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub fn runtime_url(&self) -> &str {
        &self.runtime_url
    }
}

pub fn prepare_web_runtime_session(path: &str) -> Result<WebRuntimeSession, String> {
    let entry = path_resolution::resolve_runtime_entry(path)?;
    let server = server::ensure_web_runtime_server()?;
    server::register_runtime_root(server, entry.token.clone(), entry.root.clone())?;

    Ok(WebRuntimeSession {
        runtime_id: path_resolution::build_runtime_id(&entry),
        runtime_url: path_resolution::build_runtime_url(server.port, &entry),
    })
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
        bridge_injection::{
            inject_html_wallpaper_bridge, prepare_web_runtime_body, HTML_WALLPAPER_BRIDGE,
        },
        prepare_web_runtime_session,
        server::ensure_web_runtime_server,
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

        let runtime_url = prepare_web_runtime_session(
            root.join("index.html").to_str().expect("entry path"),
        )
        .expect("runtime session")
        .runtime_url()
        .to_string();
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
    fn runtime_session_registers_root_and_exposes_runtime_url() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("index.html"), "<html><body>ok</body></html>").expect("index");

        let session =
            prepare_web_runtime_session(root.join("index.html").to_str().expect("entry path"))
                .expect("runtime session");
        let server = ensure_web_runtime_server().expect("runtime server");

        assert_eq!(
            session.runtime_url(),
            format!(
                "http://127.0.0.1:{}/web-runtime/{}/index.html",
                server.port,
                runtime_token(root)
            )
        );
        assert_eq!(
            session.runtime_id(),
            format!("{}/index.html", runtime_token(root))
        );
    }

    #[test]
    fn runtime_server_blocks_path_traversal() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("index.html"), "<html><body>ok</body></html>").expect("index");

        let runtime_url = prepare_web_runtime_session(
            root.join("index.html").to_str().expect("entry path"),
        )
        .expect("runtime session")
        .runtime_url()
        .to_string();
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

    #[test]
    fn runtime_server_supports_head_without_body() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(
            root.join("index.html"),
            "<html><head><title>Demo</title></head><body>ok</body></html>",
        )
        .expect("index");

        let runtime_url = prepare_web_runtime_session(
            root.join("index.html").to_str().expect("entry path"),
        )
        .expect("runtime session")
        .runtime_url()
        .to_string();
        let server = ensure_web_runtime_server().expect("runtime server");
        let index_path = runtime_url
            .strip_prefix(&format!("http://127.0.0.1:{}", server.port))
            .expect("runtime path");

        let response = get_http_response_with_method(server.port, index_path, "HEAD");
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Type: text/html; charset=utf-8"));
        assert!(!response.contains("wallpaperPropertyListener"));
        assert!(response.ends_with("\r\n\r\n"));
    }

    #[test]
    fn runtime_server_rejects_unsupported_methods() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("index.html"), "<html><body>ok</body></html>").expect("index");

        let runtime_url = prepare_web_runtime_session(
            root.join("index.html").to_str().expect("entry path"),
        )
        .expect("runtime session")
        .runtime_url()
        .to_string();
        let server = ensure_web_runtime_server().expect("runtime server");
        let index_path = runtime_url
            .strip_prefix(&format!("http://127.0.0.1:{}", server.port))
            .expect("runtime path");

        let response = get_http_response_with_method(server.port, index_path, "POST");
        assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed"));
        assert!(response.ends_with("Method Not Allowed"));
    }

    fn get_http_response(port: u16, path: &str) -> String {
        get_http_response_with_method(port, path, "GET")
    }

    fn get_http_response_with_method(port: u16, path: &str, method: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).expect("request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        response
    }

    fn runtime_token(root: &std::path::Path) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let canonical = root.canonicalize().expect("canonical root");
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}
