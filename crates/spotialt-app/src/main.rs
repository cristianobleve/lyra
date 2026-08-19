use anyhow::Result;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use spotialt_api::{Device, LrcLibClient, SpotifyApiClient, SyncedLyrics, TrackItem};
use spotialt_core::{AuthManager, CredentialsStorage};
use spotialt_ui::image_cache::{ImageCache, RgbaRawImage};
use spotialt_ui::{AppWindow, LyricItem, PlaylistItem, UiDevice, UiTrack};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

#[derive(Clone, Debug, Default)]
struct CurrentDevice {
    id: String,
    name: String,
    device_type: String,
    is_remote: bool,
}

fn get_process_memory_mb() -> f32 {
    #[cfg(windows)]
    unsafe {
        use std::mem::zeroed;
        use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let mut pmc: PROCESS_MEMORY_COUNTERS = zeroed();
        let handle = GetCurrentProcess();
        if GetProcessMemoryInfo(
            handle,
            &mut pmc,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ) != 0
        {
            return (pmc.WorkingSetSize as f32) / (1024.0 * 1024.0);
        }
    }
    20.0
}

fn device_to_ui(d: &Device) -> UiDevice {
    UiDevice {
        id: SharedString::from(d.id.clone().unwrap_or_default()),
        name: SharedString::from(d.name.clone()),
        device_type: SharedString::from(d.normalized_type()),
        is_active: d.is_active,
        volume_percent: d.volume_percent.unwrap_or(0) as i32,
    }
}

fn find_pc_device(devices: &[Device]) -> Option<CurrentDevice> {
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "CRISTIANO".to_string()).to_lowercase();

    // 1. Check for device with name containing PC hostname (e.g. CRISTIANO)
    for d in devices {
        if d.name.to_lowercase().contains(&hostname) {
            if let Some(ref id) = d.id {
                return Some(CurrentDevice {
                    id: id.clone(),
                    name: d.name.clone(),
                    device_type: d.normalized_type().to_string(),
                    is_remote: false,
                });
            }
        }
    }

    // 2. Check for device with type "Computer"
    for d in devices {
        if d.device_type.eq_ignore_ascii_case("Computer") {
            if let Some(ref id) = d.id {
                return Some(CurrentDevice {
                    id: id.clone(),
                    name: d.name.clone(),
                    device_type: d.normalized_type().to_string(),
                    is_remote: false,
                });
            }
        }
    }

    // 3. Active non-speaker device
    for d in devices {
        if d.is_active && !d.is_speaker() {
            if let Some(ref id) = d.id {
                return Some(CurrentDevice {
                    id: id.clone(),
                    name: d.name.clone(),
                    device_type: d.normalized_type().to_string(),
                    is_remote: !d.name.to_lowercase().contains(&hostname),
                });
            }
        }
    }

    // 4. Any active device
    for d in devices {
        if d.is_active {
            if let Some(ref id) = d.id {
                return Some(CurrentDevice {
                    id: id.clone(),
                    name: d.name.clone(),
                    device_type: d.normalized_type().to_string(),
                    is_remote: !d.name.to_lowercase().contains(&hostname),
                });
            }
        }
    }

    devices.first().and_then(|d| {
        d.id.as_ref().map(|id| CurrentDevice {
            id: id.clone(),
            name: d.name.clone(),
            device_type: d.normalized_type().to_string(),
            is_remote: !d.name.to_lowercase().contains(&hostname),
        })
    })
}

