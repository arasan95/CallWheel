# CallWheel

[🇬🇧 English](#english) · [🇯🇵 日本語](#japanese)

---

## <a id="english"></a>🇬🇧 English

A radial menu tool for quickly sending predefined phrases in games and other applications.  
Press and hold a hotkey, swipe in a direction, and release to copy/type your phrase instantly.

![platform](https://img.shields.io/badge/platform-Windows-blue)
![rust](https://img.shields.io/badge/rust-2024-orange)

### Features

- **Radial wheel overlay** — Press a hotkey to show a direction wheel centered on your cursor
- **8/6/4 direction modes** — Per-profile configurable number of slots
- **Clipboard & direct input** — Copy to clipboard, type via `SendInput`, or both
- **Configurable hotkeys** — Assign any key (A–Z, 0–9, F1–F12, Space, Enter, Ctrl, Shift, Alt, etc.)
- **Multiple profiles** — Create named sets with different hotkeys and phrases
- **Selection sound & animation** — Optional audio/visual feedback
- **Bilingual UI** — Japanese and English supported, switchable from settings
- **Per-monitor DPI aware** — Renders sharply on high-DPI displays

### Usage

1. Run the application (egui settings window opens)
2. Configure your profiles: assign a hotkey and enter phrases for each direction
3. Keep the app running in the background
4. In any game or application, **hold the hotkey** → swipe mouse in a direction → **release**
5. The selected phrase is sent to clipboard, typed, or both

#### Example (League of Legends)

Open chat, hold your hotkey, swipe toward the phrase you want, release, then paste (`Ctrl+V`) or the text will be typed automatically depending on your output mode.

### Build

```powershell
cargo build --release
```

Requires the Rust toolchain (edition 2024).

### Configuration

Settings are saved as JSON to your config folder:
```
%APPDATA%/CallWheel/CallWheel/settings.json
```

Press the **Save** button in the UI to persist changes.

#### Available hotkey names

| Category | Names |
|----------|-------|
| Letters | `A` `B` … `Z` |
| Numbers | `0` `1` … `9` |
| Function | `F1` `F2` … `F12` |
| Navigation | `Up` `Down` `Left` `Right` |
| Modifiers | `Shift` `Ctrl` `Alt` (with L/R prefix) |
| Special | `Space` `Enter` `Tab` `Escape` `Backspace` |

### Notes

- The overlay is designed for **borderless windowed or windowed mode**; exclusive fullscreen may hide it.
- Right-click or middle-click cancels the wheel.
- The tool does not inject into game processes — it only copies to clipboard or simulates keyboard input via Windows `SendInput`.

---

## <a id="japanese"></a>🇯🇵 日本語

ゲームなどで定型文を素早く送信するためのラジアルメニューツールです。  
ホットキーを長押ししてマウスをスワイプし、キーを離すと定型文をクリップボードへコピー / 直接入力できます。

![platform](https://img.shields.io/badge/platform-Windows-blue)
![rust](https://img.shields.io/badge/rust-2024-orange)

### 機能

- **ラジアルホイールオーバーレイ** — ホットキーを押すとカーソル位置に方向ホイールを表示
- **8 / 6 / 4 方向モード** — プロファイルごとにスロット数を設定可能
- **クリップボード & 直接入力** — クリップボードにコピー、`SendInput` で入力、またはその両方
- **自由なホットキー割り当て** — A–Z, 0–9, F1–F12, Space, Enter, Ctrl, Shift, Alt など任意のキーを割り当て可能
- **複数プロファイル** — 名前付きセットを作成し、ホットキーとフレーズを設定
- **選択サウンド & アニメーション** — オプションの音声・視覚フィードバック
- **二言語UI** — 日本語と英語に対応、設定画面から切替可能
- **DPI対応** — 高DPIディスプレイでもシャープに表示

### 使い方

1. アプリを起動（egui設定ウィンドウが開きます）
2. プロファイルを設定：ホットキーと各方向の定型文を入力
3. アプリを起動したままバックグラウンドで待機
4. ゲームなどで **ホットキーを長押し** → マウスをスワイプ → **キーを離す**
5. 選択した定型文がクリップボードにコピー / 入力されます

#### League of Legends での例

チャットを開き、ホットキーを長押し → 目的の方向にスワイプ → キーを離す → `Ctrl+V` で貼り付け（出力モードによっては自動入力されます）

### ビルド

```powershell
cargo build --release
```

Rustツールチェーン（edition 2024）が必要です。

### 設定

設定はJSONとして以下のフォルダに保存されます：
```
%APPDATA%/CallWheel/CallWheel/settings.json
```

UIの**保存**ボタンを押すと変更が反映されます。

#### 使用可能なホットキー名

| カテゴリ | 名前 |
|----------|------|
| 英字 | `A` `B` … `Z` |
| 数字 | `0` `1` … `9` |
| ファンクション | `F1` `F2` … `F12` |
| ナビゲーション | `Up` `Down` `Left` `Right` |
| 修飾キー | `Shift` `Ctrl` `Alt`（L/R プレフィックス付き） |
| 特殊キー | `Space` `Enter` `Tab` `Escape` `Backspace` |

### 注意

- オーバーレイは**ボーダーレスウィンドウまたはウィンドウモード**での利用を想定しています。排他フルスクリーンでは表示されないことがあります。
- 右クリックまたはホイールクリックでホイールをキャンセルできます。
- 本ツールはゲームプロセスに注入しません。クリップボードへのコピー、またはWindows `SendInput` によるキーボード入力のシミュレーションのみを行います。

---

*CallWheel — Made with Rust & egui*
