use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    pub start_time_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncedLyrics {
    pub lines: Vec<LyricLine>,
    pub is_synced: bool,
    pub plain_lyrics: Option<String>,
}

impl SyncedLyrics {
    pub fn current_line_index(&self, current_time_ms: u64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let mut selected = None;
        for (i, line) in self.lines.iter().enumerate() {
            if line.start_time_ms <= current_time_ms {
                selected = Some(i);
            } else {
                break;
            }
        }
        selected
    }

    pub fn parse_lrc(lrc_text: &str) -> Self {
        let mut lines = Vec::new();
        for raw_line in lrc_text.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Look for [mm:ss.xx] or [mm:ss]
            if let Some(close_bracket) = trimmed.find(']') {
                if trimmed.starts_with('[') {
                    let time_part = &trimmed[1..close_bracket];
                    let text_part = trimmed[close_bracket + 1..].trim().to_string();

                    if let Some(ms) = parse_time_tag(time_part) {
                        lines.push(LyricLine {
                            start_time_ms: ms,
                            text: text_part,
                        });
                    }
                }
            }
        }

        lines.sort_by_key(|l| l.start_time_ms);
        let is_synced = !lines.is_empty();

        Self {
            lines,
            is_synced,
            plain_lyrics: None,
        }
    }
}

fn parse_time_tag(tag: &str) -> Option<u64> {
    // tag can be "01:23.45" or "01:23" or "01:23:45"
    let parts: Vec<&str> = tag.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let mins: u64 = parts[0].parse().ok()?;
    let secs_part = parts[1];

    if let Some(dot_pos) = secs_part.find('.') {
        let secs: u64 = secs_part[..dot_pos].parse().ok()?;
        let fraction_str = &secs_part[dot_pos + 1..];
        let fraction_ms: u64 = match fraction_str.len() {
            1 => fraction_str.parse::<u64>().ok()? * 100,
            2 => fraction_str.parse::<u64>().ok()? * 10,
            _ => fraction_str[..3.min(fraction_str.len())].parse::<u64>().ok()?,
        };
        Some((mins * 60 + secs) * 1000 + fraction_ms)
    } else {
        let secs: u64 = secs_part.parse().ok()?;
        Some((mins * 60 + secs) * 1000)
    }
}

pub struct LrcLibClient {
    client: Client,
}

impl LrcLibClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(6))
                .user_agent("Spotialt/0.1 (https://github.com/spotialt)")
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn fetch_lyrics(
        &self,
        track_name: &str,
        artist_name: &str,
        album_name: Option<&str>,
        duration_secs: Option<u64>,
    ) -> Result<SyncedLyrics> {
        #[derive(Deserialize)]
        struct LrcResponse {
            synced_lyrics: Option<String>,
            plain_lyrics: Option<String>,
        }

        let mut req = self.client.get("https://lrclib.net/api/get")
            .query(&[("track_name", track_name), ("artist_name", artist_name)]);

        if let Some(album) = album_name {
            req = req.query(&[("album_name", album)]);
        }
        if let Some(dur) = duration_secs {
            req = req.query(&[("duration", dur.to_string().as_str())]);
        }

        let res = req.send().await.context("Failed to contact LRCLIB")?;
        if res.status().is_success() {
            let data: LrcResponse = res.json().await.context("Failed to parse LRCLIB response")?;
            if let Some(synced) = data.synced_lyrics {
                let mut lyrics = SyncedLyrics::parse_lrc(&synced);
                lyrics.plain_lyrics = data.plain_lyrics;
                return Ok(lyrics);
            } else if let Some(plain) = data.plain_lyrics {
                return Ok(SyncedLyrics {
                    lines: Vec::new(),
                    is_synced: false,
                    plain_lyrics: Some(plain),
                });
            }
        }

        Ok(SyncedLyrics::default())
    }
}
