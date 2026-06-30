#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::{Duration, Instant},
};

use arboard::Clipboard;
use device_query::{DeviceQuery, DeviceState, Keycode};
use directories::ProjectDirs;
use eframe::{
    CreationContext,
    egui::{
        self, Align, Align2, Color32, FontData, FontDefinitions, FontFamily, Id, Layout, Pos2, Vec2,
    },
};
use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
use windows::{
    Win32::{
        Foundation::{COLORREF, CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
            CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, CreateEllipticRgn,
            CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH,
            DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DeleteObject,
            DrawTextW, Ellipse, FW_BOLD, FW_NORMAL, FillRect, GetDC, HALFTONE, HDC, HFONT, LineTo,
            MoveToEx, OUT_DEFAULT_PRECIS, PS_SOLID, ReleaseDC, SRCCOPY, SetBkMode,
            SetStretchBltMode, SetTextColor, SetWindowRgn, StretchBlt, TRANSPARENT,
        },
        Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME},
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::{
            HiDpi::{GetDpiForSystem, PROCESS_PER_MONITOR_DPI_AWARE, SetProcessDpiAwareness},
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
                KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, SendInput, VK_RBUTTON,
            },
            Shell::ExtractIconExW,
            WindowsAndMessaging::{
                CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DI_NORMAL, DefWindowProcW, DestroyIcon,
                DestroyWindow, DrawIconEx, FindWindowW, GWL_EXSTYLE, GetForegroundWindow,
                GetWindowLongPtrW, GetWindowThreadProcessId, HICON, HWND_TOPMOST, LWA_ALPHA,
                RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW,
                SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, WNDCLASSW,
                WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, PWSTR},
};

const DIRECTIONS_8: [Direction; 8] = [
    Direction::Up,
    Direction::UpRight,
    Direction::Right,
    Direction::DownRight,
    Direction::Down,
    Direction::DownLeft,
    Direction::Left,
    Direction::UpLeft,
];
const DIRECTIONS_6: [Direction; 6] = [
    Direction::Up,
    Direction::UpRight,
    Direction::DownRight,
    Direction::Down,
    Direction::DownLeft,
    Direction::UpLeft,
];
const DIRECTIONS_4: [Direction; 4] = [
    Direction::Up,
    Direction::Right,
    Direction::Down,
    Direction::Left,
];

const OVERLAY_TITLE: &str = "CallWheel Overlay";
const NATIVE_OVERLAY_CLASS: &str = "CallWheelNativeOverlay";
const NATIVE_OVERLAY_TITLE: &str = "CallWheel Native Overlay";
const OVERLAY_ALPHA: u8 = 218;
const ENTER_SCANCODE: u16 = 0x1C;
const SELECTION_EFFECT_MS: u64 = 120;

fn default_true() -> bool {
    true
}

