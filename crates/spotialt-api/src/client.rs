use crate::models::*;
use anyhow::{anyhow, Context, Result};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::json;
use spotialt_core::AuthManager;
use std::sync::Arc;
use std::time::Duration;

const SPOTIFY_API_BASE: &str = "https://api.spotify.com/v1";

#[derive(Clone)]
pub struct SpotifyApiClient {
    auth_manager: Arc<AuthManager>,
    http_client: Client,
}

impl SpotifyApiClient {
    pub fn new(auth_manager: Arc<AuthManager>) -> Self {
        Self {
            auth_manager,
            http_client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    async fn send_request<T: DeserializeOwned>(&self, method: Method, endpoint: &str, body: Option<serde_json::Value>) -> Result<T> {
        let token = self.auth_manager.get_valid_access_token().await?;
        let url = format!("{}{}", SPOTIFY_API_BASE, endpoint);

        let mut req = self.http_client.request(method.clone(), &url)
            .bearer_auth(&token);

        if let Some(b) = body.as_ref() {
            req = req.json(b);
        }

        let res = req.send().await.context(format!("Failed request to {}", endpoint))?;

        if res.status() == StatusCode::UNAUTHORIZED {
            log::info!("Received 401 Unauthorized, refreshing token and retrying...");
            let new_token = self.auth_manager.refresh_access_token().await?.access_token;
            let mut retry_req = self.http_client.request(method, &url)
                .bearer_auth(&new_token);
            if let Some(b) = body {
                retry_req = retry_req.json(&b);
            }
            let retry_res = retry_req.send().await.context(format!("Failed retry request to {}", endpoint))?;
            if !retry_res.status().is_success() {
                let err_text = retry_res.text().await.unwrap_or_default();
                return Err(anyhow!("API Error {}: {}", endpoint, err_text));
            }
            return retry_res.json::<T>().await.context("Failed to parse JSON response");
        }

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("API Error {}: {}", endpoint, err_text));
        }

        res.json::<T>().await.context(format!("Failed to deserialize response from {}", endpoint))
    }

    async fn send_request_empty(&self, method: Method, endpoint: &str, body: Option<serde_json::Value>) -> Result<()> {
        let token = self.auth_manager.get_valid_access_token().await?;
        let url = format!("{}{}", SPOTIFY_API_BASE, endpoint);

        let mut req = self.http_client.request(method.clone(), &url)
            .bearer_auth(&token);

        if let Some(b) = body.as_ref() {
            req = req.json(b);
        }

        let res = req.send().await.context(format!("Failed request to {}", endpoint))?;

        if res.status() == StatusCode::UNAUTHORIZED {
            let new_token = self.auth_manager.refresh_access_token().await?.access_token;
            let mut retry_req = self.http_client.request(method, &url)
                .bearer_auth(&new_token);
            if let Some(b) = body {
                retry_req = retry_req.json(&b);
            }
            let retry_res = retry_req.send().await.context(format!("Failed retry request to {}", endpoint))?;
            if !retry_res.status().is_success() && retry_res.status() != StatusCode::NO_CONTENT {
                let err_text = retry_res.text().await.unwrap_or_default();
                return Err(anyhow!("API Error {}: {}", endpoint, err_text));
            }
            return Ok(());
        }

        if !res.status().is_success() && res.status() != StatusCode::NO_CONTENT {
            let err_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("API Error {}: {}", endpoint, err_text));
        }

