use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub source: Option<String>,
}

#[cfg(target_os = "macos")]
fn read_media_metadata_via_osascript() -> Result<Option<MediaMetadata>, String> {
    let script = r#"
set recordSeparator to "||"
set lineSeparator to "%%"

on emitMedia(sourceName, titleText, artistText, albumText)
	return sourceName & recordSeparator & titleText & recordSeparator & artistText & recordSeparator & albumText
end emitMedia

try
	tell application "Music"
		if it is running and player state is playing then
			return emitMedia("Music", name of current track, artist of current track, album of current track)
		end if
	end tell
end try

try
	tell application "Spotify"
		if it is running and player state is playing then
			return emitMedia("Spotify", name of current track, artist of current track, album of current track)
		end if
	end tell
end try

return ""
"#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| format!("osascript media query failed to start: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("osascript media query exited with status {}", output.status)
        } else {
            format!(
                "osascript media query exited with status {}: {stderr}",
                output.status
            )
        });
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("osascript media query returned invalid UTF-8: {error}"))?;
    Ok(parse_media_metadata_payload(&text))
}

fn parse_media_metadata_payload(text: &str) -> Option<MediaMetadata> {
    let payload = text.trim();
    if payload.is_empty() {
        return None;
    }
    let mut parts = payload.split("||");
    let source = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let title = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let artist = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let album = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    Some(MediaMetadata {
        title,
        artist,
        album,
        source,
    })
}

pub fn read_media_metadata() -> Result<Option<MediaMetadata>, String> {
    #[cfg(target_os = "macos")]
    {
        return read_media_metadata_via_osascript();
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("system media metadata is only available on macOS".to_string())
    }
}

#[allow(dead_code)]
pub fn get_media_metadata() -> Option<MediaMetadata> {
    read_media_metadata().ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::parse_media_metadata_payload;

    #[test]
    fn parses_media_metadata_payload_with_missing_title() {
        let metadata = parse_media_metadata_payload("Music||||Artist||Album\n")
            .expect("metadata should parse");

        assert_eq!(metadata.source.as_deref(), Some("Music"));
        assert_eq!(metadata.title, None);
        assert_eq!(metadata.artist.as_deref(), Some("Artist"));
        assert_eq!(metadata.album.as_deref(), Some("Album"));
    }

    #[test]
    fn empty_media_metadata_payload_means_no_media() {
        assert!(parse_media_metadata_payload("\n").is_none());
    }
}