fn default_wheel_item_radius() -> f32 {
    49.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl Direction {
    #[allow(dead_code)]
    fn label(self) -> &'static str {
        self.label_lang(Language::Japanese)
    }

    fn label_lang(self, lang: Language) -> &'static str {
        t(
            lang,
            match self {
                Direction::Up => "上",
                Direction::UpRight => "右上",
                Direction::Right => "右",
                Direction::DownRight => "右下",
                Direction::Down => "下",
                Direction::DownLeft => "左下",
                Direction::Left => "左",
                Direction::UpLeft => "左上",
            },
            match self {
                Direction::Up => "Up",
                Direction::UpRight => "U-R",
                Direction::Right => "Right",
                Direction::DownRight => "D-R",
                Direction::Down => "Down",
                Direction::DownLeft => "D-L",
                Direction::Left => "Left",
                Direction::UpLeft => "U-L",
            },
        )
    }

    fn index(self) -> usize {
        match self {
            Direction::Up => 0,
            Direction::UpRight => 1,
            Direction::Right => 2,
            Direction::DownRight => 3,
            Direction::Down => 4,
            Direction::DownLeft => 5,
            Direction::Left => 6,
            Direction::UpLeft => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum OutputMode {
    #[default]
    Clipboard,
    TypeText,
    Both,
    OpenTypeSend,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum DirectionMode {
    Four,
    Six,
    #[default]
    Eight,
}

impl DirectionMode {
    #[allow(dead_code)]
    fn label(self) -> &'static str {
        self.label_lang(Language::Japanese)
    }

    fn label_lang(self, lang: Language) -> &'static str {
        t(
            lang,
            match self {
                DirectionMode::Four => "4方向",
                DirectionMode::Six => "6方向",
                DirectionMode::Eight => "8方向",
            },
            match self {
                DirectionMode::Four => "4-Way",
                DirectionMode::Six => "6-Way",
                DirectionMode::Eight => "8-Way",
            },
        )
    }

    fn directions(self) -> &'static [Direction] {
        match self {
            DirectionMode::Four => &DIRECTIONS_4,
            DirectionMode::Six => &DIRECTIONS_6,
            DirectionMode::Eight => &DIRECTIONS_8,
        }
    }

    fn from_delta(self, delta: Vec2, dead_zone: f32) -> Option<Direction> {
        if delta.length() < dead_zone {
            return None;
        }

        let mut angle = (-delta.y).atan2(delta.x).to_degrees();
        if angle < 0.0 {
            angle += 360.0;
        }

        self.directions().iter().copied().min_by(|a, b| {
            angle_distance(angle, direction_angle_in_mode(self, *a))
                .partial_cmp(&angle_distance(angle, direction_angle_in_mode(self, *b)))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

impl OutputMode {
    #[allow(dead_code)]
    fn label(self) -> &'static str {
        self.label_lang(Language::Japanese)
    }

    fn label_lang(self, lang: Language) -> &'static str {
        t(
            lang,
            match self {
                OutputMode::Clipboard => "コピーのみ",
                OutputMode::TypeText => "SendInputで入力",
                OutputMode::Both => "コピー + SendInput",
                OutputMode::OpenTypeSend => "Enter + SendInput + Enter",
            },
            match self {
                OutputMode::Clipboard => "Copy Only",
                OutputMode::TypeText => "Type Text",
                OutputMode::Both => "Copy + Type",
                OutputMode::OpenTypeSend => "Enter + Send + Enter",
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Language {
    Japanese,
    English,
}

impl Default for Language {
    fn default() -> Self {
        Language::Japanese
    }
}

fn t(lang: Language, ja: &'static str, en: &'static str) -> &'static str {
    match lang {
        Language::Japanese => ja,
        Language::English => en,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum SelectionSoundKind {
    Subtle,
    Koto,
    Soft,
    #[default]
    Click,
    Confirm,
    Alert,
}

impl SelectionSoundKind {
    fn label_lang(self, lang: Language) -> &'static str {
        t(
            lang,
            match self {
                SelectionSoundKind::Subtle => "控えめ",
                SelectionSoundKind::Koto => "コト",
                SelectionSoundKind::Soft => "ソフト",
                SelectionSoundKind::Click => "クリック",
                SelectionSoundKind::Confirm => "決定",
                SelectionSoundKind::Alert => "通知",
            },
            match self {
                SelectionSoundKind::Subtle => "Subtle",
                SelectionSoundKind::Koto => "Koto",
                SelectionSoundKind::Soft => "Soft",
                SelectionSoundKind::Click => "Click",
                SelectionSoundKind::Confirm => "Confirm",
                SelectionSoundKind::Alert => "Alert",
            },
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KeyProfile {
    name: String,
    hotkey: String,
    #[serde(default)]
    direction_mode: DirectionMode,
    #[serde(default)]
    phrases: Vec<String>,
}

impl KeyProfile {
    fn default_lane() -> Self {
        Self {
            name: "レーン報告".to_owned(),
            hotkey: "F1".to_owned(),
            direction_mode: DirectionMode::Eight,
            phrases: vec![
                "ワード消した".to_owned(),
                "ベイト見よ".to_owned(),
                "右見て".to_owned(),
                "リコールしたい".to_owned(),
                "視界取りに行きたい".to_owned(),
                "JG怖い、どこ?".to_owned(),
                "左見て".to_owned(),
                "ダイブできる".to_owned(),
            ],
        }
    }

    fn default_macro() -> Self {
        Self {
            name: "マクロ".to_owned(),
            hotkey: "F2".to_owned(),
            direction_mode: DirectionMode::Eight,
            phrases: vec![
                "安全にいこ".to_owned(),
                "JG近いし強気にトレードしよ".to_owned(),
                "右寄る".to_owned(),
                "ドラゴンセットアップ".to_owned(),
                "早めのピンほしい".to_owned(),
                "スペルチェック".to_owned(),
                "左寄る".to_owned(),
                "ウェーブ待って".to_owned(),
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WheelProfile {
    name: String,
    #[serde(default)]
    target_process_paths: Vec<String>,
    #[serde(default)]
    keys: Vec<KeyProfile>,
    #[serde(default, skip_serializing)]
    hotkey: String,
    #[serde(default, skip_serializing)]
    direction_mode: DirectionMode,
    #[serde(default, skip_serializing)]
    phrases: Vec<String>,
}

impl WheelProfile {
    fn default_game() -> Self {
        Self {
            name: "デフォルト".to_owned(),
            target_process_paths: Vec::new(),
            keys: vec![KeyProfile::default_lane(), KeyProfile::default_macro()],
            hotkey: String::new(),
            direction_mode: DirectionMode::Eight,
            phrases: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct AppConfig {
    profiles: Vec<WheelProfile>,
    dead_zone: f32,
    wheel_radius: f32,
    #[serde(default = "default_wheel_item_radius")]
    wheel_item_radius: f32,
    #[serde(default, rename = "direction_mode", skip_serializing)]
    legacy_direction_mode: Option<DirectionMode>,
    output_mode: OutputMode,
    type_delay_min_ms: u64,
    type_delay_max_ms: u64,
    selection_animation: bool,
    #[serde(default = "default_true")]
    selection_sound: bool,
    #[serde(default)]
    selection_sound_kind: SelectionSoundKind,
    #[serde(default)]
    language: Language,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            profiles: vec![WheelProfile::default_game()],
            dead_zone: 34.0,
            wheel_radius: 132.0,
            wheel_item_radius: default_wheel_item_radius(),
            legacy_direction_mode: None,
            output_mode: OutputMode::Clipboard,
            type_delay_min_ms: 35,
            type_delay_max_ms: 85,
            selection_animation: true,
            selection_sound: true,
            selection_sound_kind: SelectionSoundKind::default(),
            language: Language::default(),
        }
    }
}

struct ActiveWheel {
    profile_index: usize,
    key_index: usize,
    key: Keycode,
    origin: Pos2,
    selected: Option<Direction>,
}

#[derive(Clone)]
struct OverlaySnapshot {
    profile_name: String,
    phrases: Vec<String>,
    origin: Pos2,
    selected: Option<Direction>,
    wheel_radius: f32,
    wheel_item_radius: f32,
    direction_mode: DirectionMode,
    confirmed_at: Option<Instant>,
    selection_animation: bool,
    language: Language,
}

struct Toast {
    message: String,
    until: Instant,
}

struct ToastMessage {
    message: String,
    duration: Duration,
}

struct CallWheelApp {
    config: AppConfig,
    config_path: PathBuf,
    selected_profile: usize,
    selected_key: usize,
    deleted_profiles: Vec<WheelProfile>,
    toast: Option<Toast>,
    last_save_error: Option<String>,
    preview_open: bool,
    app_icon_textures: HashMap<String, egui::TextureHandle>,
    shared_config: Arc<Mutex<AppConfig>>,
    toast_rx: Receiver<ToastMessage>,
}

#[cfg(target_os = "windows")]
struct NativeOverlay {
    hwnd: Option<HWND>,
    class_registered: bool,
    visible: bool,
    last_layout: Option<NativeOverlayLayout>,
    fonts: Option<NativeOverlayFonts>,
    backbuffer: Option<NativeBackbuffer>,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NativeOverlayLayout {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    margin: i32,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
struct NativeOverlayFonts {
    scale_key: i32,
    center: HFONT,
    direction: HFONT,
    phrase: HFONT,
}

#[cfg(target_os = "windows")]
struct NativeBackbuffer {
    hdc: HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    old_bitmap: windows::Win32::Graphics::Gdi::HGDIOBJ,
    width: i32,
    height: i32,
}

#[cfg(not(target_os = "windows"))]
#[derive(Default)]
struct NativeOverlay;

#[cfg(target_os = "windows")]
impl NativeOverlay {
    fn new() -> Self {
        Self {
            hwnd: None,
            class_registered: false,
            visible: false,
            last_layout: None,
            fonts: None,
            backbuffer: None,
        }
    }

    fn show(&mut self, snapshot: &OverlaySnapshot, pixels_per_point: f32) {
        let Some(hwnd) = self.ensure_window() else {
            return;
        };

        let outer_padding = overlay_outer_padding(snapshot.wheel_item_radius);
        let extent_padding = overlay_extent_padding(snapshot.wheel_item_radius);
        let size_points = Vec2::splat((snapshot.wheel_radius + extent_padding) * 2.0);
        let size_px = (size_points * pixels_per_point).round();
        let left = (snapshot.origin.x - size_px.x / 2.0).round() as i32;
        let top = (snapshot.origin.y - size_px.y / 2.0).round() as i32;
        let width = size_px.x.max(1.0).round() as i32;
        let height = size_px.y.max(1.0).round() as i32;
        let outer_radius = (snapshot.wheel_radius + outer_padding) * pixels_per_point;
        let margin = ((size_px.x - outer_radius * 2.0) / 2.0).round() as i32;
        let layout = NativeOverlayLayout {
            left,
            top,
            width,
            height,
            margin,
        };
        let layout_changed = self.last_layout != Some(layout);

        if layout_changed || !self.visible {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    left,
                    top,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), OVERLAY_ALPHA, LWA_ALPHA);
            }
            apply_round_window_region(hwnd, size_px, margin as f32, OVERLAY_ALPHA, 1.0);
            self.last_layout = Some(layout);
            self.visible = true;
        }

        let render_scale = 2.0;
        let render_width = (width as f32 * render_scale) as i32;
        let render_height = (height as f32 * render_scale) as i32;
        let fonts = self.ensure_fonts(pixels_per_point * render_scale);
        let Some(backbuffer) = self.ensure_backbuffer(render_width, render_height) else {
            return;
        };
        draw_native_overlay(
            backbuffer.hdc,
            snapshot,
            render_width,
            render_height,
            pixels_per_point * render_scale,
            fonts,
        );
        flush_native_backbuffer(hwnd, backbuffer, width, height);
    }

    fn hide(&mut self) {
        if let Some(hwnd) = self.hwnd {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        self.visible = false;
    }

    fn ensure_window(&mut self) -> Option<HWND> {
        if let Some(hwnd) = self.hwnd {
            return Some(hwnd);
        }

        if !self.class_registered {
            let class_name = wide_null(NATIVE_OVERLAY_CLASS);
            let window_class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(native_overlay_wnd_proc),
                hInstance: HINSTANCE::default(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            unsafe {
                let _ = RegisterClassW(&window_class);
            }
            self.class_registered = true;
        }

        let class_name = wide_null(NATIVE_OVERLAY_CLASS);
        let title = wide_null(NATIVE_OVERLAY_TITLE);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                -20000,
                -20000,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .ok()?
        };

        self.hwnd = Some(hwnd);
        Some(hwnd)
    }

    fn ensure_fonts(&mut self, pixels_per_point: f32) -> NativeOverlayFonts {
        let scale_key = (pixels_per_point * 100.0).round() as i32;
        if let Some(fonts) = self.fonts
            && fonts.scale_key == scale_key
        {
            return fonts;
        }

        if let Some(fonts) = self.fonts.take() {
            delete_native_fonts(fonts);
        }

        let fonts = NativeOverlayFonts {
            scale_key,
            center: create_native_font(15.0 * pixels_per_point, false),
            direction: create_native_font(13.0 * pixels_per_point, true),
            phrase: create_native_font(14.0 * pixels_per_point, false),
        };
        self.fonts = Some(fonts);
        fonts
    }

    fn ensure_backbuffer(&mut self, width: i32, height: i32) -> Option<&NativeBackbuffer> {
        let recreate = self
            .backbuffer
            .as_ref()
            .map(|buffer| buffer.width != width || buffer.height != height)
            .unwrap_or(true);

        if recreate {
            if let Some(buffer) = self.backbuffer.take() {
                delete_native_backbuffer(buffer);
            }

            self.backbuffer = create_native_backbuffer(width, height);
        }

        self.backbuffer.as_ref()
    }
}

#[cfg(target_os = "windows")]
impl Drop for NativeOverlay {
    fn drop(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
        if let Some(fonts) = self.fonts.take() {
            delete_native_fonts(fonts);
        }
        if let Some(buffer) = self.backbuffer.take() {
            delete_native_backbuffer(buffer);
        }
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn native_overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(not(target_os = "windows"))]
impl NativeOverlay {
    fn new() -> Self {
        Self
    }

    fn show(&mut self, _snapshot: &OverlaySnapshot, _pixels_per_point: f32) {}

    fn hide(&mut self) {}
}

fn start_wheel_worker(shared_config: Arc<Mutex<AppConfig>>, toast_tx: Sender<ToastMessage>) {
    thread::spawn(move || {
        let device = DeviceState::new();
        let mut active: Option<ActiveWheel> = None;
        let mut suppressed_key: Option<Keycode> = None;
        let mut selection_effect: Option<OverlaySnapshot> = None;
        let mut native_overlay = NativeOverlay::new();

        loop {
            let frame_start = Instant::now();
            let config = shared_config
                .lock()
                .map(|config| config.clone())
                .unwrap_or_default();
            let keys = device.get_keys();
            let mouse = device.get_mouse();
            let mouse_pos = Pos2::new(mouse.coords.0 as f32, mouse.coords.1 as f32);

            if let Some(key) = suppressed_key {
                if keys.contains(&key) {
                    sleep_until_next_frame(frame_start);
                    continue;
                }
                suppressed_key = None;
            }

            if let Some(current) = &mut active {
                if mouse_cancel_pressed(&mouse.button_pressed) {
                    suppressed_key = Some(current.key);
                    active = None;
                    selection_effect = None;
                    native_overlay.hide();
                    sleep_until_next_frame(frame_start);
                    continue;
                }

                if keys.contains(&current.key) {
                    current.selected = config
                        .profiles
                        .get(current.profile_index)
                        .and_then(|profile| profile.keys.get(current.key_index))
                        .map(|key_profile| key_profile.direction_mode)
                        .unwrap_or_default()
                        .from_delta(mouse_pos - current.origin, config.dead_zone);
                } else {
                    let profile_index = current.profile_index;
                    let key_index = current.key_index;
                    let selected = current.selected;
                    let origin = current.origin;
                    active = None;

                    if let Some(direction) = selected
                        && let Some(profile) = config.profiles.get(profile_index)
                        && let Some(key_profile) = profile.keys.get(key_index)
                    {
                        let phrases = key_profile.phrases.clone();
                        let phrase = phrase_for_direction(&phrases, direction).to_owned();
                        selection_effect = if config.selection_animation {
                            Some(OverlaySnapshot {
                                profile_name: key_profile.name.clone(),
                                phrases: phrases.clone(),
                                origin,
                                selected: Some(direction),
                                wheel_radius: config.wheel_radius,
                                wheel_item_radius: config.wheel_item_radius,
                                direction_mode: key_profile.direction_mode,
                                confirmed_at: Some(Instant::now()),
                                selection_animation: true,
                                language: config.language,
                            })
                        } else {
                            None
                        };

                        if config.selection_sound {
                            play_select_sound(config.selection_sound_kind);
                        }

                        if !phrase.is_empty() {
                            spawn_output_worker(
                                config.output_mode,
                                typing_delay_range(&config),
                                phrase,
                                toast_tx.clone(),
                                config.language,
                            );
                        }
                    }
                }
            } else {
                let foreground_process_path = foreground_process_path();
                let scoped_profile_matches = config.profiles.iter().any(|profile| {
                    profile_has_process_targets(profile)
                        && profile_matches_process_path(profile, &foreground_process_path)
                });

                for (profile_index, profile) in config.profiles.iter().enumerate() {
                    if scoped_profile_matches && !profile_has_process_targets(profile) {
                        continue;
                    }

                    if !profile_matches_process_path(profile, &foreground_process_path) {
                        continue;
                    }

                    for (key_index, key_profile) in profile.keys.iter().enumerate() {
                        if let Some(key) = keycode_from_name(&key_profile.hotkey)
                            && keys.contains(&key)
                        {
                            active = Some(ActiveWheel {
                                profile_index,
                                key_index,
                                key,
                                origin: mouse_pos,
                                selected: None,
                            });
                            break;
                        }
                    }

                    if active.is_some() {
                        break;
                    }
                }
            }

            let snapshot = active
                .as_ref()
                .and_then(|current| {
                    config
                        .profiles
                        .get(current.profile_index)
                        .and_then(|profile| profile.keys.get(current.key_index))
                        .map(|key_profile| OverlaySnapshot {
                            profile_name: key_profile.name.clone(),
                            phrases: key_profile.phrases.clone(),
                            origin: current.origin,
                            selected: current.selected,
                            wheel_radius: config.wheel_radius,
                            wheel_item_radius: config.wheel_item_radius,
                            direction_mode: key_profile.direction_mode,
                            confirmed_at: None,
                            selection_animation: config.selection_animation,
                            language: config.language,
                        })
                })
                .or_else(|| {
                    let effect = selection_effect.as_ref()?;
                    let confirmed_at = effect.confirmed_at?;
                    if confirmed_at.elapsed() <= selection_effect_duration() {
                        Some(effect.clone())
                    } else {
                        selection_effect = None;
                        None
                    }
                });

            if let Some(snapshot) = snapshot {
                native_overlay.show(&snapshot, get_system_dpi_scale());
            } else {
                native_overlay.hide();
            }

            sleep_until_next_frame(frame_start);
        }
    });
}

fn spawn_output_worker(
    mode: OutputMode,
    delay: (u64, u64),
    phrase: String,
    toast_tx: Sender<ToastMessage>,
    lang: Language,
) {
    thread::spawn(move || {
        let toast = run_output_phrase(mode, phrase, delay, lang);
        let duration = toast.until.saturating_duration_since(Instant::now());
        let _ = toast_tx.send(ToastMessage {
            message: toast.message,
            duration,
        });
    });
}

fn sleep_until_next_frame(frame_start: Instant) {
    let elapsed = frame_start.elapsed();
    if elapsed < Duration::from_millis(16) {
        thread::sleep(Duration::from_millis(16) - elapsed);
    }
}

impl CallWheelApp {
    fn new(cc: &CreationContext<'_>) -> Self {
        install_japanese_font(&cc.egui_ctx);

        let config_path = config_path();
        let config = load_config(&config_path);
        let (toast_tx, toast_rx) = channel();
        let shared_config = Arc::new(Mutex::new(config.clone()));
        start_wheel_worker(shared_config.clone(), toast_tx.clone());

        Self {
            config,
            config_path,
            selected_profile: 0,
            selected_key: 0,
            deleted_profiles: Vec::new(),
            toast: None,
            last_save_error: None,
            preview_open: false,
            app_icon_textures: HashMap::new(),
            shared_config,
            toast_rx,
        }
    }

    fn receive_output_toasts(&mut self) {
        while let Ok(toast) = self.toast_rx.try_recv() {
            self.toast = Some(Toast {
                message: toast.message,
                until: Instant::now() + toast.duration,
            });
        }
    }

    fn save(&mut self) {
        if let Some(parent) = self.config_path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                self.last_save_error = Some(error.to_string());
                return;
            }
        }

        match serde_json::to_string_pretty(&self.config)
            .map_err(|error| error.to_string())
            .and_then(|json| fs::write(&self.config_path, json).map_err(|error| error.to_string()))
        {
            Ok(()) => self.last_save_error = None,
            Err(error) => self.last_save_error = Some(error),
        }
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        let lang = self.config.language;
        let full_size = ui.available_size();
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.set_min_size(full_size);
            ui.horizontal(|ui| {
                ui.heading("CallWheel");
                let button_width = 84.0;
                ui.add_space((ui.available_width() - button_width).max(8.0));
                if ui
                    .add_sized([button_width, 28.0], egui::Button::new(t(lang, "保存", "Save")))
                    .clicked()
                {
                    self.save();
                }
            });

            ui.label(t(
                lang,
                "Fキーを長押ししてマウスを方向にスワイプ。離すと選んだ定型文をクリップボードへコピーします。",
                "Hold an F-key and swipe in a direction. Release to copy the selected phrase.",
            ));
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label(t(lang, "長押し判定距離", "Dead Zone"));
                ui.add(egui::Slider::new(&mut self.config.dead_zone, 12.0..=100.0).suffix(" px"));
                ui.label(t(lang, "ホイール半径", "Wheel Radius"));
                ui.add(egui::Slider::new(&mut self.config.wheel_radius, 92.0..=190.0).suffix(" px"));
                ui.label(t(lang, "選択肢サイズ", "Item Size"));
                ui.add(
                    egui::Slider::new(&mut self.config.wheel_item_radius, 34.0..=78.0)
                        .suffix(" px"),
                );
                if ui.button(t(lang, "プレビュー", "Preview")).clicked() {
                    self.preview_open = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label(t(lang, "文字間隔", "Char Delay"));
                ui.add(
                    egui::DragValue::new(&mut self.config.type_delay_min_ms)
                        .range(0..=300)
                        .suffix(t(lang, " ms 最小", " ms min")),
                );
                ui.add(
                    egui::DragValue::new(&mut self.config.type_delay_max_ms)
                        .range(0..=400)
                        .suffix(t(lang, " ms 最大", " ms max")),
                );
                if self.config.type_delay_min_ms > self.config.type_delay_max_ms {
                    self.config.type_delay_max_ms = self.config.type_delay_min_ms;
                }
                ui.label(t(lang, "出力", "Output"));
                egui::ComboBox::from_id_salt("output_mode_combo")
                    .selected_text(self.config.output_mode.label_lang(lang))
                    .show_ui(ui, |ui| {
                        for mode in [
                            OutputMode::Clipboard,
                            OutputMode::TypeText,
                            OutputMode::Both,
                            OutputMode::OpenTypeSend,
                        ] {
                            ui.selectable_value(
                                &mut self.config.output_mode,
                                mode,
                                mode.label_lang(lang),
                            );
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.config.selection_animation,
                    t(lang, "選択アニメーション", "Selection Animation"),
                );
                ui.checkbox(
                    &mut self.config.selection_sound,
                    t(lang, "選択サウンド", "Selection Sound"),
                );
                ui.add_enabled_ui(self.config.selection_sound, |ui| {
                    ui.label(t(lang, "音", "Sound"));
                    egui::ComboBox::from_id_salt("selection_sound_kind_combo")
                        .selected_text(self.config.selection_sound_kind.label_lang(lang))
                        .show_ui(ui, |ui| {
                            for sound in [
                                SelectionSoundKind::Subtle,
                                SelectionSoundKind::Koto,
                                SelectionSoundKind::Soft,
                                SelectionSoundKind::Click,
                                SelectionSoundKind::Confirm,
                                SelectionSoundKind::Alert,
                            ] {
                                ui.selectable_value(
                                    &mut self.config.selection_sound_kind,
                                    sound,
                                    sound.label_lang(lang),
                                );
                            }
                        });
                });
            });

            ui.horizontal(|ui| {
                ui.label(t(lang, "言語", "Language"));
                egui::ComboBox::from_id_salt("language_combo")
                    .selected_text(t(
                        lang,
                        match lang {
                            Language::Japanese => "日本語",
                            Language::English => "English",
                        },
                        match lang {
                            Language::Japanese => "Japanese",
                            Language::English => "English",
                        },
                    ))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.config.language,
                            Language::Japanese,
                            t(lang, "日本語", "Japanese"),
                        );
                        ui.selectable_value(
                            &mut self.config.language,
                            Language::English,
                            t(lang, "English", "English"),
                        );
                    });
            });

            if let Some(error) = &self.last_save_error {
                ui.colored_label(
                    Color32::from_rgb(210, 80, 80),
                    format!(
                        "{}: {error}",
                        t(lang, "保存エラー", "Save error")
                    ),
                );
            }

            ui.separator();

            let editor_height = ui.available_height().max(260.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), editor_height),
                Layout::left_to_right(Align::Min),
                |ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(240.0, editor_height),
                    Layout::top_down(Align::Min),
                    |ui| {
                    ui.heading(t(lang, "プロファイル", "Profiles"));
                    ui.add_space(4.0);
                    let list_height = (ui.available_height() - 42.0).max(180.0);
                    egui::ScrollArea::vertical()
                        .id_salt("profile_selector_scroll")
                        .max_height(list_height)
                        .show(ui, |ui| {
                            for (index, profile) in self.config.profiles.iter().enumerate() {
                                let selected = self.selected_profile == index;
                                if ui.selectable_label(selected, &profile.name).clicked() {
                                    self.selected_profile = index;
                                    self.selected_key = 0;
                                }
                            }
                        });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button(t(lang, "追加", "Add")).clicked() {
                            let next = self.config.profiles.len() + 1;
                            self.config.profiles.push(WheelProfile {
                                name: t(lang, "プロファイル ", "Profile ").to_owned()
                                    + &next.to_string(),
                                target_process_paths: Vec::new(),
                                keys: vec![KeyProfile::default_lane()],
                                hotkey: String::new(),
                                direction_mode: DirectionMode::Eight,
                                phrases: Vec::new(),
                            });
                            self.selected_profile = self.config.profiles.len() - 1;
                            self.selected_key = 0;
                        }

                        let can_restore = !self.deleted_profiles.is_empty();
                        if ui
                            .add_enabled(can_restore, egui::Button::new(t(lang, "復元", "Restore")))
                            .clicked()
                            && let Some(profile) = self.deleted_profiles.pop()
                        {
                            self.config.profiles.push(profile);
                            self.selected_profile = self.config.profiles.len() - 1;
                            self.selected_key = 0;
                        }

                        let can_remove = self.config.profiles.len() > 1;
                        if ui
                            .add_enabled(can_remove, egui::Button::new(t(lang, "削除", "Remove")))
                            .clicked()
                        {
                            let removed = self.config.profiles.remove(self.selected_profile);
                            self.deleted_profiles.push(removed);
                            self.selected_profile = self.selected_profile.saturating_sub(1);
                            self.selected_key = 0;
                        }
                    });
                    },
                );

                ui.separator();

                ui.allocate_ui_with_layout(
                    Vec2::new(240.0, editor_height),
                    Layout::top_down(Align::Min),
                    |ui| {
                    ui.heading(t(lang, "キー", "Keys"));
                    ui.add_space(4.0);
                    if let Some(profile) = self.config.profiles.get_mut(self.selected_profile) {
                        if profile.keys.is_empty() {
                            profile.keys.push(KeyProfile::default_lane());
                        }
                        self.selected_key = self.selected_key.min(profile.keys.len() - 1);

                        let list_height = (ui.available_height() - 42.0).max(180.0);
                        egui::ScrollArea::vertical()
                            .id_salt("key_selector_scroll")
                            .max_height(list_height)
                            .show(ui, |ui| {
                                for (index, key_profile) in profile.keys.iter().enumerate() {
                                    let selected = self.selected_key == index;
                                    let label = format!(
                                        "{}\n{}",
                                        key_profile.name,
                                        if key_profile.hotkey.trim().is_empty() {
                                            "-"
                                        } else {
                                            key_profile.hotkey.as_str()
                                        }
                                    );
                                    if ui.selectable_label(selected, label).clicked() {
                                        self.selected_key = index;
                                    }
                                }
                            });

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button(t(lang, "追加", "Add")).clicked() {
                                let next = profile.keys.len() + 1;
                                profile.keys.push(KeyProfile {
                                    name: t(lang, "キー ", "Key ").to_owned() + &next.to_string(),
                                    hotkey: "F1".to_owned(),
                                    direction_mode: DirectionMode::Eight,
                                    phrases: vec![String::new(); DIRECTIONS_8.len()],
                                });
                                self.selected_key = profile.keys.len() - 1;
                            }

                            let can_remove = profile.keys.len() > 1;
                            if ui
                                .add_enabled(
                                    can_remove,
                                    egui::Button::new(t(lang, "削除", "Remove")),
                                )
                                .clicked()
                            {
                                profile.keys.remove(self.selected_key);
                                self.selected_key = self.selected_key.saturating_sub(1);
                            }
                        });
                    }
                    },
                );

                ui.separator();

                if let Some(profile) = self.config.profiles.get_mut(self.selected_profile) {
                    if profile.keys.is_empty() {
                        profile.keys.push(KeyProfile::default_lane());
                    }
                    self.selected_key = self.selected_key.min(profile.keys.len() - 1);
                    let profile_name = profile.name.clone();
                    let key_name = profile.keys[self.selected_key].name.clone();

                    egui::ScrollArea::vertical()
                        .id_salt("settings_detail_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.heading(format!("{profile_name} / {key_name}"));
                                ui.add_space(6.0);

                                ui.label(t(lang, "プロファイル設定", "Profile Settings"));
                                egui::Grid::new(format!("profile_meta_{}", self.selected_profile))
                                    .num_columns(2)
                                    .spacing([12.0, 8.0])
                                    .show(ui, |ui| {
                                        ui.label(t(lang, "名前", "Name"));
                                        ui.add_sized(
                                            [260.0, 28.0],
                                            egui::TextEdit::singleline(&mut profile.name),
                                        );
                                        ui.end_row();
                                    });

                                ui.add_space(10.0);
                                ui.label(t(lang, "対象アプリ", "Target Apps"));
                                Self::draw_target_apps(
                                    ui,
                                    lang,
                                    self.selected_profile,
                                    profile,
                                    &mut self.app_icon_textures,
                                );

                                ui.add_space(12.0);
                                ui.label(t(lang, "キー設定", "Key Settings"));
                                let key_profile = &mut profile.keys[self.selected_key];
                                egui::Grid::new(format!(
                                    "key_meta_{}_{}",
                                    self.selected_profile, self.selected_key
                                ))
                                .num_columns(2)
                                .spacing([12.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label(t(lang, "名前", "Name"));
                                    ui.add_sized(
                                        [260.0, 28.0],
                                        egui::TextEdit::singleline(&mut key_profile.name),
                                    );
                                    ui.end_row();

                                    ui.label(t(lang, "ホットキー", "Hotkey"));
                                    ui.add_sized(
                                        [200.0, 28.0],
                                        egui::TextEdit::singleline(&mut key_profile.hotkey),
                                    );
                                    if !key_profile.hotkey.is_empty()
                                        && keycode_from_name(&key_profile.hotkey).is_none()
                                    {
                                        ui.colored_label(Color32::from_rgb(210, 80, 80), "?");
                                    }
                                    ui.end_row();

                                    ui.label(t(lang, "選択数", "Directions"));
                                    egui::ComboBox::from_id_salt(format!(
                                        "direction_mode_combo_{}_{}",
                                        self.selected_profile, self.selected_key
                                    ))
                                    .width(260.0)
                                    .selected_text(key_profile.direction_mode.label_lang(lang))
                                    .show_ui(ui, |ui| {
                                        for mode in [
                                            DirectionMode::Four,
                                            DirectionMode::Six,
                                            DirectionMode::Eight,
                                        ] {
                                            ui.selectable_value(
                                                &mut key_profile.direction_mode,
                                                mode,
                                                mode.label_lang(lang),
                                            );
                                        }
                                    });
                                    ui.end_row();
                                });

                                ui.add_space(12.0);
                                ui.label(t(lang, "方向ごとの割り当て", "Direction Assignments"));
                                egui::Grid::new(format!(
                                    "profile_phrases_{}_{}",
                                    self.selected_profile, self.selected_key
                                ))
                                .num_columns(2)
                                .spacing([12.0, 8.0])
                                .show(ui, |ui| {
                                    for direction in key_profile.direction_mode.directions() {
                                        ui.label(format!("{:>2}", direction.label_lang(lang)));
                                        ui.add_sized(
                                            [420.0, 30.0],
                                            egui::TextEdit::singleline(
                                                &mut key_profile.phrases[direction.index()],
                                            ),
                                        );
                                        ui.end_row();
                                    }
                                });
                            });
                        });
                }
                },
            );
        });
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some(toast) = &self.toast else {
            return;
        };

        if Instant::now() > toast.until {
            self.toast = None;
            return;
        }

        egui::Area::new(Id::new("toast"))
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-18.0, -18.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(&toast.message);
                });
            });
    }

    fn draw_preview_viewport(&mut self, ctx: &egui::Context) {
        if !self.preview_open {
            return;
        }

        let lang = self.config.language;
        let title = t(lang, "ホイールプレビュー", "Wheel Preview");
        let viewport_id = egui::ViewportId::from_hash_of("callwheel_preview_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([720.0, 720.0])
            .with_min_inner_size([420.0, 420.0]);

        let mut open = true;
        ctx.show_viewport_immediate(viewport_id, builder, |ui, _class| {
            if ui.input(|input| input.viewport().close_requested()) {
                open = false;
                return;
            }

            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t(lang, "ホイール半径", "Wheel Radius"));
                    ui.add(
                        egui::Slider::new(&mut self.config.wheel_radius, 92.0..=190.0)
                            .suffix(" px"),
                    );
                    ui.label(t(lang, "選択肢サイズ", "Item Size"));
                    ui.add(
                        egui::Slider::new(&mut self.config.wheel_item_radius, 34.0..=78.0)
                            .suffix(" px"),
                    );
                });

                ui.add_space(8.0);

                if let Some(snapshot) = self.preview_snapshot() {
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| draw_egui_overlay_preview(ui, &snapshot));
                }
            });
        });

        self.preview_open = open;
    }

    fn preview_snapshot(&self) -> Option<OverlaySnapshot> {
        let profile = self.config.profiles.get(self.selected_profile)?;
        let key_profile = profile.keys.get(self.selected_key)?;
        Some(OverlaySnapshot {
            profile_name: key_profile.name.clone(),
            phrases: key_profile.phrases.clone(),
            origin: Pos2::ZERO,
            selected: Some(
                profile
                    .keys
                    .get(self.selected_key)?
                    .direction_mode
                    .directions()
                    .get(1)
                    .copied()
                    .unwrap_or(Direction::Up),
            ),
            wheel_radius: self.config.wheel_radius,
            wheel_item_radius: self.config.wheel_item_radius,
            direction_mode: key_profile.direction_mode,
            confirmed_at: None,
            selection_animation: self.config.selection_animation,
            language: self.config.language,
        })
    }

    fn draw_target_apps(
        ui: &mut egui::Ui,
        lang: Language,
        selected_profile: usize,
        profile: &mut WheelProfile,
        app_icon_textures: &mut HashMap<String, egui::TextureHandle>,
    ) {
        if profile.target_process_paths.is_empty() {
            ui.label(t(
                lang,
                "未指定の場合は、どのアプリでもこのプロファイルが有効です。",
                "When empty, this profile is active in every app.",
            ));
        }

        let mut remove_index = None;
        for index in 0..profile.target_process_paths.len() {
            ui.horizontal(|ui| {
                let path = profile.target_process_paths[index].clone();
                draw_app_icon(ui, app_icon_textures, &path);
                ui.add_sized(
                    [360.0, 28.0],
                    egui::TextEdit::singleline(&mut profile.target_process_paths[index])
                        .hint_text(t(lang, "実行ファイルのパス", "Executable path")),
                );
                if ui.button(t(lang, "参照", "Browse")).clicked()
                    && let Some(path) = pick_executable_path()
                {
                    profile.target_process_paths[index] = path;
                }
                if ui.button(t(lang, "削除", "Remove")).clicked() {
                    remove_index = Some(index);
                }
            });
        }

        if let Some(index) = remove_index {
            profile.target_process_paths.remove(index);
        }

        ui.horizontal(|ui| {
            if ui.button(t(lang, "アプリを追加", "Add App")).clicked() {
                if let Some(path) = pick_executable_path() {
                    profile.target_process_paths.push(path);
                } else {
                    profile.target_process_paths.push(String::new());
                }
            }
            if ui
                .button(t(lang, "入力欄を追加", "Add Field"))
                .on_hover_text(t(
                    lang,
                    "手入力したい場合に空の入力欄を追加します。",
                    "Adds an empty field for manual entry.",
                ))
                .clicked()
            {
                profile.target_process_paths.push(String::new());
            }
            ui.small(format!(
                "{} {}",
                t(lang, "設定中:", "Targets:"),
                profile
                    .target_process_paths
                    .iter()
                    .filter(|path| !path.trim().is_empty())
                    .count()
            ));
        });

        ui.add_space(2.0);
        ui.small(format!(
            "{} #{}",
            t(lang, "編集中のプロファイル", "Editing profile"),
            selected_profile + 1
        ));
    }
}

