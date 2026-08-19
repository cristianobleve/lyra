use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Response, Server};
use url::Url;

const SPOTIFY_AUTH_URL: &str = "https://accounts.spotify.com/authorize";
const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const DEFAULT_SCOPES: &str = "user-read-private user-read-email user-read-playback-state user-modify-playback-state user-read-currently-playing playlist-read-private playlist-read-collaborative user-library-read user-library-modify streaming user-read-recently-played";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64,
    pub token_type: String,
    pub scope: Option<String>,
}

impl SpotifyTokens {
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Refresh 60 seconds before actual expiration
        now + 60 >= self.expires_at
    }
}

pub struct AuthManager {
    client_id: parking_lot::RwLock<String>,
    http_client: Client,
    tokens: parking_lot::RwLock<Option<SpotifyTokens>>,
}

impl AuthManager {
    pub fn new(client_id: String) -> Self {
        Self {
            client_id: parking_lot::RwLock::new(client_id),
            http_client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            tokens: parking_lot::RwLock::new(None),
        }
    }

    pub fn set_client_id(&self, client_id: String) {
        *self.client_id.write() = client_id;
    }

    pub fn get_client_id(&self) -> String {
        self.client_id.read().clone()
    }

    pub fn set_tokens(&self, tokens: SpotifyTokens) {
        *self.tokens.write() = Some(tokens);
    }

    pub fn get_tokens(&self) -> Option<SpotifyTokens> {
        self.tokens.read().clone()
    }

    pub fn is_authenticated(&self) -> bool {
        self.tokens.read().is_some()
    }

    pub fn generate_pkce_pair() -> (String, String) {
        let mut bytes = [0u8; 64];
        rand::thread_rng().fill_bytes(&mut bytes);
        let verifier = URL_SAFE_NO_PAD.encode(bytes);

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        (verifier, challenge)
    }

