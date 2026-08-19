use serde::{Deserialize, Serialize};

pub const DEFAULT_CLIENT_ID: &str = "";
pub const DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub client_id: String,
    pub redirect_uri: String,
    pub audio_quality: AudioQuality,
    pub volume: f32,
    pub bit_perfect_wasapi: bool,
    pub max_image_cache_mb: usize,
    pub dark_theme: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioQuality {
    Low96,
    Normal160,
    High320,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            client_id: DEFAULT_CLIENT_ID.to_string(),
            redirect_uri: DEFAULT_REDIRECT_URI.to_string(),
            audio_quality: AudioQuality::High320,
            volume: 0.85,
            bit_perfect_wasapi: true,
            max_image_cache_mb: 20,
            dark_theme: true,
        }
    }
}
