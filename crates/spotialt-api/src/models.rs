use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageObject {
    pub url: String,
    pub height: Option<u32>,
    pub width: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserProfile {
    pub id: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub product: Option<String>,
    #[serde(default)]
    pub images: Vec<ImageObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimplifiedArtist {
    pub id: String,
    pub name: String,
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimplifiedAlbum {
    pub id: String,
    pub name: String,
    pub uri: Option<String>,
    #[serde(default)]
    pub images: Vec<ImageObject>,
    pub release_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrackItem {
    pub id: Option<String>,
    pub name: String,
    pub uri: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub artists: Vec<SimplifiedArtist>,
    pub album: Option<SimplifiedAlbum>,
    pub track_number: Option<u32>,
    pub explicit: Option<bool>,
}

impl TrackItem {
    pub fn artist_names(&self) -> String {
        self.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn album_name(&self) -> String {
        self.album.as_ref().map(|a| a.name.clone()).unwrap_or_default()
    }

    pub fn cover_url(&self) -> Option<String> {
        self.album.as_ref().and_then(|a| a.images.first().map(|i| i.url.clone()))
    }

    pub fn formatted_duration(&self) -> String {
        let total_secs = self.duration_ms / 1000;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{:02}:{:02}", mins, secs)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaylistTrackContainer {
    pub track: Option<TrackItem>,
    pub added_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SavedTrackContainer {
    pub track: TrackItem,
    pub added_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimplifiedPlaylist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub uri: String,
    #[serde(default)]
    pub images: Vec<ImageObject>,
    pub tracks: PlaylistTracksRef,
    pub owner: Option<PlaylistOwner>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaylistOwner {
    pub id: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaylistTracksRef {
    pub total: u32,
    pub href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PagingObject<T> {
    pub items: Vec<T>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchResults {
    pub tracks: Option<PagingObject<TrackItem>>,
    pub playlists: Option<PagingObject<SimplifiedPlaylist>>,
    pub albums: Option<PagingObject<SimplifiedAlbum>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Device {
    pub id: Option<String>,
    pub is_active: bool,
    pub is_private_session: bool,
    pub is_restricted: bool,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: String,
    pub volume_percent: Option<u32>,
}

impl Device {
    pub fn is_speaker(&self) -> bool {
        self.device_type.eq_ignore_ascii_case("Speaker")
            || self.device_type.eq_ignore_ascii_case("CastAudio")
            || self.name.to_lowercase().contains("echo")
            || self.name.to_lowercase().contains("alexa")
            || self.name.to_lowercase().contains("speaker")
    }

    pub fn is_smartphone(&self) -> bool {
        self.device_type.eq_ignore_ascii_case("Smartphone")
            || self.name.to_lowercase().contains("phone")
            || self.name.to_lowercase().contains("iphone")
            || self.name.to_lowercase().contains("android")
    }

    pub fn is_tv(&self) -> bool {
        self.device_type.eq_ignore_ascii_case("TV")
            || self.device_type.eq_ignore_ascii_case("CastVideo")
            || self.name.to_lowercase().contains("tv")
    }

    pub fn normalized_type(&self) -> &'static str {
        if self.is_speaker() {
            "Speaker"
        } else if self.is_smartphone() {
            "Smartphone"
        } else if self.is_tv() {
            "TV"
        } else {
            "Computer"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DevicesResponse {
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueueResponse {
    pub currently_playing: Option<TrackItem>,
    #[serde(default)]
    pub queue: Vec<TrackItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaybackState {
    pub device: Option<Device>,
    pub repeat_state: Option<String>,
    pub shuffle_state: Option<bool>,
    pub is_playing: bool,
    pub progress_ms: Option<u64>,
    pub item: Option<TrackItem>,
}

