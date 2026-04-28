use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};

use crate::models::{
    SceneNowPlayingAvailability, SceneNowPlayingDiagnostic, SceneNowPlayingDiagnosticSeverity,
    SceneNowPlayingSnapshot, SceneNowPlayingState,
};

use super::system_service::{self, MediaMetadata};

pub const DEFAULT_NOW_PLAYING_REFRESH_INTERVAL_MILLIS: u64 = 1_500;

fn shared_now_playing_provider() -> &'static SceneNowPlayingProvider {
    static PROVIDER: OnceLock<SceneNowPlayingProvider> = OnceLock::new();
    PROVIDER.get_or_init(SceneNowPlayingProvider::default)
}

pub fn now_playing_snapshot() -> SceneNowPlayingSnapshot {
    shared_now_playing_provider().snapshot()
}

pub fn provider_refresh_interval_millis(snapshot: &SceneNowPlayingSnapshot) -> u64 {
    snapshot
        .refresh_interval_millis
        .max(DEFAULT_NOW_PLAYING_REFRESH_INTERVAL_MILLIS)
}

pub fn uses_now_playing_provider(manifest: &crate::models::SceneManifest) -> bool {
    manifest
        .text_layers
        .iter()
        .any(|layer| layer.behavior == crate::models::SceneTextBehavior::MediaTitle)
}

struct SceneNowPlayingProvider {
    cache: Mutex<SceneNowPlayingProviderCache>,
    poll_interval: Duration,
}

struct SceneNowPlayingProviderCache {
    snapshot: SceneNowPlayingSnapshot,
    last_poll_at: Option<Instant>,
}

impl Default for SceneNowPlayingProvider {
    fn default() -> Self {
        Self::new(Duration::from_millis(
            DEFAULT_NOW_PLAYING_REFRESH_INTERVAL_MILLIS,
        ))
    }
}

impl SceneNowPlayingProvider {
    fn new(poll_interval: Duration) -> Self {
        Self {
            cache: Mutex::new(SceneNowPlayingProviderCache {
                snapshot: SceneNowPlayingSnapshot {
                    refresh_interval_millis: poll_interval.as_millis().min(u64::MAX as u128) as u64,
                    ..SceneNowPlayingSnapshot::default()
                },
                last_poll_at: None,
            }),
            poll_interval,
        }
    }

    fn snapshot(&self) -> SceneNowPlayingSnapshot {
        self.snapshot_with_reader(
            Instant::now(),
            Utc::now(),
            system_service::read_media_metadata,
        )
    }

    fn snapshot_with_reader<ReadMedia>(
        &self,
        monotonic_now: Instant,
        updated_at: DateTime<Utc>,
        read_media: ReadMedia,
    ) -> SceneNowPlayingSnapshot
    where
        ReadMedia: FnOnce() -> Result<Option<MediaMetadata>, String>,
    {
        let Ok(mut cache) = self.cache.lock() else {
            return snapshot_from_media_result(
                &SceneNowPlayingSnapshot::default(),
                Err("now-playing provider cache lock failed".to_string()),
                updated_at,
                self.refresh_interval_millis(),
            );
        };

        if let Some(last_poll_at) = cache.last_poll_at {
            if monotonic_now.duration_since(last_poll_at) < self.poll_interval {
                return cache.snapshot.clone();
            }
        }

        let next = snapshot_from_media_result(
            &cache.snapshot,
            read_media(),
            updated_at,
            self.refresh_interval_millis(),
        );
        cache.snapshot = next.clone();
        cache.last_poll_at = Some(monotonic_now);
        next
    }

    fn refresh_interval_millis(&self) -> u64 {
        self.poll_interval.as_millis().min(u64::MAX as u128) as u64
    }
}