impl eframe::App for CallWheelApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(mut shared_config) = self.shared_config.lock() {
            *shared_config = self.config.clone();
        }
        self.receive_output_toasts();
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw_settings(ui);
        let ctx = ui.ctx();
        hide_overlay_window();
        self.draw_preview_viewport(ctx);
        self.draw_toast(ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn selection_effect_duration() -> Duration {
    Duration::from_millis(SELECTION_EFFECT_MS)
}

fn draw_egui_overlay_preview(ui: &mut egui::Ui, snapshot: &OverlaySnapshot) {
    let outer_radius = snapshot.wheel_radius + overlay_outer_padding(snapshot.wheel_item_radius);
    let extent_radius = snapshot.wheel_radius + overlay_extent_padding(snapshot.wheel_item_radius);
    let size = Vec2::splat(extent_radius * 2.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let wheel_radius = snapshot.wheel_radius;
    let inner_radius = 42.0;

    painter.circle_filled(center, outer_radius, Color32::from_rgb(5, 12, 20));
    painter.circle_stroke(
        center,
        outer_radius,
        egui::Stroke::new(3.0, Color32::from_rgb(116, 86, 38)),
    );
    painter.circle_stroke(
        center,
        outer_radius - 7.0,
        egui::Stroke::new(1.0, Color32::from_rgb(180, 146, 74)),
    );
    painter.circle_stroke(
        center,
        wheel_radius - 10.0,
        egui::Stroke::new(1.0, Color32::from_rgb(55, 170, 180)),
    );
    painter.circle_filled(center, inner_radius, Color32::from_rgb(3, 9, 16));
    painter.circle_stroke(
        center,
        inner_radius,
        egui::Stroke::new(2.0, Color32::from_rgb(200, 158, 74)),
    );
    painter.text(
        center,
        Align2::CENTER_CENTER,
        &snapshot.profile_name,
        egui::FontId::proportional(15.0),
        Color32::WHITE,
    );

    let directions = snapshot.direction_mode.directions();
    for pass in 0..2u8 {
        for &direction in directions {
            let is_selected = snapshot.selected == Some(direction);
            if (pass == 0 && is_selected) || (pass == 1 && !is_selected) {
                continue;
            }

            let angle = direction_angle_in_mode(snapshot.direction_mode, direction).to_radians();
            let unit = Vec2::new(angle.cos(), -angle.sin());
            let item_pos = center + unit * wheel_radius;
            let node_radius = if is_selected {
                snapshot.wheel_item_radius + 5.0
            } else {
                snapshot.wheel_item_radius
            };

            painter.line_segment(
                [center + unit * inner_radius, item_pos],
                egui::Stroke::new(1.0, Color32::from_rgb(150, 118, 55)),
            );

            if is_selected && snapshot.selection_animation {
                painter.circle_filled(item_pos, node_radius + 7.0, Color32::from_rgb(26, 196, 205));
            }

            let bg = if is_selected {
                Color32::from_rgb(8, 78, 91)
            } else {
                Color32::from_rgb(12, 25, 34)
            };
            let fg = if is_selected {
                Color32::from_rgb(245, 250, 255)
            } else {
                Color32::from_rgb(216, 226, 226)
            };
            let border = if is_selected {
                Color32::from_rgb(200, 158, 74)
            } else {
                Color32::from_rgb(116, 86, 38)
            };

            painter.circle_filled(item_pos, node_radius, bg);
            painter.circle_stroke(
                item_pos,
                node_radius,
                egui::Stroke::new(if is_selected { 3.0 } else { 2.0 }, border),
            );
            painter.circle_stroke(
                item_pos,
                node_radius - 6.0,
                egui::Stroke::new(1.0, Color32::from_rgb(185, 165, 105)),
            );
            painter.text(
                item_pos,
                Align2::CENTER_CENTER,
                compact_phrase(phrase_for_direction(&snapshot.phrases, direction)),
                egui::FontId::proportional(14.0),
                fg,
            );
        }
    }
}

fn selection_effect_progress(confirmed_at: Instant) -> f32 {
    (confirmed_at.elapsed().as_secs_f32() / selection_effect_duration().as_secs_f32())
        .clamp(0.0, 1.0)
}

fn overlay_outer_padding(item_radius: f32) -> f32 {
    item_radius + 13.0
}

fn overlay_extent_padding(item_radius: f32) -> f32 {
    overlay_outer_padding(item_radius) + 33.0
}

#[cfg(target_os = "windows")]
fn ensure_koto_sound_file() -> Option<PathBuf> {
    let path = std::env::temp_dir().join("callwheel_koto.wav");
    if path.exists() {
        return Some(path);
    }

    fs::write(&path, make_koto_wav()).ok()?;
    Some(path)
}

#[cfg(target_os = "windows")]
fn make_koto_wav() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 44_100;
    const DURATION_SECONDS: f32 = 0.15;
    let sample_count = (SAMPLE_RATE as f32 * DURATION_SECONDS) as usize;
    let mut samples = Vec::with_capacity(sample_count * 2);

    for i in 0..sample_count {
        let t = i as f32 / SAMPLE_RATE as f32;
        let attack = (t / 0.008).min(1.0);
        let decay = (-t * 24.0).exp();
        let thump_decay = (-t * 48.0).exp();
        let click_decay = (-t * 85.0).exp();

        let wood = (2.0 * std::f32::consts::PI * 220.0 * t).sin() * 0.48
            + (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.28
            + (2.0 * std::f32::consts::PI * 720.0 * t).sin() * 0.12;
        let thump = (2.0 * std::f32::consts::PI * 128.0 * t).sin() * 0.18;
        let click = (2.0 * std::f32::consts::PI * 1320.0 * t).sin() * 0.05;
        let value = (wood * decay + thump * thump_decay + click * click_decay) * attack * 0.55;
        let sample = (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        samples.extend_from_slice(&sample.to_le_bytes());
    }

    let data_len = samples.len() as u32;
    let mut wav = Vec::with_capacity(44 + samples.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&samples);
    wav
}

fn play_select_sound(kind: SelectionSoundKind) {
    #[cfg(target_os = "windows")]
    {
        thread::spawn(move || unsafe {
            unsafe extern "system" {
                fn Beep(dwFreq: u32, dwDuration: u32) -> i32;
            }
            match kind {
                SelectionSoundKind::Koto => {
                    if let Some(path) = ensure_koto_sound_file() {
                        let wide = wide_null(&path.to_string_lossy());
                        let _ = PlaySoundW(PCWSTR(wide.as_ptr()), None, SND_FILENAME | SND_ASYNC);
                    } else {
                        Beep(440, 16);
                    }
                }
                SelectionSoundKind::Subtle => {
                    Beep(440, 16);
                }
                SelectionSoundKind::Soft => {
                    Beep(520, 24);
                }
                SelectionSoundKind::Click => {
                    Beep(660, 35);
                }
                SelectionSoundKind::Confirm => {
                    Beep(660, 28);
                    thread::sleep(Duration::from_millis(18));
                    Beep(880, 42);
                }
                SelectionSoundKind::Alert => {
                    Beep(880, 34);
                    thread::sleep(Duration::from_millis(20));
                    Beep(740, 54);
                }
            }
        });
    }

    #[cfg(not(target_os = "windows"))]
    let _ = kind;
}

fn direction_angle_in_mode(mode: DirectionMode, direction: Direction) -> f32 {
    let directions = mode.directions();
    let Some(index) = directions
        .iter()
        .position(|candidate| *candidate == direction)
    else {
        return 90.0;
    };

    // Start from "up" and place each active direction at equal intervals clockwise.
    let step = 360.0 / directions.len() as f32;
    (90.0 - (index as f32 * step)).rem_euclid(360.0)
}

fn angle_distance(a: f32, b: f32) -> f32 {
    let diff = (a - b).abs() % 360.0;
    diff.min(360.0 - diff)
}

fn phrase_for_direction(phrases: &[String], direction: Direction) -> &str {
    phrases
        .get(direction.index())
        .map(String::as_str)
        .unwrap_or("")
}

fn mouse_cancel_pressed(buttons: &[bool]) -> bool {
    buttons.get(1).copied().unwrap_or(false)
        || buttons.get(2).copied().unwrap_or(false)
        || windows_right_mouse_pressed()
}

fn profile_matches_process_path(profile: &WheelProfile, foreground_process_path: &str) -> bool {
    if !profile_has_process_targets(profile) {
        return true;
    }

    let foreground = normalize_process_path(foreground_process_path);
    !foreground.is_empty()
        && profile
            .target_process_paths
            .iter()
            .map(|path| normalize_process_path(path))
            .any(|path| !path.is_empty() && path == foreground)
}

fn profile_has_process_targets(profile: &WheelProfile) -> bool {
    profile
        .target_process_paths
        .iter()
        .any(|path| !path.trim().is_empty())
}

fn normalize_process_path(path: &str) -> String {
    path.trim()
        .trim_matches('"')
        .replace('/', "\\")
        .to_lowercase()
}

#[cfg(target_os = "windows")]
fn foreground_process_path() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == HWND::default() {
            return String::new();
        }

        let mut process_id = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id as *mut u32));
        if process_id == 0 {
            return String::new();
        }

        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) else {
            return String::new();
        };

        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);

        if result.is_err() || size == 0 {
            return String::new();
        }

        String::from_utf16_lossy(&buffer[..size as usize])
    }
}

