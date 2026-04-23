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
fn read_media_metadata_via_osascript() -> Option<MediaMetadata> {
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
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let payload = text.trim();
    if payload.is_empty() {
        return None;
    }
    let mut parts = payload.split("||");
    let source = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
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
        source: Some(source.to_string()),
    })
}

pub fn get_media_metadata() -> Option<MediaMetadata> {
    #[cfg(target_os = "macos")]
    {
        return read_media_metadata_via_osascript();
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