fn snapshot_from_media_result(
    previous: &SceneNowPlayingSnapshot,
    result: Result<Option<MediaMetadata>, String>,
    updated_at: DateTime<Utc>,
    refresh_interval_millis: u64,
) -> SceneNowPlayingSnapshot {
    let mut next = match result {
        Ok(Some(metadata)) => snapshot_from_metadata(metadata, updated_at, refresh_interval_millis),
        Ok(None) => idle_snapshot(updated_at, refresh_interval_millis),
        Err(error) => unavailable_snapshot(error, updated_at, refresh_interval_millis),
    };
    next.generation = if same_now_playing_generation_inputs(previous, &next) {
        previous.generation
    } else {
        previous.generation.saturating_add(1)
    };
    next
}

fn snapshot_from_metadata(
    metadata: MediaMetadata,
    updated_at: DateTime<Utc>,
    refresh_interval_millis: u64,
) -> SceneNowPlayingSnapshot {
    let title = normalize_optional_text(metadata.title);
    let artist = normalize_optional_text(metadata.artist);
    let album = normalize_optional_text(metadata.album);
    let source = normalize_optional_text(metadata.source);
    let state = if title.is_some() {
        SceneNowPlayingState::Ready
    } else {
        SceneNowPlayingState::PlayingWithoutTitle
    };
    let diagnostics = if title.is_some() {
        Vec::new()
    } else {
        vec![SceneNowPlayingDiagnostic {
            severity: SceneNowPlayingDiagnosticSeverity::Warning,
            code: "title-missing".to_string(),
            message: "Now Playing metadata was available but did not include a title.".to_string(),
            detail: source.as_deref().map(|source| format!("source: {source}")),
        }]
    };

    SceneNowPlayingSnapshot {
        availability: SceneNowPlayingAvailability::Available,
        state,
        title,
        artist,
        album,
        source,
        generation: 0,
        updated_at,
        refresh_interval_millis,
        diagnostics,
    }
}

fn idle_snapshot(
    updated_at: DateTime<Utc>,
    refresh_interval_millis: u64,
) -> SceneNowPlayingSnapshot {
    SceneNowPlayingSnapshot {
        availability: SceneNowPlayingAvailability::Available,
        state: SceneNowPlayingState::Idle,
        title: None,
        artist: None,
        album: None,
        source: None,
        generation: 0,
        updated_at,
        refresh_interval_millis,
        diagnostics: vec![SceneNowPlayingDiagnostic {
            severity: SceneNowPlayingDiagnosticSeverity::Info,
            code: "no-media".to_string(),
            message: "No playing media was reported by the Now Playing provider.".to_string(),
            detail: None,
        }],
    }
}