#[cfg(not(target_os = "windows"))]
fn foreground_process_path() -> String {
    String::new()
}

fn pick_executable_path() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Executable", &["exe"])
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}

fn draw_app_icon(
    ui: &mut egui::Ui,
    app_icon_textures: &mut HashMap<String, egui::TextureHandle>,
    path: &str,
) {
    let key = normalize_process_path(path);
    let texture = app_icon_textures.entry(key.clone()).or_insert_with(|| {
        let image = load_app_icon_image(path).unwrap_or_else(|| fallback_app_icon_image(path));
        ui.ctx().load_texture(
            format!("app_icon:{key}"),
            image,
            egui::TextureOptions::LINEAR,
        )
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

#[cfg(target_os = "windows")]
fn load_app_icon_image(path: &str) -> Option<egui::ColorImage> {
    if path.trim().is_empty() {
        return None;
    }

    unsafe {
        let wide = wide_null(path.trim().trim_matches('"'));
        let mut large_icon = HICON::default();
        let count = ExtractIconExW(
            PCWSTR(wide.as_ptr()),
            0,
            Some(&mut large_icon as *mut HICON),
            None,
            1,
        );
        if count == 0 {
            return None;
        }

        let image = icon_to_color_image(large_icon);
        let _ = DestroyIcon(large_icon);
        image
    }
}

#[cfg(not(target_os = "windows"))]
fn load_app_icon_image(_path: &str) -> Option<egui::ColorImage> {
    None
}

#[cfg(target_os = "windows")]
unsafe fn icon_to_color_image(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
) -> Option<egui::ColorImage> {
    const SIZE: i32 = 32;
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }

        let hdc = CreateCompatibleDC(Some(screen));
        let _ = ReleaseDC(None, screen);
        if hdc.is_invalid() {
            return None;
        }

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let Ok(bitmap) = CreateDIBSection(Some(hdc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = windows::Win32::Graphics::Gdi::DeleteDC(hdc);
            return None;
        };

        if bitmap.is_invalid() || bits.is_null() {
            let _ = DeleteObject(bitmap.into());
            let _ = windows::Win32::Graphics::Gdi::DeleteDC(hdc);
            return None;
        }

        let old = windows::Win32::Graphics::Gdi::SelectObject(hdc, bitmap.into());
        let drawn = DrawIconEx(hdc, 0, 0, icon, SIZE, SIZE, 0, None, DI_NORMAL);
        let bytes = std::slice::from_raw_parts(bits as *const u8, (SIZE * SIZE * 4) as usize);
        let mut rgba = Vec::with_capacity(bytes.len());
        for bgra in bytes.chunks_exact(4) {
            rgba.extend_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
        }

        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = windows::Win32::Graphics::Gdi::DeleteDC(hdc);

        if drawn.is_err() {
            return None;
        }

        Some(egui::ColorImage::from_rgba_unmultiplied(
            [SIZE as usize, SIZE as usize],
            &rgba,
        ))
    }
}

fn fallback_app_icon_image(path: &str) -> egui::ColorImage {
    const SIZE: usize = 32;
    let seed = path.bytes().fold(0u8, |acc, byte| acc.wrapping_add(byte));
    let accent = Color32::from_rgb(
        70u8.wrapping_add(seed / 3),
        110u8.wrapping_add(seed / 5),
        150u8.wrapping_add(seed / 7),
    );
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = (y * SIZE + x) * 4;
            let border = x < 3 || y < 3 || x >= SIZE - 3 || y >= SIZE - 3;
            let color = if border {
                Color32::from_rgb(220, 224, 228)
            } else {
                accent
            };
            rgba[i..i + 4].copy_from_slice(&color.to_array());
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([SIZE, SIZE], &rgba)
}

fn run_output_phrase(mode: OutputMode, phrase: String, delay: (u64, u64), lang: Language) -> Toast {
    match mode {
        OutputMode::Clipboard => copy_phrase_to_clipboard(&phrase, lang),
        OutputMode::TypeText => type_phrase_with_delay(&phrase, delay, lang),
        OutputMode::Both => {
            let _ = copy_phrase_to_clipboard(&phrase, lang);
            type_phrase_with_delay(&phrase, delay, lang)
        }
        OutputMode::OpenTypeSend => open_type_send_phrase_with_delay(&phrase, delay, lang),
    }
}

fn copy_phrase_to_clipboard(phrase: &str, lang: Language) -> Toast {
    let t_ok = t(lang, "コピーしました", "Copied");
    let t_err = t(lang, "コピー失敗", "Copy failed");
    match Clipboard::new().and_then(|mut clipboard| clipboard.set_text(phrase.to_owned())) {
        Ok(()) => Toast {
            message: format!("{t_ok}: {phrase}"),
            until: Instant::now() + Duration::from_secs(2),
        },
        Err(error) => Toast {
            message: format!("{t_err}: {error}"),
            until: Instant::now() + Duration::from_secs(3),
        },
    }
}

fn type_phrase_with_delay(phrase: &str, delay: (u64, u64), lang: Language) -> Toast {
    let t_ok = t(lang, "入力しました", "Typed");
    let t_err = t(lang, "入力失敗", "Type failed");
    match send_unicode_text(phrase, delay) {
        Ok(()) => Toast {
            message: format!("{t_ok}: {phrase}"),
            until: Instant::now() + Duration::from_secs(2),
        },
        Err(error) => Toast {
            message: format!("{t_err}: {error}"),
            until: Instant::now() + Duration::from_secs(3),
        },
    }
}

fn open_type_send_phrase_with_delay(phrase: &str, delay: (u64, u64), lang: Language) -> Toast {
    let t_ok = t(lang, "送信しました", "Sent");
    let t_err = t(lang, "送信失敗", "Send failed");
    match send_chat_message(phrase, delay) {
        Ok(()) => Toast {
            message: format!("{t_ok}: {phrase}"),
            until: Instant::now() + Duration::from_secs(2),
        },
        Err(error) => Toast {
            message: format!("{t_err}: {error}"),
            until: Instant::now() + Duration::from_secs(3),
        },
    }
}

#[cfg(target_os = "windows")]
fn hide_overlay_window() {
    if let Some(hwnd) = find_window_by_title(OVERLAY_TITLE) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn hide_overlay_window() {}

#[cfg(target_os = "windows")]
fn windows_right_mouse_pressed() -> bool {
    unsafe { GetAsyncKeyState(VK_RBUTTON.0 as i32) as u32 & 0x8000 != 0 }
}

#[cfg(not(target_os = "windows"))]
fn windows_right_mouse_pressed() -> bool {
    false
}

fn typing_delay_range(config: &AppConfig) -> (u64, u64) {
    (
        config.type_delay_min_ms.min(config.type_delay_max_ms),
        config.type_delay_min_ms.max(config.type_delay_max_ms),
    )
}

#[cfg(target_os = "windows")]
fn send_unicode_text(text: &str, delay_ms: (u64, u64)) -> Result<(), String> {
    send_unicode_text_with_delay(text, delay_ms)
}

#[cfg(target_os = "windows")]
fn send_unicode_text_with_delay(text: &str, delay_ms: (u64, u64)) -> Result<(), String> {
    let mut rng = rand::rng();
    for unit in text.encode_utf16() {
        send_inputs(&[
            unicode_key_input(unit, false),
            unicode_key_input(unit, true),
        ])?;
        sleep_random_ms(&mut rng, delay_ms);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn send_chat_message(text: &str, delay_ms: (u64, u64)) -> Result<(), String> {
    send_inputs(&[
        scancode_key_input(ENTER_SCANCODE, false),
        scancode_key_input(ENTER_SCANCODE, true),
    ])?;
    sleep_random_range((65, 115));

    send_unicode_text_with_delay(text, delay_ms)?;
    sleep_random_range((35, 75));

    send_inputs(&[
        scancode_key_input(ENTER_SCANCODE, false),
        scancode_key_input(ENTER_SCANCODE, true),
    ])
}

#[cfg(target_os = "windows")]
fn sleep_random_range(delay_ms: (u64, u64)) {
    let mut rng = rand::rng();
    sleep_random_ms(&mut rng, delay_ms);
}

#[cfg(target_os = "windows")]
fn sleep_random_ms(rng: &mut impl Rng, delay_ms: (u64, u64)) {
    let min = delay_ms.0.min(delay_ms.1);
    let max = delay_ms.0.max(delay_ms.1);
    let delay = if min == max {
        min
    } else {
        rng.random_range(min..=max)
    };

    if delay > 0 {
        thread::sleep(Duration::from_millis(delay));
    }
}

#[cfg(target_os = "windows")]
fn send_inputs(inputs: &[INPUT]) -> Result<(), String> {
    if inputs.is_empty() {
        return Ok(());
    }

    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(format!("SendInput sent {sent}/{}", inputs.len()))
    }
}

#[cfg(target_os = "windows")]
fn unicode_key_input(unit: u16, key_up: bool) -> INPUT {
    let flags = if key_up {
        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_UNICODE
    };

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: Default::default(),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(target_os = "windows")]
fn scancode_key_input(scan: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: Default::default(),
                wScan: scan,
                dwFlags: if key_up {
                    KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_SCANCODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(not(target_os = "windows"))]
fn send_unicode_text(_text: &str, _delay_ms: (u64, u64)) -> Result<(), String> {
    Err("SendInput is only available on Windows".to_owned())
}

#[cfg(not(target_os = "windows"))]
fn send_chat_message(_text: &str, _delay_ms: (u64, u64)) -> Result<(), String> {
    Err("SendInput is only available on Windows".to_owned())
}

#[cfg(target_os = "windows")]
fn draw_native_overlay(
    hdc: HDC,
    snapshot: &OverlaySnapshot,
    width: i32,
    height: i32,
    pixels_per_point: f32,
    fonts: NativeOverlayFonts,
) {
    unsafe {
        let rect = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        let bg_brush = CreateSolidBrush(rgb(5, 12, 20));
        FillRect(hdc, &rect, bg_brush);
        let _ = DeleteObject(bg_brush.into());
        let _ = SetBkMode(hdc, TRANSPARENT);

        let center = Vec2::new(width as f32 / 2.0, height as f32 / 2.0);
        let wheel_radius = snapshot.wheel_radius * pixels_per_point;
        let item_radius = snapshot.wheel_item_radius * pixels_per_point;
        let selected_item_radius = (snapshot.wheel_item_radius + 5.0) * pixels_per_point;
        let outer_radius = (snapshot.wheel_radius
            + overlay_outer_padding(snapshot.wheel_item_radius))
            * pixels_per_point;
        let inner_radius = 42.0 * pixels_per_point;
        let effect_progress = snapshot.confirmed_at.map(selection_effect_progress);

        draw_native_circle(
            hdc,
            center,
            outer_radius,
            rgb(5, 12, 20),
            rgb(116, 86, 38),
            3,
        );
        draw_native_circle_outline(
            hdc,
            center,
            outer_radius - 7.0 * pixels_per_point,
            rgb(180, 146, 74),
            1,
        );
        draw_native_circle_outline(
            hdc,
            center,
            wheel_radius - 10.0 * pixels_per_point,
            rgb(55, 170, 180),
            1,
        );
        draw_native_circle(
            hdc,
            center,
            inner_radius,
            rgb(3, 9, 16),
            rgb(200, 158, 74),
            2,
        );

        let center_label: &str = if snapshot.confirmed_at.is_some() {
            t(snapshot.language, "選択", "Selected")
        } else {
            &snapshot.profile_name
        };
        draw_native_text_center(
            hdc,
            center_label,
            center,
            96.0 * pixels_per_point,
            30.0 * pixels_per_point,
            fonts.center,
            rgb(255, 255, 255),
        );

        // Draw non-selected items first, then selected item on top
        let directions = snapshot.direction_mode.directions();
        for pass in 0..2u8 {
            for &direction in directions {
                let is_selected = snapshot.selected == Some(direction);
                // Pass 0: non-selected only, Pass 1: selected only
                if (pass == 0 && is_selected) || (pass == 1 && !is_selected) {
                    continue;
                }

                let angle =
                    direction_angle_in_mode(snapshot.direction_mode, direction).to_radians();
                let item_pos = center + Vec2::new(angle.cos(), -angle.sin()) * wheel_radius;
                let node_radius = if is_selected {
                    selected_item_radius
                } else {
                    item_radius
                };
                let bg = if is_selected {
                    if snapshot.confirmed_at.is_some() {
                        rgb(38, 130, 118)
                    } else {
                        rgb(8, 78, 91)
                    }
                } else {
                    rgb(12, 25, 34)
                };
                let fg = if is_selected {
                    rgb(245, 250, 255)
                } else {
                    rgb(216, 226, 226)
                };
                let border = if is_selected {
                    rgb(200, 158, 74)
                } else {
                    rgb(116, 86, 38)
                };

                let line_start = center + Vec2::new(angle.cos(), -angle.sin()) * inner_radius;
                draw_native_line(hdc, line_start, item_pos, rgb(150, 118, 55), 1);
                if is_selected && snapshot.selection_animation {
                    draw_native_circle_fill(
                        hdc,
                        item_pos,
                        node_radius + 7.0 * pixels_per_point,
                        rgb(26, 196, 205),
                    );
                    if let Some(progress) = effect_progress {
                        let ring_radius = node_radius
                            + 10.0 * pixels_per_point
                            + progress * 16.0 * pixels_per_point;
                        draw_native_circle_fill(
                            hdc,
                            item_pos,
                            node_radius
                                + 12.0 * pixels_per_point
                                + progress * 8.0 * pixels_per_point,
                            rgb(24, 92, 92),
                        );
                        draw_native_circle_outline(
                            hdc,
                            item_pos,
                            ring_radius,
                            rgb(248, 224, 150),
                            3,
                        );
                        draw_native_circle_outline(
                            hdc,
                            item_pos,
                            ring_radius + 8.0 * pixels_per_point,
                            rgb(90, 236, 220),
                            2,
                        );
                    }
                }

                draw_native_circle(
                    hdc,
                    item_pos,
                    node_radius,
                    bg,
                    border,
                    if is_selected { 3 } else { 2 },
                );
                draw_native_circle_outline(
                    hdc,
                    item_pos,
                    node_radius - 6.0 * pixels_per_point,
                    rgb(185, 165, 105),
                    1,
                );
                draw_native_text_center(
                    hdc,
                    &compact_phrase(phrase_for_direction(&snapshot.phrases, direction)),
                    item_pos,
                    node_radius * 1.75,
                    24.0 * pixels_per_point,
                    fonts.phrase,
                    fg,
                );
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn draw_native_circle(
    hdc: HDC,
    center: Vec2,
    radius: f32,
    fill: COLORREF,
    stroke: COLORREF,
    stroke_width: i32,
) {
    unsafe {
        let brush = CreateSolidBrush(fill);
        let pen = CreatePen(PS_SOLID, stroke_width, stroke);
        let old_brush = windows::Win32::Graphics::Gdi::SelectObject(hdc, brush.into());
        let old_pen = windows::Win32::Graphics::Gdi::SelectObject(hdc, pen.into());
        let _ = Ellipse(
            hdc,
            (center.x - radius).round() as i32,
            (center.y - radius).round() as i32,
            (center.x + radius).round() as i32,
            (center.y + radius).round() as i32,
        );
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_brush);
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_pen);
        let _ = DeleteObject(brush.into());
        let _ = DeleteObject(pen.into());
    }
}

#[cfg(target_os = "windows")]
fn draw_native_circle_fill(hdc: HDC, center: Vec2, radius: f32, fill: COLORREF) {
    draw_native_circle(hdc, center, radius, fill, fill, 1);
}

#[cfg(target_os = "windows")]
fn draw_native_circle_outline(
    hdc: HDC,
    center: Vec2,
    radius: f32,
    color: COLORREF,
    stroke_width: i32,
) {
    unsafe {
        let hollow_brush = windows::Win32::Graphics::Gdi::GetStockObject(
            windows::Win32::Graphics::Gdi::HOLLOW_BRUSH,
        );
        let pen = CreatePen(PS_SOLID, stroke_width, color);
        let old_brush = windows::Win32::Graphics::Gdi::SelectObject(hdc, hollow_brush);
        let old_pen = windows::Win32::Graphics::Gdi::SelectObject(hdc, pen.into());
        let _ = Ellipse(
            hdc,
            (center.x - radius).round() as i32,
            (center.y - radius).round() as i32,
            (center.x + radius).round() as i32,
            (center.y + radius).round() as i32,
        );
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_brush);
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}

#[cfg(target_os = "windows")]
fn draw_native_line(hdc: HDC, start: Vec2, end: Vec2, color: COLORREF, width: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, width, color);
        let old_pen = windows::Win32::Graphics::Gdi::SelectObject(hdc, pen.into());
        let _ = MoveToEx(hdc, start.x.round() as i32, start.y.round() as i32, None);
        let _ = LineTo(hdc, end.x.round() as i32, end.y.round() as i32);
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}

#[cfg(target_os = "windows")]
fn draw_native_text_center(
    hdc: HDC,
    text: &str,
    center: Vec2,
    width: f32,
    height: f32,
    font: HFONT,
    color: COLORREF,
) {
    if text.is_empty() {
        return;
    }

    unsafe {
        let old_font = windows::Win32::Graphics::Gdi::SelectObject(hdc, font.into());
        let _ = SetTextColor(hdc, color);
        let mut text = text.encode_utf16().collect::<Vec<_>>();
        let mut rect = RECT {
            left: (center.x - width / 2.0).round() as i32,
            top: (center.y - height / 2.0).round() as i32,
            right: (center.x + width / 2.0).round() as i32,
            bottom: (center.y + height / 2.0).round() as i32,
        };
        let _ = DrawTextW(
            hdc,
            &mut text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_font);
    }
}

#[cfg(target_os = "windows")]
fn create_native_font(font_size: f32, bold: bool) -> HFONT {
    let face = wide_null("Yu Gothic UI");
    unsafe {
        CreateFontW(
            -(font_size.round() as i32),
            0,
            0,
            0,
            if bold { FW_BOLD.0 } else { FW_NORMAL.0 } as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

#[cfg(target_os = "windows")]
fn delete_native_fonts(fonts: NativeOverlayFonts) {
    unsafe {
        let _ = DeleteObject(fonts.center.into());
        let _ = DeleteObject(fonts.direction.into());
        let _ = DeleteObject(fonts.phrase.into());
    }
}

#[cfg(target_os = "windows")]
fn create_native_backbuffer(width: i32, height: i32) -> Option<NativeBackbuffer> {
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }

        let hdc = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let _ = ReleaseDC(None, screen);

        if hdc.is_invalid() || bitmap.is_invalid() {
            if !hdc.is_invalid() {
                let _ = windows::Win32::Graphics::Gdi::DeleteDC(hdc);
            }
            if !bitmap.is_invalid() {
                let _ = DeleteObject(bitmap.into());
            }
            return None;
        }

        let old_bitmap = windows::Win32::Graphics::Gdi::SelectObject(hdc, bitmap.into());
        Some(NativeBackbuffer {
            hdc,
            bitmap,
            old_bitmap,
            width,
            height,
        })
    }
}

#[cfg(target_os = "windows")]
fn flush_native_backbuffer(hwnd: HWND, buffer: &NativeBackbuffer, dst_width: i32, dst_height: i32) {
    unsafe {
        let target = GetDC(Some(hwnd));
        if target.is_invalid() {
            return;
        }

        SetStretchBltMode(target, HALFTONE);
        let _ = StretchBlt(
            target,
            0,
            0,
            dst_width,
            dst_height,
            Some(buffer.hdc),
            0,
            0,
            buffer.width,
            buffer.height,
            SRCCOPY,
        );
        let _ = ReleaseDC(Some(hwnd), target);
    }
}

#[cfg(target_os = "windows")]
fn delete_native_backbuffer(buffer: NativeBackbuffer) {
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::SelectObject(buffer.hdc, buffer.old_bitmap);
        let _ = DeleteObject(buffer.bitmap.into());
        let _ = windows::Win32::Graphics::Gdi::DeleteDC(buffer.hdc);
    }
}

#[cfg(target_os = "windows")]
fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

#[cfg(target_os = "windows")]
fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn apply_round_window_region(
    hwnd: HWND,
    size: Vec2,
    margin: f32,
    alpha: u8,
    pixels_per_point: f32,
) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);

        let left = (margin * pixels_per_point).floor() as i32;
        let top = (margin * pixels_per_point).floor() as i32;
        let right = ((size.x - margin) * pixels_per_point).ceil() as i32 + 2;
        let bottom = ((size.y - margin) * pixels_per_point).ceil() as i32 + 2;
        let region = CreateEllipticRgn(left, top, right, bottom);
        if !region.is_invalid() {
            let _ = SetWindowRgn(hwnd, Some(region), true);
        }
    }
}

#[cfg(target_os = "windows")]
fn find_window_by_title(title: &str) -> Option<HWND> {
    let mut title: Vec<u16> = title.encode_utf16().collect();
    title.push(0);

    unsafe {
        let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) else {
            return None;
        };
        if hwnd == HWND::default() {
            None
        } else {
            Some(hwnd)
        }
    }
}

#[cfg(target_os = "windows")]
fn get_system_dpi_scale() -> f32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

#[cfg(not(target_os = "windows"))]
fn get_system_dpi_scale() -> f32 {
    1.0
}

#[cfg(target_os = "windows")]
fn set_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
    }
}

