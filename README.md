# Lyra

> Ultra-low memory Spotify desktop client built with native Rust and Slint.

[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Slint](https://img.shields.io/badge/GUI-Slint%201.9-purple.svg)](https://slint.dev)

Lyra is a fast, lightweight, and resource-efficient Spotify client designed to replace heavy Electron/Chromium-based desktop apps. It runs in a single process consuming only **~20 MB of RAM** while providing **Bit-Perfect WASAPI** audio playback, real-time synchronized lyrics via LRCLIB, and complete local privacy.

---

## ⚡ Features

- **Extreme Low Memory Footprint**: Uses ~20 MB of RAM at idle (<0.1% CPU).
- **Bit-Perfect WASAPI Audio**: Direct low-latency hardware stream at 320 kbps Vorbis.
- **Synchronized Lyrics**: Millisecond-accurate real-time lyrics powered by [LRCLIB](https://lrclib.net).
- **Spotify Connect & Jam**: Full remote playback transfer and Jam session support.
- **Zero Telemetry**: All tokens and configuration stay 100% local on your disk (`config.json`).
- **Native GPU UI**: Rendered with Slint via hardware acceleration.

---

## 📦 Installation

### Windows (via WinGet)
```powershell
winget install cristianobleve.lyra
```

### Build from Source
Ensure you have the latest Rust toolchain installed:
```bash
git clone https://github.com/cristianobleve/lyra.git
cd lyra
cargo run --release
```

---

## 🔑 Spotify Developer Setup

In accordance with Spotify's developer access policies:
1. Go to the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard) and click **Create App**.
2. Set the Redirect URI to `http://127.0.0.1:8888/callback` and enable **Web API**.
3. Copy your **Client ID**, paste it into Lyra on startup, and click **Connect**.

---

## 🌐 Website Showcase

A modern showcase landing page is available in the [`website/`](./website) folder.

---

## 📄 License

Dual-licensed under either of:
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