        Ok(())
    }

    pub async fn get_current_user(&self) -> Result<UserProfile> {
        self.send_request(Method::GET, "/me", None).await
    }

    pub async fn get_user_playlists(&self, limit: u32, offset: u32) -> Result<PagingObject<SimplifiedPlaylist>> {
        let endpoint = format!("/me/playlists?limit={}&offset={}", limit, offset);
        self.send_request(Method::GET, &endpoint, None).await
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str, limit: u32, offset: u32) -> Result<PagingObject<PlaylistTrackContainer>> {
        let endpoint = format!("/playlists/{}/tracks?limit={}&offset={}", playlist_id, limit, offset);
        self.send_request(Method::GET, &endpoint, None).await
    }

    pub async fn get_saved_tracks(&self, limit: u32, offset: u32) -> Result<PagingObject<SavedTrackContainer>> {
        let endpoint = format!("/me/tracks?limit={}&offset={}", limit, offset);
        self.send_request(Method::GET, &endpoint, None).await
    }

    pub async fn save_track(&self, track_id: &str) -> Result<()> {
        let endpoint = format!("/me/tracks?ids={}", track_id);
        self.send_request_empty(Method::PUT, &endpoint, None).await
    }

    pub async fn remove_saved_track(&self, track_id: &str) -> Result<()> {
        let endpoint = format!("/me/tracks?ids={}", track_id);
        self.send_request_empty(Method::DELETE, &endpoint, None).await
    }

    pub async fn check_saved_tracks(&self, track_ids: &[&str]) -> Result<Vec<bool>> {
        if track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids_str = track_ids.join(",");
        let endpoint = format!("/me/tracks/contains?ids={}", ids_str);
        self.send_request(Method::GET, &endpoint, None).await
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<SearchResults> {
        let encoded_q = urlencoding::encode(query);
        let endpoint = format!("/search?q={}&type=track,album,playlist&limit={}", encoded_q, limit);
        self.send_request(Method::GET, &endpoint, None).await
    }

    pub async fn get_playback_state(&self) -> Result<Option<PlaybackState>> {
        let token = self.auth_manager.get_valid_access_token().await?;
        let url = format!("{}/me/player", SPOTIFY_API_BASE);

        let res = self.http_client.get(&url)
            .bearer_auth(&token)
            .send()
            .await?;

        if res.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }

        if !res.status().is_success() {
            return Ok(None);
        }

        let state = res.json::<PlaybackState>().await.ok();
        Ok(state)
    }

    pub async fn get_available_devices(&self) -> Result<Vec<Device>> {
        let res: DevicesResponse = self.send_request(Method::GET, "/me/player/devices", None).await?;
        Ok(res.devices)
    }

    pub async fn transfer_playback(&self, device_id: &str, play: bool) -> Result<()> {
        let body = json!({
            "device_ids": [device_id],
            "play": play
        });
        self.send_request_empty(Method::PUT, "/me/player", Some(body)).await
    }

    pub async fn play(&self, device_id: Option<&str>, context_uri: Option<&str>, uris: Option<Vec<String>>, offset_position: Option<u32>) -> Result<()> {
        let mut endpoint = "/me/player/play".to_string();
        if let Some(id) = device_id {
            endpoint = format!("{}?device_id={}", endpoint, id);
        }

        let mut body_map = serde_json::Map::new();
        if let Some(ctx) = context_uri {
            body_map.insert("context_uri".to_string(), json!(ctx));
            if let Some(offset) = offset_position {
                body_map.insert("offset".to_string(), json!({ "position": offset }));
            }
        } else if let Some(u) = uris {
            body_map.insert("uris".to_string(), json!(u));
            if let Some(offset) = offset_position {
                body_map.insert("offset".to_string(), json!({ "position": offset }));
            }
        }

        let body = if body_map.is_empty() { None } else { Some(serde_json::Value::Object(body_map)) };
        self.send_request_empty(Method::PUT, &endpoint, body).await
    }

    pub async fn pause(&self, device_id: Option<&str>) -> Result<()> {
        let endpoint = match device_id {
            Some(id) => format!("/me/player/pause?device_id={}", id),
            None => "/me/player/pause".to_string(),
        };
        self.send_request_empty(Method::PUT, &endpoint, None).await
    }

    pub async fn next(&self, device_id: Option<&str>) -> Result<()> {
        let endpoint = match device_id {
            Some(id) => format!("/me/player/next?device_id={}", id),
            None => "/me/player/next".to_string(),
        };
        self.send_request_empty(Method::POST, &endpoint, None).await
    }

    pub async fn previous(&self, device_id: Option<&str>) -> Result<()> {
        let endpoint = match device_id {
            Some(id) => format!("/me/player/previous?device_id={}", id),
            None => "/me/player/previous".to_string(),
        };
        self.send_request_empty(Method::POST, &endpoint, None).await
    }

    pub async fn seek(&self, position_ms: u64, device_id: Option<&str>) -> Result<()> {
        let endpoint = match device_id {
            Some(id) => format!("/me/player/seek?position_ms={}&device_id={}", position_ms, id),
            None => format!("/me/player/seek?position_ms={}", position_ms),
        };
        self.send_request_empty(Method::PUT, &endpoint, None).await
    }

    pub async fn set_volume(&self, volume_percent: u32, device_id: Option<&str>) -> Result<()> {
        let endpoint = match device_id {
            Some(id) => format!("/me/player/volume?volume_percent={}&device_id={}", volume_percent, id),
            None => format!("/me/player/volume?volume_percent={}", volume_percent),
        };
        self.send_request_empty(Method::PUT, &endpoint, None).await
    }

    pub async fn set_shuffle(&self, state: bool, device_id: Option<&str>) -> Result<()> {
        let endpoint = match device_id {
            Some(id) => format!("/me/player/shuffle?state={}&device_id={}", state, id),
            None => format!("/me/player/shuffle?state={}", state),
        };
        self.send_request_empty(Method::PUT, &endpoint, None).await
    }

    pub async fn set_repeat(&self, state: &str, device_id: Option<&str>) -> Result<()> {
        let endpoint = match device_id {
            Some(id) => format!("/me/player/repeat?state={}&device_id={}", state, id),
            None => format!("/me/player/repeat?state={}", state),
        };
        self.send_request_empty(Method::PUT, &endpoint, None).await
    }

    pub async fn get_queue(&self) -> Result<QueueResponse> {
        self.send_request(Method::GET, "/me/player/queue", None).await
    }

    pub async fn add_to_queue(&self, uri: &str, device_id: Option<&str>) -> Result<()> {
        let encoded_uri = urlencoding::encode(uri);
        let endpoint = match device_id {
            Some(id) => format!("/me/player/queue?uri={}&device_id={}", encoded_uri, id),
            None => format!("/me/player/queue?uri={}", encoded_uri),
        };
        self.send_request_empty(Method::POST, &endpoint, None).await
    }
}

