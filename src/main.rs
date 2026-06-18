#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
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
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap,
            CreateCompatibleDC, CreateEllipticRgn, CreateFontW, CreatePen, CreateSolidBrush,
            DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER,
            DeleteObject, DrawTextW, Ellipse, FW_BOLD, FW_NORMAL, FillRect, GetDC, HALFTONE, HDC,
            HFONT, LineTo, MoveToEx, OUT_DEFAULT_PRECIS, PS_SOLID, ReleaseDC, SetBkMode,
            SetStretchBltMode, SetTextColor, SetWindowRgn, SRCCOPY, StretchBlt, TRANSPARENT,
        },
        UI::{
            HiDpi::{GetDpiForSystem, SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE},
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
                KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, SendInput, VK_RBUTTON,
            },
            WindowsAndMessaging::{
                CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
                FindWindowW, GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, LWA_ALPHA,
                RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW,
                SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, WNDCLASSW,
                WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::PCWSTR,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WheelProfile {
    name: String,
    hotkey: String,
    #[serde(default)]
    direction_mode: DirectionMode,
    phrases: Vec<String>,
}

impl WheelProfile {
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
#[serde(default)]
struct AppConfig {
    profiles: Vec<WheelProfile>,
    dead_zone: f32,
    wheel_radius: f32,
    #[serde(default, rename = "direction_mode", skip_serializing)]
    legacy_direction_mode: Option<DirectionMode>,
    output_mode: OutputMode,
    type_delay_min_ms: u64,
    type_delay_max_ms: u64,
    selection_animation: bool,
    #[serde(default = "default_true")]
    selection_sound: bool,
    #[serde(default)]
    language: Language,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            profiles: vec![WheelProfile::default_lane(), WheelProfile::default_macro()],
            dead_zone: 34.0,
            wheel_radius: 132.0,
            legacy_direction_mode: None,
            output_mode: OutputMode::Clipboard,
            type_delay_min_ms: 35,
            type_delay_max_ms: 85,
            selection_animation: true,
            selection_sound: true,
            language: Language::default(),
        }
    }
}

struct ActiveWheel {
    profile_index: usize,
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
    toast: Option<Toast>,
    last_save_error: Option<String>,
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