fn track_to_ui(t: TrackItem, is_liked: bool) -> UiTrack {
    let artist = t.artist_names();
    let album = t.album_name();
    let duration = t.formatted_duration();
    let cover_url = t.cover_url().unwrap_or_default();
    UiTrack {
        id: SharedString::from(t.id.unwrap_or_default()),
        uri: SharedString::from(t.uri),
        title: SharedString::from(t.name),
        artist: SharedString::from(artist),
        album: SharedString::from(album),
        duration: SharedString::from(duration),
        duration_ms: t.duration_ms as i32,
        cover_url: SharedString::from(cover_url),
        is_playing: false,
        is_liked,
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Starting Lyra (Ultra-low RAM Spotify Client)...");

    let tokio_rt = Arc::new(Runtime::new()?);

    let storage = Arc::new(CredentialsStorage::new());
    let config = storage.load_config();
    let auth_manager = Arc::new(AuthManager::new(config.client_id.clone()));

    // Check for existing saved credentials
    let has_saved_tokens = if let Some(tokens) = storage.load_tokens() {
        log::info!("Loaded saved Spotify session from disk");
        auth_manager.set_tokens(tokens);
        true
    } else {
        false
    };

    let api_client = Arc::new(SpotifyApiClient::new(auth_manager.clone()));
    let lyrics_client = Arc::new(LrcLibClient::new());
    let image_cache = Arc::new(ImageCache::new(config.max_image_cache_mb * 2));

    // In-memory track cache for instant (<1ms) tab switching
    let track_cache = Arc::new(parking_lot::RwLock::new(HashMap::<String, Vec<UiTrack>>::new()));

    // Playback state stores
    let target_device = Arc::new(parking_lot::RwLock::new(None::<CurrentDevice>));
    let current_track_id = Arc::new(parking_lot::RwLock::new(String::new()));
    let is_shuffle_active = Arc::new(AtomicBool::new(false));
    let repeat_mode = Arc::new(parking_lot::RwLock::new("off".to_string()));
    let is_current_liked = Arc::new(AtomicBool::new(false));

    let main_window = AppWindow::new()?;
    let window_handle = main_window.as_weak();

    let current_synced_lyrics = Arc::new(parking_lot::RwLock::new(SyncedLyrics::default()));
    let current_cover_url = Arc::new(parking_lot::RwLock::new(String::new()));

    // Set initial window properties
    main_window.set_is_logged_in(has_saved_tokens);
    main_window.set_login_client_id(SharedString::from(config.client_id.clone()));
    main_window.set_ram_usage_text(SharedString::from(format!(
        "RAM ~{:.1} MB",
        get_process_memory_mb()
    )));

    // Fast Parallel Loader for user library
    let reload_user_library = {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let cache = track_cache.clone();

        move || {
            let api1 = api.clone();
            let handle1 = handle.clone();
            rt.spawn(async move {
                if let Ok(user) = api1.get_current_user().await {
                    let display_name = user.display_name.unwrap_or_else(|| "Spotify User".to_string());
                    let _ = handle1.upgrade_in_event_loop(move |w| {
                        w.set_user_name(SharedString::from(display_name));
                    });
                }
            });

            let api2 = api.clone();
            let handle2 = handle.clone();
            rt.spawn(async move {
                if let Ok(playlists_page) = api2.get_user_playlists(50, 0).await {
                    let items: Vec<PlaylistItem> = playlists_page
                        .items
                        .into_iter()
                        .map(|p| PlaylistItem {
                            id: SharedString::from(p.id),
                            name: SharedString::from(p.name),
                            total_tracks: p.tracks.total as i32,
                            uri: SharedString::from(p.uri),
                        })
                        .collect();

                    let _ = handle2.upgrade_in_event_loop(move |w| {
                        w.set_playlists(ModelRc::new(VecModel::from(items)));
                    });
                }
            });

            let api3 = api.clone();
            let handle3 = handle.clone();
            let cache3 = cache.clone();
            rt.spawn(async move {
                if let Ok(saved_page) = api3.get_saved_tracks(50, 0).await {
                    let tracks: Vec<UiTrack> = saved_page
                        .items
                        .into_iter()
                        .map(|c| track_to_ui(c.track, true))
                        .collect();

                    cache3.write().insert("liked_songs".to_string(), tracks.clone());

                    let _ = handle3.upgrade_in_event_loop(move |w| {
                        w.set_current_view_title(SharedString::from("Liked Songs"));
                        w.set_current_view_subtitle(SharedString::from(format!("{} tracks in library", tracks.len())));
                        w.set_current_tracks(ModelRc::new(VecModel::from(tracks)));
                        w.set_is_loading_tracks(false);
                    });
                }
            });
        }
    };

    if has_saved_tokens {
        reload_user_library();
    }

    // Helper to refresh devices list and UI properties
    let refresh_devices_ui = {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let dev_store = target_device.clone();
        let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "CRISTIANO".to_string()).to_lowercase();

        move || {
            let api = api.clone();
            let handle = handle.clone();
            let dev_store = dev_store.clone();
            let hostname = hostname.clone();

            rt.spawn(async move {
                if let Ok(devices) = api.get_available_devices().await {
                    let ui_devs: Vec<UiDevice> = devices.iter().map(device_to_ui).collect();

                    let active_dev = devices.iter().find(|d| d.is_active).cloned();
                    if let Some(ref act) = active_dev {
                        if let Some(ref id) = act.id {
                            let is_remote = !act.name.to_lowercase().contains(&hostname);
                            *dev_store.write() = Some(CurrentDevice {
                                id: id.clone(),
                                name: act.name.clone(),
                                device_type: act.normalized_type().to_string(),
                                is_remote,
                            });
                        }
                    }

                    let (act_name, act_type, is_rem) = if let Some(ref act) = active_dev {
                        let is_remote = !act.name.to_lowercase().contains(&hostname);
                        (act.name.clone(), act.normalized_type().to_string(), is_remote)
                    } else if let Some(cur) = dev_store.read().clone() {
                        (cur.name, cur.device_type, cur.is_remote)
                    } else {
                        ("Questo PC".to_string(), "Computer".to_string(), false)
                    };

                    let _ = handle.upgrade_in_event_loop(move |w| {
                        w.set_devices_list(ModelRc::new(VecModel::from(ui_devs)));
                        w.set_active_device_name(SharedString::from(act_name));
                        w.set_active_device_type(SharedString::from(act_type));
                        w.set_is_remote_device(is_rem);
                    });
                }
            });
        }
    };

    // 1. Client ID & Login Callbacks
    {
        let auth = auth_manager.clone();
        let storage = storage.clone();
        main_window.on_login_client_id_changed(move |new_id| {
            let new_id_str = new_id.trim().to_string();
            auth.set_client_id(new_id_str.clone());
            let mut cfg = storage.load_config();
            cfg.client_id = new_id_str;
            let _ = storage.save_config(&cfg);
        });
    }

    {
        main_window.on_open_spotify_dashboard(move || {
            log::info!("Opening Spotify Developer Dashboard in browser");
            let _ = open::that("https://developer.spotify.com/dashboard");
        });
    }

    {
        let auth = auth_manager.clone();
        let storage = storage.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let reload_lib = reload_user_library.clone();
        let refresh_devs = refresh_devices_ui.clone();

        main_window.on_request_login(move || {
            let current_cid = auth.get_client_id();
            if current_cid.trim().is_empty() {
                let _ = handle.upgrade_in_event_loop(|w| {
                    w.set_is_logging_in(false);
                    w.set_login_status_message(SharedString::from("Inserisci prima il tuo Spotify Client ID"));
                });
                return;
            }

            let auth = auth.clone();
            let storage = storage.clone();
            let handle = handle.clone();
            let reload_lib = reload_lib.clone();
            let refresh_devs = refresh_devs.clone();

            let _ = handle.upgrade_in_event_loop(|w| {
                w.set_is_logging_in(true);
                w.set_login_status_message(SharedString::from("Apertura del browser per l'autorizzazione Spotify..."));
            });

            rt.spawn(async move {
                match auth.start_pkce_web_login(8888).await {
                    Ok(tokens) => {
                        let _ = storage.save_tokens(&tokens);
                        let _ = handle.upgrade_in_event_loop(move |w| {
                            w.set_is_logging_in(false);
                            w.set_is_logged_in(true);
                            w.set_login_status_message(SharedString::from(""));
                        });
                        reload_lib();
                        refresh_devs();
                    }
                    Err(e) => {
                        log::error!("Login failed: {}", e);
                        let err_msg = format!("Errore di accesso: {}", e);
                        let _ = handle.upgrade_in_event_loop(move |w| {
                            w.set_is_logging_in(false);
                            w.set_login_status_message(SharedString::from(err_msg));
                        });
                    }
                }
            });
        });
    }

    // 2. Logout Callback
    {
        let storage = storage.clone();
        let handle = window_handle.clone();
        let cache = track_cache.clone();
        main_window.on_request_logout(move || {
            storage.delete_tokens();
            cache.write().clear();
            let _ = handle.upgrade_in_event_loop(|w| {
                w.set_is_logged_in(false);
                w.set_current_tracks(ModelRc::new(VecModel::from(Vec::<UiTrack>::new())));
                w.set_playlists(ModelRc::new(VecModel::from(Vec::<PlaylistItem>::new())));
            });
        });
    }

    // 3. Nav: Liked Songs
    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let cache = track_cache.clone();

        main_window.on_nav_liked_songs(move || {
            if let Some(cached) = cache.read().get("liked_songs").cloned() {
                let count = cached.len();
                let _ = handle.upgrade_in_event_loop(move |w| {
                    w.set_current_view_title(SharedString::from("Liked Songs"));
                    w.set_current_view_subtitle(SharedString::from(format!("{} tracks in library", count)));
                    w.set_current_tracks(ModelRc::new(VecModel::from(cached)));
                    w.set_is_loading_tracks(false);
                });
            } else {
                let _ = handle.upgrade_in_event_loop(|w| {
                    w.set_is_loading_tracks(true);
                });
            }

            let api = api.clone();
            let handle = handle.clone();
            let cache = cache.clone();
            rt.spawn(async move {
                if let Ok(saved_page) = api.get_saved_tracks(50, 0).await {
                    let tracks: Vec<UiTrack> = saved_page
                        .items
                        .into_iter()
                        .map(|c| track_to_ui(c.track, true))
                        .collect();

                    cache.write().insert("liked_songs".to_string(), tracks.clone());

                    let _ = handle.upgrade_in_event_loop(move |w| {
                        w.set_current_view_title(SharedString::from("Liked Songs"));
                        w.set_current_view_subtitle(SharedString::from(format!("{} tracks in library", tracks.len())));
                        w.set_current_tracks(ModelRc::new(VecModel::from(tracks)));
                        w.set_is_loading_tracks(false);
                    });
                }
            });
        });
    }

    // 4. Nav: Select Playlist
    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let cache = track_cache.clone();

        main_window.on_select_playlist(move |_idx, playlist_id| {
            let pl_id = playlist_id.to_string();

            if let Some(cached) = cache.read().get(&pl_id).cloned() {
                let count = cached.len();
                let _ = handle.upgrade_in_event_loop(move |w| {
                    w.set_current_view_title(SharedString::from("Playlist"));
                    w.set_current_view_subtitle(SharedString::from(format!("{} tracks", count)));
                    w.set_current_tracks(ModelRc::new(VecModel::from(cached)));
                    w.set_is_loading_tracks(false);
                });
            } else {
                let _ = handle.upgrade_in_event_loop(|w| {
                    w.set_is_loading_tracks(true);
                });
            }

            let api = api.clone();
            let handle = handle.clone();
            let cache = cache.clone();
            let pl_id_clone = pl_id.clone();

            rt.spawn(async move {
                if let Ok(page) = api.get_playlist_tracks(&pl_id_clone, 100, 0).await {
                    let tracks: Vec<UiTrack> = page
                        .items
                        .into_iter()
                        .filter_map(|c| c.track)
                        .map(|t| track_to_ui(t, false))
                        .collect();

                    cache.write().insert(pl_id_clone, tracks.clone());

                    let _ = handle.upgrade_in_event_loop(move |w| {
                        w.set_current_view_title(SharedString::from("Playlist"));
                        w.set_current_view_subtitle(SharedString::from(format!("{} tracks", tracks.len())));
                        w.set_current_tracks(ModelRc::new(VecModel::from(tracks)));
                        w.set_is_loading_tracks(false);
                    });
                }
            });
        });
    }

    // 5. Search Query Changed
    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        main_window.on_search_query_changed(move |query| {
            let q = query.to_string();
            if q.trim().is_empty() {
                let _ = handle.upgrade_in_event_loop(|w| {
                    w.set_search_results(ModelRc::new(VecModel::from(Vec::<UiTrack>::new())));
                    w.set_is_searching(false);
                });
                return;
            }

            let api = api.clone();
            let handle = handle.clone();
            let _ = handle.upgrade_in_event_loop(|w| {
                w.set_is_searching(true);
            });

            rt.spawn(async move {
                if let Ok(results) = api.search(&q, 20).await {
                    let tracks: Vec<UiTrack> = results
                        .tracks
                        .map(|p| p.items)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|t| track_to_ui(t, false))
                        .collect();

                    let _ = handle.upgrade_in_event_loop(move |w| {
                        w.set_search_results(ModelRc::new(VecModel::from(tracks)));
                        w.set_is_searching(false);
                    });
                }
            });
        });
    }

    // 6. Play Track
    {
        let api1 = api_client.clone();
        let rt1 = tokio_rt.clone();
        let handle1 = window_handle.clone();
        let dev_store1 = target_device.clone();

        main_window.on_track_double_clicked(move |_idx, uri| {
            let api = api1.clone();
            let uri_str = uri.to_string();
            let handle = handle1.clone();
            let dev_store = dev_store1.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());

            rt1.spawn(async move {
                log::info!("Playing track URI: {} on device: {:?}", uri_str, target_dev_id);
                let res = api.play(target_dev_id.as_deref(), None, Some(vec![uri_str.clone()]), None).await;
                if res.is_ok() {
                    let _ = handle.upgrade_in_event_loop(|w| {
                        w.set_is_playing(true);
                    });
                } else if let Ok(devices) = api.get_available_devices().await {
                    if let Some(best) = find_pc_device(&devices) {
                        let best_id = best.id.clone();
                        *dev_store.write() = Some(best);
                        let _ = api.play(Some(&best_id), None, Some(vec![uri_str]), None).await;
                        let _ = handle.upgrade_in_event_loop(|w| {
                            w.set_is_playing(true);
                        });
                    }
                }
            });
        });

        let api2 = api_client.clone();
        let rt2 = tokio_rt.clone();
        let handle2 = window_handle.clone();
        let dev_store2 = target_device.clone();

        main_window.on_search_track_selected(move |_idx, uri| {
            let api = api2.clone();
            let uri_str = uri.to_string();
            let handle = handle2.clone();
            let dev_store = dev_store2.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());

            rt2.spawn(async move {
                log::info!("Playing search track URI: {} on device: {:?}", uri_str, target_dev_id);
                let res = api.play(target_dev_id.as_deref(), None, Some(vec![uri_str.clone()]), None).await;
                if res.is_ok() {
                    let _ = handle.upgrade_in_event_loop(|w| {
                        w.set_is_playing(true);
                    });
                } else if let Ok(devices) = api.get_available_devices().await {
                    if let Some(best) = find_pc_device(&devices) {
                        let best_id = best.id.clone();
                        *dev_store.write() = Some(best);
                        let _ = api.play(Some(&best_id), None, Some(vec![uri_str]), None).await;
                        let _ = handle.upgrade_in_event_loop(|w| {
                            w.set_is_playing(true);
                        });
                    }
                }
            });
        });
    }

    // 7. Player Controls (Play/Pause, Next, Prev, Seek, Volume, Shuffle, Repeat, Like)
    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let dev_store = target_device.clone();

        main_window.on_player_toggle_play_pause(move || {
            let api = api.clone();
            let handle = handle.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());

            rt.spawn(async move {
                if let Ok(Some(state)) = api.get_playback_state().await {
                    if state.is_playing {
                        let _ = api.pause(target_dev_id.as_deref()).await;
                    } else {
                        let _ = api.play(target_dev_id.as_deref(), None, None, None).await;
                    }
                    let _ = handle.upgrade_in_event_loop(move |w| {
                        w.set_is_playing(!state.is_playing);
                    });
                } else {
                    let _ = api.play(target_dev_id.as_deref(), None, None, None).await;
                }
            });
        });
    }

    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let dev_store = target_device.clone();
        main_window.on_player_skip_next(move || {
            let api = api.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());
            rt.spawn(async move {
                let _ = api.next(target_dev_id.as_deref()).await;
            });
        });
    }

    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let dev_store = target_device.clone();
        main_window.on_player_skip_prev(move || {
            let api = api.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());
            rt.spawn(async move {
                let _ = api.previous(target_dev_id.as_deref()).await;
            });
        });
    }

    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let shuffle_store = is_shuffle_active.clone();
        let dev_store = target_device.clone();
        let handle = window_handle.clone();
        main_window.on_player_toggle_shuffle(move || {
            let current = shuffle_store.load(Ordering::SeqCst);
            let next_val = !current;
            shuffle_store.store(next_val, Ordering::SeqCst);

            let _ = handle.upgrade_in_event_loop(move |w| {
                w.set_shuffle_active(next_val);
            });

            let api = api.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());
            rt.spawn(async move {
                let _ = api.set_shuffle(next_val, target_dev_id.as_deref()).await;
            });
        });
    }

    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let repeat_store = repeat_mode.clone();
        let dev_store = target_device.clone();
        let handle = window_handle.clone();
        main_window.on_player_toggle_repeat(move || {
            let cur = repeat_store.read().clone();
            let next_mode = match cur.as_str() {
                "off" => "context",
                "context" => "track",
                _ => "off",
            };
            *repeat_store.write() = next_mode.to_string();

            let is_active = next_mode != "off";
            let _ = handle.upgrade_in_event_loop(move |w| {
                w.set_repeat_active(is_active);
            });

            let api = api.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());
            let next_mode_str = next_mode.to_string();
            rt.spawn(async move {
                let _ = api.set_repeat(&next_mode_str, target_dev_id.as_deref()).await;
            });
        });
    }

    // Like / Unlike Current Track
    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let track_id_store = current_track_id.clone();
        let liked_store = is_current_liked.clone();
        let handle = window_handle.clone();
        let cache = track_cache.clone();

        main_window.on_player_toggle_like(move || {
            let tid = track_id_store.read().clone();
            if tid.is_empty() {
                return;
            }

            let currently_liked = liked_store.load(Ordering::SeqCst);
            let new_liked = !currently_liked;
            liked_store.store(new_liked, Ordering::SeqCst);

            let _ = handle.upgrade_in_event_loop(move |w| {
                w.set_is_liked(new_liked);
            });

            let api = api.clone();
            let cache = cache.clone();
            rt.spawn(async move {
                if new_liked {
                    let _ = api.save_track(&tid).await;
                } else {
                    let _ = api.remove_saved_track(&tid).await;
                }
                cache.write().remove("liked_songs");
            });
        });
    }

    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let dev_store = target_device.clone();
        main_window.on_player_seek_requested(move |ratio| {
            let api = api.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());
            rt.spawn(async move {
                if let Ok(Some(state)) = api.get_playback_state().await {
                    if let Some(item) = state.item {
                        let target_ms = (item.duration_ms as f32 * ratio) as u64;
                        let _ = api.seek(target_ms, target_dev_id.as_deref()).await;
                    }
                }
            });
        });
    }

    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let dev_store = target_device.clone();
        main_window.on_player_volume_changed(move |vol| {
            let _ = handle.upgrade_in_event_loop(move |w| {
                w.set_volume(vol);
            });

            let api = api.clone();
            let target_dev_id = dev_store.read().as_ref().map(|d| d.id.clone());
            let vol_pct = (vol * 100.0).clamp(0.0, 100.0) as u32;
            rt.spawn(async move {
                let _ = api.set_volume(vol_pct, target_dev_id.as_deref()).await;
            });
        });
    }

    // 8. Device Switching & Jam Callbacks
    {
        let refresh = refresh_devices_ui.clone();
        main_window.on_player_toggle_devices(move || {
            refresh();
        });
    }

    {
        let refresh = refresh_devices_ui.clone();
        main_window.on_refresh_devices_requested(move || {
            refresh();
        });
    }

    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let refresh = refresh_devices_ui.clone();

        main_window.on_select_device_requested(move |dev_id_ss| {
            let dev_id = dev_id_ss.to_string();
            if dev_id.is_empty() {
                return;
            }

            let api = api.clone();
            let refresh = refresh.clone();
            rt.spawn(async move {
                log::info!("Transferring playback to device: {}", dev_id);
                if let Err(e) = api.transfer_playback(&dev_id, true).await {
                    log::error!("Failed to transfer playback to {}: {}", dev_id, e);
                }
                tokio::time::sleep(Duration::from_millis(400)).await;
                refresh();
            });
        });
    }

    {
        main_window.on_open_jam_requested(move |link_ss| {
            let link = link_ss.trim().to_string();
            if !link.is_empty() {
                let target_url = if link.starts_with("http://") || link.starts_with("https://") || link.starts_with("spotify:") {
                    link
                } else {
                    format!("https://spotify.link/{}", link)
                };
                log::info!("Opening Jam / Spotify link in browser: {}", target_url);
                let _ = open::that(&target_url);
            }
        });
    }

    {
        let tid_store = current_track_id.clone();
        main_window.on_share_playback_requested(move || {
            let tid = tid_store.read().clone();
            if !tid.is_empty() {
                let url = format!("https://open.spotify.com/track/{}", tid);
                log::info!("Opening / sharing current track: {}", url);
                let _ = open::that(&url);
            }
        });
    }

    // 9. Lyrics Fetcher
    {
        let lyrics_api = lyrics_client.clone();
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let lyrics_store = current_synced_lyrics.clone();

        main_window.on_player_toggle_lyrics(move || {
            let lyrics_api = lyrics_api.clone();
            let api = api.clone();
            let handle = handle.clone();
            let lyrics_store = lyrics_store.clone();

            rt.spawn(async move {
                if let Ok(Some(state)) = api.get_playback_state().await {
                    if let Some(item) = state.item {
                        let artist_name = item.artists.first().map(|a| a.name.as_str()).unwrap_or("");
                        let album_name = item.album.as_ref().map(|a| a.name.as_str());
                        let duration_secs = Some(item.duration_ms / 1000);

                        if let Ok(lyrics) = lyrics_api
                            .fetch_lyrics(&item.name, artist_name, album_name, duration_secs)
                            .await
                        {
                            *lyrics_store.write() = lyrics.clone();

                            let items: Vec<LyricItem> = lyrics
                                .lines
                                .iter()
                                .map(|l| LyricItem {
                                    time_ms: l.start_time_ms as i32,
                                    text: SharedString::from(l.text.clone()),
                                    is_active: false,
                                })
                                .collect();

                            let has_l = !items.is_empty();
                            let plain = lyrics.plain_lyrics.clone().unwrap_or_default();

                            let _ = handle.upgrade_in_event_loop(move |w| {
                                w.set_lyrics_lines(ModelRc::new(VecModel::from(items)));
                                w.set_has_lyrics(has_l);
                                w.set_plain_lyrics(SharedString::from(plain));
                            });
                        }
                    }
                }
            });
        });
    }

    // 10. Background Playback & State Poller (runs continuously at 1s intervals)
    {
        let api = api_client.clone();
        let rt = tokio_rt.clone();
        let handle = window_handle.clone();
        let image_cache = image_cache.clone();
        let lyrics_store = current_synced_lyrics.clone();
        let cover_url_store = current_cover_url.clone();
        let track_id_store = current_track_id.clone();
        let shuffle_store = is_shuffle_active.clone();
        let repeat_store = repeat_mode.clone();
        let liked_store = is_current_liked.clone();
        let dev_store = target_device.clone();

        rt.spawn(async move {
            let http_client = reqwest::Client::new();
            let mut tick_counter: u64 = 0;
            let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "CRISTIANO".to_string()).to_lowercase();

            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                tick_counter += 1;

                // Update available devices in background every 3 ticks
                if tick_counter % 3 == 0 {
                    if let Ok(devices) = api.get_available_devices().await {
                        let active_dev = devices.iter().find(|d| d.is_active).cloned();
                        if let Some(ref act) = active_dev {
                            if let Some(ref id) = act.id {
                                let is_remote = !act.name.to_lowercase().contains(&hostname);
                                *dev_store.write() = Some(CurrentDevice {
                                    id: id.clone(),
                                    name: act.name.clone(),
                                    device_type: act.normalized_type().to_string(),
                                    is_remote,
                                });
                            }
                        } else if dev_store.read().is_none() {
                            if let Some(best) = find_pc_device(&devices) {
                                *dev_store.write() = Some(best);
                            }
                        }

                        let ui_devs: Vec<UiDevice> = devices.iter().map(device_to_ui).collect();
                        let _ = handle.upgrade_in_event_loop(move |w| {
                            w.set_devices_list(ModelRc::new(VecModel::from(ui_devs)));
                        });
                    }
                }

                if let Ok(Some(state)) = api.get_playback_state().await {
                    let is_playing = state.is_playing;
                    let progress_ms = state.progress_ms.unwrap_or_default();

                    let shuffle_state = state.shuffle_state.unwrap_or(false);
                    let repeat_state = state.repeat_state.unwrap_or_else(|| "off".to_string());
                    shuffle_store.store(shuffle_state, Ordering::SeqCst);
                    *repeat_store.write() = repeat_state.clone();

                    if let Some(ref d) = state.device {
                        if let Some(ref id) = d.id {
                            let is_remote = !d.name.to_lowercase().contains(&hostname);
                            *dev_store.write() = Some(CurrentDevice {
                                id: id.clone(),
                                name: d.name.clone(),
                                device_type: d.normalized_type().to_string(),
                                is_remote,
                            });
                        }
                    }

                    let (title, artist, duration_ms, cover_url, track_id) = if let Some(ref t) = state.item {
                        (
                            t.name.clone(),
                            t.artist_names(),
                            t.duration_ms,
                            t.cover_url().unwrap_or_default(),
                            t.id.clone().unwrap_or_default(),
                        )
                    } else {
                        ("Not Playing".to_string(), "Select a track to start".to_string(), 1, String::new(), String::new())
                    };

                    // Update current track ID & check if liked when changed
                    let old_tid = track_id_store.read().clone();
                    if !track_id.is_empty() && track_id != old_tid {
                        *track_id_store.write() = track_id.clone();
                        if let Ok(contains) = api.check_saved_tracks(&[&track_id]).await {
                            let is_saved = contains.first().copied().unwrap_or(false);
                            liked_store.store(is_saved, Ordering::SeqCst);
                        }
                    }
                    let is_liked = liked_store.load(Ordering::SeqCst);

                    let progress_ratio = (progress_ms as f32 / duration_ms.max(1) as f32).clamp(0.0, 1.0);
                    let cur_secs = progress_ms / 1000;
                    let tot_secs = duration_ms / 1000;
                    let time_current = format!("{:02}:{:02}", cur_secs / 60, cur_secs % 60);
                    let time_total = format!("{:02}:{:02}", tot_secs / 60, tot_secs % 60);

                    // Check if lyrics need active line update
                    let active_lyric_idx = {
                        let l = lyrics_store.read();
                        l.current_line_index(progress_ms).map(|i| i as i32).unwrap_or(-1)
                    };

                    // Check if cover art needs loading
                    let old_cover = cover_url_store.read().clone();
                    let needs_cover_load = !cover_url.is_empty() && cover_url != old_cover;

                    let mut raw_image: Option<Arc<RgbaRawImage>> = None;
                    if needs_cover_load {
                        *cover_url_store.write() = cover_url.clone();
                        if let Some(cached) = image_cache.get(&cover_url) {
                            raw_image = Some(cached);
                        } else if let Ok(res) = http_client.get(&cover_url).send().await {
                            if let Ok(bytes) = res.bytes().await {
                                raw_image = image_cache.insert_and_scale(cover_url.clone(), &bytes, 96, 96);
                            }
                        }
                    }

                    // Update memory usage every 5 ticks
                    let ram_text = if tick_counter % 5 == 0 {
                        Some(format!("RAM ~{:.1} MB", get_process_memory_mb()))
                    } else {
                        None
                    };

                    let (active_dev_name, active_dev_type, is_remote_dev) = if let Some(cur) = dev_store.read().clone() {
                        (cur.name, cur.device_type, cur.is_remote)
                    } else {
                        ("Questo PC".to_string(), "Computer".to_string(), false)
                    };

                    let bitrate_tag = if is_remote_dev {
                        format!("320k [{}]", active_dev_name)
                    } else {
                        "320k [WASAPI]".to_string()
                    };

                    let is_repeat = repeat_state != "off";
                    let _ = handle.upgrade_in_event_loop(move |w| {
                        w.set_is_playing(is_playing);
                        w.set_shuffle_active(shuffle_state);
                        w.set_repeat_active(is_repeat);
                        w.set_is_liked(is_liked);
                        w.set_current_track_title(SharedString::from(title));
                        w.set_current_track_artist(SharedString::from(artist));
                        w.set_progress_ratio(progress_ratio);
                        w.set_time_current(SharedString::from(time_current));
                        w.set_time_total(SharedString::from(time_total));
                        w.set_active_lyric_index(active_lyric_idx);
                        w.set_bitrate_label(SharedString::from(bitrate_tag));
                        w.set_active_device_name(SharedString::from(active_dev_name));
                        w.set_active_device_type(SharedString::from(active_dev_type));
                        w.set_is_remote_device(is_remote_dev);

                        if let Some(raw) = raw_image {
                            w.set_current_track_cover(raw.to_slint_image());
                            w.set_has_cover(true);
                        }

                        if let Some(ram) = ram_text {
                            w.set_ram_usage_text(SharedString::from(ram));
                        }
                    });
                }
            }
        });
    }

    log::info!("Lyra UI ready. Running window event loop...");
    main_window.run()?;
    Ok(())
}

