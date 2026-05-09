use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

pub(super) struct RuntimeRequest {
    pub(super) token: String,
    pub(super) relative_path: PathBuf,
}

pub(super) enum AssetResolutionError {
    Forbidden,
    NotFound,
}

pub(super) fn guess_content_type(path: &Path) -> &'static str {
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

pub(super) fn url_encode_component(input: &str) -> String {
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

pub(super) fn runtime_token(root: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn parse_runtime_request_target(target: &str) -> Option<RuntimeRequest> {
    let target = target.split('?').next().unwrap_or("/");
    let path = target.strip_prefix("/web-runtime/")?;

    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let token = segments.next()?.to_string();
    let relative_segments = segments.map(percent_decode).collect::<Vec<_>>();
    let relative_path = if relative_segments.is_empty() {
        PathBuf::from("index.html")
    } else {
        relative_segments
            .iter()
            .fold(PathBuf::new(), |mut path, segment| {
                path.push(segment);
                path
            })
    };

    Some(RuntimeRequest {
        token,
        relative_path,
    })
}

pub(super) fn resolve_runtime_asset(
    root: &Path,
    relative_path: &Path,
) -> Result<PathBuf, AssetResolutionError> {
    let candidate = root.join(relative_path);
    let canonical = candidate
        .canonicalize()
        .map_err(|_| AssetResolutionError::NotFound)?;
    if !canonical.starts_with(root) {
        return Err(AssetResolutionError::Forbidden);
    }
    Ok(canonical)
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