        let size_points = Vec2::splat(snapshot.wheel_radius * 2.0 + 190.0);
        let size_px = (size_points * pixels_per_point).round();
        let left = (snapshot.origin.x - size_px.x / 2.0).round() as i32;
        let top = (snapshot.origin.y - size_px.y / 2.0).round() as i32;
        let width = size_px.x.max(1.0).round() as i32;
        let height = size_px.y.max(1.0).round() as i32;
        let outer_radius = (snapshot.wheel_radius + 62.0) * pixels_per_point;
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
                        .map(|profile| profile.direction_mode)
                        .unwrap_or_default()
                        .from_delta(mouse_pos - current.origin, config.dead_zone);
                } else {
                    let profile_index = current.profile_index;
                    let selected = current.selected;
                    let origin = current.origin;
                    active = None;

                    if let Some(direction) = selected
                        && let Some(profile) = config.profiles.get(profile_index)
                    {
                        let phrases = profile.phrases.clone();
                        let phrase = phrase_for_direction(&phrases, direction).to_owned();
                        selection_effect = if config.selection_animation {
                            Some(OverlaySnapshot {
                                profile_name: profile.name.clone(),
                                phrases: phrases.clone(),
                                origin,
                                selected: Some(direction),
                                wheel_radius: config.wheel_radius,
                                direction_mode: profile.direction_mode,
                                confirmed_at: Some(Instant::now()),
                                selection_animation: true,
                                language: config.language,
                            })
                            } else {
                                None
                            };

                            if config.selection_sound {
                                play_select_sound();
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
                for (index, profile) in config.profiles.iter().enumerate() {
                    if let Some(key) = keycode_from_name(&profile.hotkey)
                        && keys.contains(&key)
                    {
                        active = Some(ActiveWheel {
                            profile_index: index,
                            key,
                            origin: mouse_pos,
                            selected: None,
                        });
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
                        .map(|profile| OverlaySnapshot {
                            profile_name: profile.name.clone(),
                            phrases: profile.phrases.clone(),
                            origin: current.origin,
                            selected: current.selected,
                            wheel_radius: config.wheel_radius,
                            direction_mode: profile.direction_mode,
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
            toast: None,
            last_save_error: None,
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
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("CallWheel");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(t(lang, "保存", "Save")).clicked() {
                        self.save();
                    }
                });
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

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(180.0);
                    for (index, profile) in self.config.profiles.iter().enumerate() {
                        let selected = self.selected_profile == index;
                        if ui
                            .selectable_label(selected, format!("{} ({})", profile.name, profile.hotkey))
                            .clicked()
                        {
                            self.selected_profile = index;
                        }
                    }

                    ui.add_space(8.0);
                    if ui.button(t(lang, "セットを追加", "Add Set")).clicked() {
                        let next = self.config.profiles.len() + 1;
                        self.config.profiles.push(WheelProfile {
                            name: t(lang, "セット ", "Set ").to_owned() + &next.to_string(),
                            hotkey: "F1".to_owned(),
                            direction_mode: DirectionMode::Eight,
                            phrases: vec![String::new(); DIRECTIONS_8.len()],
                        });
                        self.selected_profile = self.config.profiles.len() - 1;
                    }

                    let can_remove = self.config.profiles.len() > 1;
                    if ui
                        .add_enabled(
                            can_remove,
                            egui::Button::new(t(lang, "選択セットを削除", "Remove Selected Set")),
                        )
                        .clicked()
                    {
                        self.config.profiles.remove(self.selected_profile);
                        self.selected_profile = self.selected_profile.saturating_sub(1);
                    }
                });

                ui.separator();

                if let Some(profile) = self.config.profiles.get_mut(self.selected_profile) {
                    ui.vertical(|ui| {
                        egui::Grid::new(format!("profile_meta_{}", self.selected_profile))
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label(t(lang, "名前", "Name"));
                                ui.add_sized([260.0, 28.0], egui::TextEdit::singleline(&mut profile.name));
                                ui.end_row();

                                ui.label(t(lang, "ホットキー", "Hotkey"));
                                ui.add_sized([200.0, 28.0], egui::TextEdit::singleline(&mut profile.hotkey));
                                if !profile.hotkey.is_empty() && keycode_from_name(&profile.hotkey).is_none() {
                                    ui.colored_label(Color32::from_rgb(210, 80, 80), "?");
                                }
                                ui.end_row();

                                ui.label(t(lang, "選択数", "Directions"));
                                egui::ComboBox::from_id_salt(format!(
                                    "direction_mode_combo_{}",
                                    self.selected_profile
                                ))
                                .width(260.0)
                                .selected_text(profile.direction_mode.label_lang(lang))
                                .show_ui(ui, |ui| {
                                    for mode in [
                                        DirectionMode::Four,
                                        DirectionMode::Six,
                                        DirectionMode::Eight,
                                    ] {
                                        ui.selectable_value(
                                            &mut profile.direction_mode,
                                            mode,
                                            mode.label_lang(lang),
                                        );
                                    }
                                });
                                ui.end_row();
                            });

                        ui.add_space(8.0);
                        egui::Grid::new(format!("profile_phrases_{}", self.selected_profile))
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                for direction in profile.direction_mode.directions() {
                                    ui.label(format!("{:>2}", direction.label_lang(lang)));
                                    ui.add_sized(
                                        [420.0, 30.0],
                                        egui::TextEdit::singleline(
                                            &mut profile.phrases[direction.index()],
                                        ),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
                }
            });
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
}

impl eframe::App for CallWheelApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
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
        self.draw_toast(ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn selection_effect_duration() -> Duration {
    Duration::from_millis(SELECTION_EFFECT_MS)
}

fn selection_effect_progress(confirmed_at: Instant) -> f32 {
    (confirmed_at.elapsed().as_secs_f32() / selection_effect_duration().as_secs_f32())
        .clamp(0.0, 1.0)
}

fn play_select_sound() {
    #[cfg(target_os = "windows")]
    {
        thread::spawn(|| {
            unsafe {
                unsafe extern "system" {
                    fn Beep(dwFreq: u32, dwDuration: u32) -> i32;
                }
                Beep(660, 35);
            }
        });
    }
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
        let outer_radius = (snapshot.wheel_radius + 62.0) * pixels_per_point;
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

                let angle = direction_angle_in_mode(snapshot.direction_mode, direction).to_radians();
                let item_pos = center + Vec2::new(angle.cos(), -angle.sin()) * wheel_radius;
                let node_radius = (if is_selected { 54.0 } else { 49.0 }) * pixels_per_point;
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
                        let ring_radius =
                            node_radius + 10.0 * pixels_per_point + progress * 16.0 * pixels_per_point;
                        draw_native_circle_fill(
                            hdc,
                            item_pos,
                            node_radius + 12.0 * pixels_per_point + progress * 8.0 * pixels_per_point,
                            rgb(24, 92, 92),
                        );
                        draw_native_circle_outline(hdc, item_pos, ring_radius, rgb(248, 224, 150), 3);
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
    let legacy_direction_mode = config.legacy_direction_mode.take();
    for profile in &mut config.profiles {
        if let Some(direction_mode) = legacy_direction_mode {
            profile.direction_mode = direction_mode;
        }
        normalize_phrases(&mut profile.phrases);
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
                .all(|profile| profile.direction_mode == DirectionMode::Four)
        );
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
    let mut rgba = Vec::with_capacity(APP_ICON_SIZE * APP_ICON_SIZE * 4);
    let size = APP_ICON_SIZE as f32;
    for y in 0..APP_ICON_SIZE {
        for x in 0..APP_ICON_SIZE {
            let (r, g, b, a) = app_icon_pixel(x as f32, y as f32, size);
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    egui::IconData {
        rgba,
        width: APP_ICON_SIZE as u32,
        height: APP_ICON_SIZE as u32,
    }
}

fn app_icon_pixel(x: f32, y: f32, size: f32) -> (u8, u8, u8, u8) {
    let center = (size - 1.0) * 0.5;
    let dx = x - center;
    let dy = y - center;
    let dist = (dx * dx + dy * dy).sqrt();

    let outer = size * 0.49;
    let ring_outer = size * 0.43;
    let ring_inner = size * 0.31;
    let core = size * 0.20;

    let alpha = smooth_fill(outer - dist, size * 0.018);
    if alpha <= 0.001 {
        return (0, 0, 0, 0);
    }

    let mut r = 8.0;
    let mut g = 16.0;
    let mut b = 28.0;

    let ring_t = smooth_step(ring_outer, ring_inner, dist);
    r = mix(12.0, r, ring_t);
    g = mix(148.0, g, ring_t);
    b = mix(208.0, b, ring_t);

    let edge = smooth_band(
        dist,
        outer - size * 0.032,
        outer - size * 0.005,
        size * 0.01,
    );
    r = mix(r, 86.0, edge);
    g = mix(g, 236.0, edge);
    b = mix(b, 245.0, edge);

    let core_t = smooth_fill(core - dist, size * 0.015);
    r = mix(r, 5.0, core_t);
    g = mix(g, 10.0, core_t);
    b = mix(b, 20.0, core_t);

    let line_width = size * 0.07;
    let slash = (dx + dy * 0.85).abs();
    let slash_t = smooth_fill(line_width - slash, size * 0.015);
    r = mix(r, 250.0, slash_t);
    g = mix(g, 250.0, slash_t);
    b = mix(b, 255.0, slash_t);

    let tip_x = center + size * 0.20;
    let tip_y = center - size * 0.22;
    let tdx = x - tip_x;
    let tdy = y - tip_y;
    let tip_d = (tdx * tdx + tdy * tdy).sqrt();
    let tip_t = smooth_fill(size * 0.09 - tip_d, size * 0.02);
    r = mix(r, 120.0, tip_t);
    g = mix(g, 245.0, tip_t);
    b = mix(b, 255.0, tip_t);

    (
        r.round() as u8,
        g.round() as u8,
        b.round() as u8,
        (alpha * 255.0).round() as u8,
    )
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn smooth_fill(value: f32, feather: f32) -> f32 {
    if feather <= 0.0 {
        return if value >= 0.0 { 1.0 } else { 0.0 };
    }
    ((value / feather) * 0.5 + 0.5).clamp(0.0, 1.0)
}

fn smooth_step(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smooth_band(x: f32, min: f32, max: f32, feather: f32) -> f32 {
    let enter = smooth_fill(x - min, feather);
    let leave = 1.0 - smooth_fill(x - max, feather);
    (enter * leave).clamp(0.0, 1.0)
}

fn main() -> eframe::Result {
    set_dpi_awareness();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CallWheel")
            .with_inner_size([760.0, 520.0])
            .with_min_inner_size([680.0, 460.0])
            .with_icon(app_icon())
            .with_transparent(true),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "CallWheel",
        options,
        Box::new(|cc| Ok(Box::new(CallWheelApp::new(cc)))),
    )
}