#[cfg(not(target_os = "windows"))]
fn set_dpi_awareness() {}

fn compact_phrase(phrase: &str) -> String {
    const LIMIT: usize = 12;
    let mut chars = phrase.chars();
    let short: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_some() {
        format!("{short}...")
    } else {
        short
    }
}

fn keycode_from_name(name: &str) -> Option<Keycode> {
    match name.to_ascii_uppercase().as_str() {
        "0" => Some(Keycode::Key0),
        "1" => Some(Keycode::Key1),
        "2" => Some(Keycode::Key2),
        "3" => Some(Keycode::Key3),
        "4" => Some(Keycode::Key4),
        "5" => Some(Keycode::Key5),
        "6" => Some(Keycode::Key6),
        "7" => Some(Keycode::Key7),
        "8" => Some(Keycode::Key8),
        "9" => Some(Keycode::Key9),
        "A" => Some(Keycode::A),
        "B" => Some(Keycode::B),
        "C" => Some(Keycode::C),
        "D" => Some(Keycode::D),
        "E" => Some(Keycode::E),
        "F" => Some(Keycode::F),
        "G" => Some(Keycode::G),
        "H" => Some(Keycode::H),
        "I" => Some(Keycode::I),
        "J" => Some(Keycode::J),
        "K" => Some(Keycode::K),
        "L" => Some(Keycode::L),
        "M" => Some(Keycode::M),
        "N" => Some(Keycode::N),
        "O" => Some(Keycode::O),
        "P" => Some(Keycode::P),
        "Q" => Some(Keycode::Q),
        "R" => Some(Keycode::R),
        "S" => Some(Keycode::S),
        "T" => Some(Keycode::T),
        "U" => Some(Keycode::U),
        "V" => Some(Keycode::V),
        "W" => Some(Keycode::W),
        "X" => Some(Keycode::X),
        "Y" => Some(Keycode::Y),
        "Z" => Some(Keycode::Z),
        "F1" => Some(Keycode::F1),
        "F2" => Some(Keycode::F2),
        "F3" => Some(Keycode::F3),
        "F4" => Some(Keycode::F4),
        "F5" => Some(Keycode::F5),
        "F6" => Some(Keycode::F6),
        "F7" => Some(Keycode::F7),
        "F8" => Some(Keycode::F8),
        "F9" => Some(Keycode::F9),
        "F10" => Some(Keycode::F10),
        "F11" => Some(Keycode::F11),
        "F12" => Some(Keycode::F12),
        "SPACE" => Some(Keycode::Space),
        "ENTER" | "RETURN" => Some(Keycode::Enter),
        "BACKSPACE" | "BS" => Some(Keycode::Backspace),
        "TAB" => Some(Keycode::Tab),
        "ESCAPE" | "ESC" => Some(Keycode::Escape),
        "UP" => Some(Keycode::Up),
        "DOWN" => Some(Keycode::Down),
        "LEFT" => Some(Keycode::Left),
        "RIGHT" => Some(Keycode::Right),
        "SHIFT" | "LSHIFT" => Some(Keycode::LShift),
        "RSHIFT" => Some(Keycode::RShift),
        "CONTROL" | "CTRL" | "LCONTROL" | "LCTRL" => Some(Keycode::LControl),
        "RCONTROL" | "RCTRL" => Some(Keycode::RControl),
        "ALT" | "LALT" => Some(Keycode::LAlt),
        "RALT" => Some(Keycode::RAlt),
        _ => None,
    }
}