fn unavailable_snapshot(
    error: String,
    updated_at: DateTime<Utc>,
    refresh_interval_millis: u64,
) -> SceneNowPlayingSnapshot {
    SceneNowPlayingSnapshot {
        availability: SceneNowPlayingAvailability::Unavailable,
        state: SceneNowPlayingState::Unavailable,
        title: None,
        artist: None,
        album: None,
        source: None,
        generation: 0,
        updated_at,
        refresh_interval_millis,
        diagnostics: vec![SceneNowPlayingDiagnostic {
            severity: SceneNowPlayingDiagnosticSeverity::Warning,
            code: "read-failed".to_string(),
            message: "Now Playing provider could not read system media metadata.".to_string(),
            detail: Some(error),
        }],
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn same_now_playing_generation_inputs(
    previous: &SceneNowPlayingSnapshot,
    next: &SceneNowPlayingSnapshot,
) -> bool {
    previous.availability == next.availability
        && previous.state == next.state
        && previous.title == next.title
        && previous.artist == next.artist
        && previous.album == next.album
        && previous.source == next.source
        && previous.diagnostics == next.diagnostics
}

#[cfg(test)]
mod tests {
    use super::{
        snapshot_from_media_result, MediaMetadata, SceneNowPlayingProvider, SceneNowPlayingState,
    };
    use crate::models::{
        SceneNowPlayingAvailability, SceneNowPlayingDiagnosticSeverity, SceneNowPlayingSnapshot,
    };
    use chrono::{TimeZone, Utc};
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };

    fn timestamp(seconds: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    fn metadata(title: Option<&str>) -> MediaMetadata {
        MediaMetadata {
            title: title.map(ToString::to_string),
            artist: Some("Artist".to_string()),
            album: Some("Album".to_string()),
            source: Some("Music".to_string()),
        }
    }

    #[test]
    fn provider_normalizes_ready_idle_missing_title_and_read_failed_states() {
        let initial = SceneNowPlayingSnapshot::default();
        let ready = snapshot_from_media_result(
            &initial,
            Ok(Some(metadata(Some(" Song ")))),
            timestamp(1),
            1500,
        );
        assert_eq!(ready.availability, SceneNowPlayingAvailability::Available);
        assert_eq!(ready.state, SceneNowPlayingState::Ready);
        assert_eq!(ready.title.as_deref(), Some("Song"));
        assert_eq!(ready.generation, 1);

        let missing_title =
            snapshot_from_media_result(&ready, Ok(Some(metadata(None))), timestamp(2), 1500);
        assert_eq!(
            missing_title.state,
            SceneNowPlayingState::PlayingWithoutTitle
        );
        assert_eq!(missing_title.diagnostics[0].code, "title-missing");
        assert_eq!(
            missing_title.diagnostics[0].severity,
            SceneNowPlayingDiagnosticSeverity::Warning
        );

        let idle = snapshot_from_media_result(&missing_title, Ok(None), timestamp(3), 1500);
        assert_eq!(idle.state, SceneNowPlayingState::Idle);
        assert_eq!(idle.diagnostics[0].code, "no-media");
        assert_eq!(
            idle.diagnostics[0].severity,
            SceneNowPlayingDiagnosticSeverity::Info
        );

        let failed = snapshot_from_media_result(
            &idle,
            Err("permission denied".to_string()),
            timestamp(4),
            1500,
        );
        assert_eq!(
            failed.availability,
            SceneNowPlayingAvailability::Unavailable
        );
        assert_eq!(failed.state, SceneNowPlayingState::Unavailable);
        assert_eq!(failed.diagnostics[0].code, "read-failed");
    }

    #[test]
    fn provider_generation_changes_only_when_normalized_output_changes() {
        let initial = SceneNowPlayingSnapshot::default();
        let first = snapshot_from_media_result(
            &initial,
            Ok(Some(metadata(Some("Song")))),
            timestamp(1),
            1500,
        );
        let second = snapshot_from_media_result(
            &first,
            Ok(Some(metadata(Some("Song")))),
            timestamp(2),
            1500,
        );
        let third = snapshot_from_media_result(
            &second,
            Ok(Some(metadata(Some("Other")))),
            timestamp(3),
            1500,
        );

        assert_eq!(first.generation, 1);
        assert_eq!(second.generation, 1);
        assert_eq!(second.updated_at, timestamp(2));
        assert_eq!(third.generation, 2);
    }

    #[test]
    fn provider_cache_throttles_system_reads_until_poll_interval_elapses() {
        let provider = SceneNowPlayingProvider::new(Duration::from_secs(10));
        let start = Instant::now();
        let reads = Arc::new(AtomicUsize::new(0));
        let first_reads = Arc::clone(&reads);
        let first = provider.snapshot_with_reader(start, timestamp(1), move || {
            first_reads.fetch_add(1, Ordering::SeqCst);
            Ok(Some(metadata(Some("One"))))
        });
        let second_reads = Arc::clone(&reads);
        let second = provider.snapshot_with_reader(
            start + Duration::from_secs(2),
            timestamp(2),
            move || {
                second_reads.fetch_add(1, Ordering::SeqCst);
                Ok(Some(metadata(Some("Two"))))
            },
        );
        let third_reads = Arc::clone(&reads);
        let third = provider.snapshot_with_reader(
            start + Duration::from_secs(11),
            timestamp(3),
            move || {
                third_reads.fetch_add(1, Ordering::SeqCst);
                Ok(Some(metadata(Some("Two"))))
            },
        );

        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert_eq!(first.title.as_deref(), Some("One"));
        assert_eq!(second.title.as_deref(), Some("One"));
        assert_eq!(third.title.as_deref(), Some("Two"));
        assert_eq!(first.generation, 1);
        assert_eq!(second.generation, 1);
        assert_eq!(third.generation, 2);
    }
}
