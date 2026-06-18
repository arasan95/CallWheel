# CallWheel

A radial menu tool for quickly sending predefined phrases in games and other applications.  
Press and hold a hotkey, swipe in a direction, and release to copy/type your phrase instantly.

![screenshot](https://img.shields.io/badge/platform-Windows-blue)
![rust](https://img.shields.io/badge/rust-2024-orange)

## Features

- **Radial wheel overlay** — Press a hotkey to show a direction wheel centered on your cursor
- **8/6/4 direction modes** — Per-profile configurable number of slots
- **Clipboard & direct input** — Copy to clipboard, type via `SendInput`, or both
- **Configurable hotkeys** — Assign any key (A–Z, 0–9, F1–F12, Space, Enter, Ctrl, Shift, Alt, etc.)
- **Multiple profiles** — Create named sets with different hotkeys and phrases
- **Selection sound & animation** — Optional audio/visual feedback
- **Bilingual UI** — Japanese and English supported, switchable from settings
- **Per-monitor DPI aware** — Renders sharply on high-DPI displays

## Usage

1. Run the application (egui settings window opens)
2. Configure your profiles: assign a hotkey and enter phrases for each direction
3. Keep the app running in the background
4. In any game or application, **hold the hotkey** → swipe mouse in a direction → **release**
5. The selected phrase is sent to clipboard, typed, or both

### Example (League of Legends)

Open chat, hold your hotkey, swipe toward the phrase you want, release, then paste (`Ctrl+V`) or the text will be typed automatically depending on your output mode.

## Build

```powershell
cargo build --release
```

Requires the Rust toolchain (edition 2024).

## Configuration

Settings are saved as JSON to your config folder:
```
%APPDATA%/CallWheel/CallWheel/settings.json
```

Press the **Save** button in the UI to persist changes.

### Available hotkey names

| Category | Names |
|----------|-------|
| Letters | `A` `B` … `Z` |
| Numbers | `0` `1` … `9` |
| Function | `F1` `F2` … `F12` |
| Navigation | `Up` `Down` `Left` `Right` |
| Modifiers | `Shift` `Ctrl` `Alt` (with L/R prefix) |
| Special | `Space` `Enter` `Tab` `Escape` `Backspace` |

## Notes

- The overlay is designed for **borderless windowed or windowed mode**; exclusive fullscreen may hide it.
- Right-click or middle-click cancels the wheel.
- The tool does not inject into game processes — it only copies to clipboard or simulates keyboard input via Windows `SendInput`.