fn config_path() -> PathBuf {
    ProjectDirs::from("dev", "CallWheel", "CallWheel")
        .map(|dirs| dirs.config_dir().join("settings.json"))
        .unwrap_or_else(|| PathBuf::from("settings.json"))
}

fn load_config(path: &PathBuf) -> AppConfig {
    let mut config = fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default();
    normalize_config(&mut config);
    config
}

fn normalize_config(config: &mut AppConfig) {
    config.wheel_item_radius = config.wheel_item_radius.clamp(34.0, 78.0);
    config.wheel_radius = config.wheel_radius.clamp(92.0, 190.0);
    let legacy_direction_mode = config.legacy_direction_mode.take();
    for profile in &mut config.profiles {
        profile
            .target_process_paths
            .retain(|path| !path.trim().is_empty());
        for path in &mut profile.target_process_paths {
            *path = path.trim().trim_matches('"').to_owned();
        }
        profile
            .target_process_paths
            .sort_by_key(|path| normalize_process_path(path));
        profile
            .target_process_paths
            .dedup_by(|a, b| normalize_process_path(a) == normalize_process_path(b));

        if profile.keys.is_empty() {
            let mut key_profile = KeyProfile {
                name: profile.name.clone(),
                hotkey: profile.hotkey.clone(),
                direction_mode: profile.direction_mode,
                phrases: std::mem::take(&mut profile.phrases),
            };
            if key_profile.hotkey.is_empty() {
                key_profile.hotkey = "F1".to_owned();
            }
            if key_profile.phrases.is_empty() {
                key_profile.phrases = vec![String::new(); DIRECTIONS_8.len()];
            }
            profile.keys.push(key_profile);
        }

        if let Some(direction_mode) = legacy_direction_mode {
            for key_profile in &mut profile.keys {
                key_profile.direction_mode = direction_mode;
            }
        }
        for key_profile in &mut profile.keys {
            normalize_phrases(&mut key_profile.phrases);
        }
    }
}

