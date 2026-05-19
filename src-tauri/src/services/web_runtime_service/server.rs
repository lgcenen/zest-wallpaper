use std::{
    net::TcpListener,
    sync::OnceLock,
    thread,
};

use super::{request_handler::handle_web_runtime_connection, root_registry::RuntimeRootRegistry};

pub(super) struct WebRuntimeServer {
    pub(super) port: u16,
    pub(super) roots: RuntimeRootRegistry,
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
        let roots = RuntimeRootRegistry::new();
        let thread_roots = roots.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let roots = thread_roots.clone();
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

pub(super) fn register_runtime_root(
    server: &WebRuntimeServer,
    token: String,
    root: std::path::PathBuf,
) -> Result<(), String> {
    server.roots.register(token, root)
}