    pub async fn start_pkce_web_login(&self, port: u16) -> Result<SpotifyTokens> {
        let client_id = self.get_client_id();
        if client_id.trim().is_empty() {
            return Err(anyhow!("Spotify Client ID is required. Please enter your Client ID."));
        }

        let (code_verifier, code_challenge) = Self::generate_pkce_pair();
        let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

        let mut auth_url = Url::parse(SPOTIFY_AUTH_URL)?;
        auth_url.query_pairs_mut()
            .append_pair("client_id", &client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &code_challenge)
            .append_pair("scope", DEFAULT_SCOPES);

        let server_addr = format!("127.0.0.1:{}", port);
        let server = Server::http(&server_addr)
            .map_err(|e| anyhow!("Failed to bind local HTTP server on {}: {}", server_addr, e))?;

        log::info!("Opening Spotify authorization URL in browser: {}", auth_url);
        open::that(auth_url.as_str()).context("Failed to open default web browser")?;

        // Wait for redirect in blocking task to avoid blocking tokio runtime
        let auth_code = tokio::task::spawn_blocking(move || -> Result<String> {
            let start = Instant::now();
            let timeout = Duration::from_secs(180);

            while start.elapsed() < timeout {
                if let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(500)) {
                    let url_str = format!("http://localhost{}", request.url());
                    if let Ok(req_url) = Url::parse(&url_str) {
                        let query_map: HashMap<_, _> = req_url.query_pairs().into_owned().collect();

                        if let Some(code) = query_map.get("code") {
                            let code = code.clone();
                            let html_success = r#"
                            <!DOCTYPE html>
                            <html>
                            <head>
                                <meta charset="utf-8">
                                <title>Spotialt - Authorized</title>
                                <style>
                                    body {
                                        background: #171717;
                                        color: #ffffff;
                                        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
                                        display: flex;
                                        align-items: center;
                                        justify-content: center;
                                        height: 100vh;
                                        margin: 0;
                                    }
                                    .card {
                                        background: #202020;
                                        border: 1px solid #333333;
                                        border-radius: 8px;
                                        padding: 32px;
                                        max-width: 420px;
                                        text-align: center;
                                        box-shadow: 0 4px 20px rgba(0,0,0,0.5);
                                    }
                                    h1 { font-size: 20px; margin-bottom: 12px; font-weight: 600; }
                                    p { color: #888888; font-size: 14px; line-height: 1.5; margin: 0; }
                                    .accent { color: #0070f3; font-weight: 500; }
                                </style>
                            </head>
                            <body>
                                <div class="card">
                                    <h1>Spotialt Connected</h1>
                                    <p>Spotify authorization was successful.<br>You can now close this tab and return to <span class="accent">Spotialt</span>.</p>
                                </div>
                            </body>
                            </html>
                            "#;
                            let header = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
                            let mut response = Response::from_string(html_success);
                            response.add_header(header);
                            let _ = request.respond(response);
                            return Ok(code);
                        } else if let Some(err) = query_map.get("error") {
                            let html_error = format!(r#"
                            <!DOCTYPE html>
                            <html>
                            <body style="background:#171717;color:#ee0000;font-family:sans-serif;display:flex;justify-content:center;align-items:center;height:100vh;">
                                <div style="background:#202020;padding:32px;border-radius:8px;border:1px solid #441111;">
                                    <h2>Spotify Authorization Error</h2>
                                    <p style="color:#aaaaaa;">Error: {}</p>
                                </div>
                            </body>
                            </html>
                            "#, err);
                            let _ = request.respond(Response::from_string(html_error));
                            return Err(anyhow!("Spotify authorization error: {}", err));
                        }
                    }
                }
            }
            Err(anyhow!("Login timed out after 3 minutes"))
        }).await??;

        // Exchange code for token
        let tokens = self.exchange_code_for_token(&auth_code, &code_verifier, &redirect_uri).await?;
        self.set_tokens(tokens.clone());
        Ok(tokens)
    }

    pub async fn exchange_code_for_token(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<SpotifyTokens> {
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            token_type: String,
            scope: Option<String>,
            expires_in: u64,
            refresh_token: Option<String>,
        }

        let client_id = self.get_client_id();
        let params = [
            ("client_id", client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ];

        let res = self.http_client
            .post(SPOTIFY_TOKEN_URL)
            .form(&params)
            .send()
            .await
            .context("Failed to send token exchange request")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Token exchange failed: {}", error_text));
        }

        let token_data: TokenResponse = res.json().await.context("Failed to parse token response")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(SpotifyTokens {
            access_token: token_data.access_token,
            refresh_token: token_data.refresh_token,
            expires_at: now + token_data.expires_in,
            token_type: token_data.token_type,
            scope: token_data.scope,
        })
    }

    pub async fn refresh_access_token(&self) -> Result<SpotifyTokens> {
        let current_tokens = self.tokens.read().clone().ok_or_else(|| anyhow!("No tokens stored"))?;
        let refresh_token = current_tokens.refresh_token.ok_or_else(|| anyhow!("No refresh token available"))?;

        #[derive(Deserialize)]
        struct RefreshResponse {
            access_token: String,
            token_type: String,
            scope: Option<String>,
            expires_in: u64,
            refresh_token: Option<String>,
        }

        let client_id = self.get_client_id();
        let params = [
            ("client_id", client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
        ];

        let res = self.http_client
            .post(SPOTIFY_TOKEN_URL)
            .form(&params)
            .send()
            .await
            .context("Failed to send token refresh request")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Token refresh failed: {}", error_text));
        }

        let data: RefreshResponse = res.json().await.context("Failed to parse refresh token response")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let new_tokens = SpotifyTokens {
            access_token: data.access_token,
            refresh_token: data.refresh_token.or(Some(refresh_token)),
            expires_at: now + data.expires_in,
            token_type: data.token_type,
            scope: data.scope.or(current_tokens.scope),
        };

        self.set_tokens(new_tokens.clone());
        Ok(new_tokens)
    }

    pub async fn get_valid_access_token(&self) -> Result<String> {
        let tokens = self.tokens.read().clone().ok_or_else(|| anyhow!("Not authenticated"))?;
        if tokens.is_expired() {
            log::info!("Access token expired, refreshing...");
            let refreshed = self.refresh_access_token().await?;
            Ok(refreshed.access_token)
        } else {
            Ok(tokens.access_token)
        }
    }
}