fn normalize_phrases(phrases: &mut Vec<String>) {
    if phrases.len() == 6 {
        let old = std::mem::take(phrases);
        *phrases = vec![
            old[0].clone(),
            old[1].clone(),
            String::new(),
            old[2].clone(),
            old[3].clone(),
            old[4].clone(),
            String::new(),
            old[5].clone(),
        ];
        return;
    }

    phrases.resize_with(DIRECTIONS_8.len(), String::new);
    phrases.truncate(DIRECTIONS_8.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_keeps_vertical_swipes_stable() {
        assert_eq!(
            DirectionMode::Eight.from_delta(Vec2::new(24.0, -80.0), 10.0),
            Some(Direction::Up)
        );
        assert_eq!(
            DirectionMode::Eight.from_delta(Vec2::new(-24.0, 80.0), 10.0),
            Some(Direction::Down)
        );
    }

    #[test]
    fn direction_selects_diagonals_when_sideways_motion_is_clear() {
        assert_eq!(
            DirectionMode::Eight.from_delta(Vec2::new(90.0, -70.0), 10.0),
            Some(Direction::UpRight)
        );
        assert_eq!(
            DirectionMode::Eight.from_delta(Vec2::new(-90.0, -70.0), 10.0),
            Some(Direction::UpLeft)
        );
    }

    #[test]
    fn direction_selects_horizontal_swipes() {
        assert_eq!(
            DirectionMode::Eight.from_delta(Vec2::new(90.0, 8.0), 10.0),
            Some(Direction::Right)
        );
        assert_eq!(
            DirectionMode::Eight.from_delta(Vec2::new(-90.0, -8.0), 10.0),
            Some(Direction::Left)
        );
    }

    #[test]
    fn direction_modes_limit_available_directions() {
        assert_eq!(
            DirectionMode::Four.from_delta(Vec2::new(90.0, -70.0), 10.0),
            Some(Direction::Right)
        );
        assert_eq!(
            DirectionMode::Six.from_delta(Vec2::new(90.0, 0.0), 10.0),
            Some(Direction::UpRight)
        );
        assert_eq!(
            DirectionMode::Eight.from_delta(Vec2::new(90.0, 0.0), 10.0),
            Some(Direction::Right)
        );
    }

    #[test]
    fn six_direction_mode_uses_even_angle_spacing() {
        assert!((direction_angle_in_mode(DirectionMode::Six, Direction::Up) - 90.0).abs() < 0.1);
        assert!(
            (direction_angle_in_mode(DirectionMode::Six, Direction::UpRight) - 30.0).abs() < 0.1
        );
        assert!(
            (direction_angle_in_mode(DirectionMode::Six, Direction::DownRight) - 330.0).abs() < 0.1
        );
        assert!(
            (direction_angle_in_mode(DirectionMode::Six, Direction::DownLeft) - 210.0).abs() < 0.1
        );
    }

    #[test]
    fn mouse_cancel_accepts_left_and_right_buttons() {
        assert!(mouse_cancel_pressed(&[false, true, false]));
        assert!(mouse_cancel_pressed(&[false, false, true]));
    }

    #[test]
    fn six_direction_profiles_migrate_to_eight_without_shifting_diagonals() {
        let mut phrases = ["上", "右上", "右下", "下", "左下", "左上"]
            .map(str::to_owned)
            .to_vec();

        normalize_phrases(&mut phrases);

        assert_eq!(phrases[Direction::Up.index()], "上");
        assert_eq!(phrases[Direction::UpRight.index()], "右上");
        assert_eq!(phrases[Direction::Right.index()], "");
        assert_eq!(phrases[Direction::DownRight.index()], "右下");
        assert_eq!(phrases[Direction::Down.index()], "下");
        assert_eq!(phrases[Direction::DownLeft.index()], "左下");
        assert_eq!(phrases[Direction::Left.index()], "");
        assert_eq!(phrases[Direction::UpLeft.index()], "左上");
    }

    #[test]
    fn phrase_lookup_treats_missing_entries_as_empty() {
        let phrases = vec!["上".to_owned()];

        assert_eq!(phrase_for_direction(&phrases, Direction::Up), "上");
        assert_eq!(phrase_for_direction(&phrases, Direction::Right), "");
    }

    #[test]
    fn legacy_global_direction_mode_moves_to_profiles() {
        let mut config = AppConfig {
            legacy_direction_mode: Some(DirectionMode::Four),
            ..Default::default()
        };

        normalize_config(&mut config);

        assert!(config.legacy_direction_mode.is_none());
        assert!(
            config
                .profiles
                .iter()
                .flat_map(|profile| profile.keys.iter())
                .all(|key_profile| key_profile.direction_mode == DirectionMode::Four)
        );
    }

    #[test]
    fn blank_target_apps_keep_profile_global() {
        let profile = WheelProfile::default_game();

        assert!(profile_matches_process_path(&profile, ""));
        assert!(profile_matches_process_path(
            &profile,
            "C:\\Games\\Game.exe"
        ));
    }

    #[test]
    fn target_app_matches_by_normalized_full_path() {
        let mut profile = WheelProfile::default_game();
        profile
            .target_process_paths
            .push("C:\\Games\\League\\League.exe".to_owned());

        assert!(profile_matches_process_path(
            &profile,
            "c:/games/league/LEAGUE.exe"
        ));
        assert!(!profile_matches_process_path(
            &profile,
            "C:\\Games\\Valorant\\VALORANT.exe"
        ));
    }

    #[test]
    fn process_target_detects_non_blank_values() {
        let mut profile = WheelProfile::default_game();
        assert!(!profile_has_process_targets(&profile));

        profile
            .target_process_paths
            .push("  C:\\Game.exe  ".to_owned());
        assert!(profile_has_process_targets(&profile));
    }
}

fn install_japanese_font(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\NotoSansJP-VF.ttf",
        r"C:\Windows\Fonts\NotoSansCJKjp-Regular.otf",
    ];

    let Some(font_bytes) = candidates.iter().find_map(|path| fs::read(path).ok()) else {
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "japanese".to_owned(),
        Arc::new(FontData::from_owned(font_bytes)),
    );

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "japanese".to_owned());
    }

    ctx.set_fonts(fonts);
}

const APP_ICON_SIZE: usize = 64;

fn app_icon() -> egui::IconData {
    let source = image::load_from_memory(include_bytes!("../assets/app_icon.png"))
        .expect("load embedded app icon")
        .into_rgba8();
    let resized = image::imageops::resize(
        &source,
        APP_ICON_SIZE as u32,
        APP_ICON_SIZE as u32,
        image::imageops::FilterType::Lanczos3,
    );

    egui::IconData {
        rgba: resized.into_raw(),
        width: APP_ICON_SIZE as u32,
        height: APP_ICON_SIZE as u32,
    }
}

fn main() -> eframe::Result {
    set_dpi_awareness();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CallWheel")
            .with_inner_size([760.0, 520.0])
            .with_min_inner_size([680.0, 460.0])
            .with_icon(app_icon()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "CallWheel",
        options,
        Box::new(|cc| Ok(Box::new(CallWheelApp::new(cc)))),
    )
}
