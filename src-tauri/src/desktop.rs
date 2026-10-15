//! Desktop integration: system tray, close-to-tray, launch at login.

use crate::config::{AppSettings, CloseAction};
use crate::servers::ServerRegistry;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent, Wry};

const TRAY_ID: &str = "main";
/// Passed by the autostart entry so we can tell a login launch from a manual one.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Name of the login entry on macOS: the LaunchAgent's file and `Label`, reverse DNS like the
/// bundle identifier. Windows (Run key) and Linux (.desktop file) keep the product name.
#[cfg(target_os = "macos")]
const LAUNCH_AGENT_LABEL: &str = "com.harnessstudio.app";

/// The "start at login" plugin (a LaunchAgent on macOS), launching with [`MINIMIZED_ARG`].
pub fn autostart_plugin() -> tauri::plugin::TauriPlugin<Wry> {
    let builder = tauri_plugin_autostart::Builder::new().arg(MINIMIZED_ARG);
    // The launcher and app name setters exist only in the plugin's macOS build.
    #[cfg(target_os = "macos")]
    let builder = builder
        .macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent)
        .app_name(LAUNCH_AGENT_LABEL);
    builder.build()
}

/// Versions before the reverse-DNS label wrote `~/Library/LaunchAgents/Harness Studio.plist`:
/// it is removed and, since it was on, the entry is written again under the new label (two
/// entries would start the app twice).
///
/// Also brings an entry written by an earlier version up to date ([`ensure_launch_agent_bundle_id`]).
#[cfg(target_os = "macos")]
pub fn migrate_launch_agent(app: &AppHandle) {
    move_legacy_launch_agent(app);
    ensure_launch_agent_bundle_id(app);
}

#[cfg(target_os = "macos")]
fn move_legacy_launch_agent(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let Some(dir) = launch_agents_dir() else {
        return;
    };
    let legacy = dir.join(format!("{}.plist", app.package_info().name));
    if !legacy.exists() {
        return;
    }
    // Rewriting it from a translocated copy would point the entry at a vanishing path.
    let translocated = std::env::current_exe()
        .is_ok_and(|exe| crate::integrations::cli_install::is_translocated(&exe));
    if translocated {
        return;
    }
    if let Err(e) = std::fs::remove_file(&legacy) {
        eprintln!("[Harness Studio] Old login entry not removed: {}", e);
        return;
    }
    if let Err(e) = app.autolaunch().enable() {
        eprintln!("[Harness Studio] Login entry not moved: {}", e);
    }
}

#[cfg(target_os = "macos")]
fn launch_agents_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join("Library/LaunchAgents"))
}

/// The LaunchAgent the plugin writes names only the executable, so Login Items / "Background
/// Activity" lists it as `harness-studio` with a generic icon. `AssociatedBundleIdentifiers`
/// attributes it to the app bundle (name and icon). Call after every `enable()`; a missing entry
/// or one that already has the key is left alone.
#[cfg(target_os = "macos")]
pub fn ensure_launch_agent_bundle_id(app: &AppHandle) {
    let Some(dir) = launch_agents_dir() else {
        return;
    };
    let path = dir.join(format!("{}.plist", LAUNCH_AGENT_LABEL));
    let Ok(current) = std::fs::read_to_string(&path) else {
        return;
    };
    let Some(updated) = with_associated_bundle_id(&current, &app.config().identifier) else {
        return;
    };
    if let Err(e) = std::fs::write(&path, updated) {
        eprintln!("[Harness Studio] Login entry not linked to the app: {}", e);
    }
}

/// Adds `AssociatedBundleIdentifiers` = [`bundle_id`] to an XML property list, before the
/// top-level dict closes. `None` when nothing should change: the key is already there, or the
/// text is not an XML plist with a dict (a binary plist is never rewritten as text).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn with_associated_bundle_id(plist: &str, bundle_id: &str) -> Option<String> {
    const KEY: &str = "<key>AssociatedBundleIdentifiers</key>";
    if bundle_id.is_empty() || plist.contains(KEY) || !plist.contains("<plist") {
        return None;
    }
    let close = plist.rfind("</dict>")?;
    if !plist[close..].contains("</plist>") {
        return None;
    }
    // Insert at the start of the closing line so the indentation of `</dict>` is kept.
    let line_start = plist[..close].rfind('\n').map_or(close, |i| i + 1);
    let at = if plist[line_start..close].trim().is_empty() {
        line_start
    } else {
        close
    };
    let id = bundle_id
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let entry = format!("  {KEY}\n  <array>\n    <string>{id}</string>\n  </array>\n");
    let mut out = String::with_capacity(plist.len() + entry.len());
    out.push_str(&plist[..at]);
    out.push_str(&entry);
    out.push_str(&plist[at..]);
    Some(out)
}

struct TrayItems {
    show: MenuItem<Wry>,
    stop_all: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

struct Labels {
    show: &'static str,
    stop_all: &'static str,
    quit: &'static str,
    idle: &'static str,
    running: &'static str,
}

fn labels(language: &str) -> Labels {
    if language == "en" {
        Labels {
            show: "Show Harness Studio",
            stop_all: "Stop All Servers",
            quit: "Quit",
            idle: "no server running",
            running: "server(s) running",
        }
    } else {
        Labels {
            show: "Harness Studio'yu Göster",
            stop_all: "Tüm Sunucuları Durdur",
            quit: "Çıkış",
            idle: "çalışan sunucu yok",
            running: "sunucu çalışıyor",
        }
    }
}

pub fn launched_minimized() -> bool {
    std::env::args().any(|a| a == MINIMIZED_ARG)
}

/// The tray icon exists. Without it (e.g. a Linux desktop with no tray host) the window is never
/// hidden: closing quits and "start minimized" opens the window, or the app would be unreachable.
static TRAY_READY: AtomicBool = AtomicBool::new(false);

pub fn tray_available() -> bool {
    TRAY_READY.load(Ordering::Relaxed) && tray_host_present()
}

/// Linux: something shows AppIndicator icons, i.e. a StatusNotifierWatcher is on the session
/// bus (KDE, Cinnamon, XFCE, or GNOME with the AppIndicator extension). Stock GNOME has none:
/// the icon is created but never shown, so the window must not hide into it. Asked each time,
/// since the extension can be turned on while the app runs.
#[cfg(target_os = "linux")]
fn tray_host_present() -> bool {
    use gio::prelude::*;
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&("org.kde.StatusNotifierWatcher",).to_variant()),
        gio::glib::VariantTy::new("(b)").ok(),
        gio::DBusCallFlags::NONE,
        1000,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|v| v.get::<(bool,)>())
    .is_some_and(|(owned,)| owned)
}

/// Creates the tray. On Linux the StatusNotifierItem tray (ksni) registers only if a watcher is on
/// the bus when it is created, and a failed attempt still registers on its own once one appears
/// (an icon the app has no handle to). So without a watcher (started at login before the shell's
/// tray extension, or the extension turned on later) the tray is created when one appears. The
/// callback runs on the main thread's GLib loop.
pub fn setup_tray_now_or_later(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    if !tray_host_present() {
        let app = app.clone();
        let _watch = gio::bus_watch_name(
            gio::BusType::Session,
            "org.kde.StatusNotifierWatcher",
            gio::BusNameWatcherFlags::NONE,
            move |_, _, _| {
                if !TRAY_READY.load(Ordering::Relaxed) {
                    if let Err(e) = setup_tray(&app) {
                        eprintln!("[Harness Studio] Tray unavailable: {}", e);
                    }
                }
            },
            |_, _| {},
        );
        return;
    }
    if let Err(e) = setup_tray(app) {
        eprintln!("[Harness Studio] Tray unavailable: {}", e);
    }
}

#[cfg(not(target_os = "linux"))]
fn tray_host_present() -> bool {
    true
}

/// A close question is on screen. A second close while it is open quits (and a frontend that
/// cannot answer never keeps the window from closing).
static CLOSE_PROMPT_OPEN: AtomicBool = AtomicBool::new(false);

/// Last visibility sent to the frontend (`window-visible`). WebView2 does not tell the page when
/// the window is minimized or hidden, so background polling would otherwise keep running.
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(true);

fn report_visibility<M: Manager<Wry> + Emitter<Wry>>(target: &M, visible: bool) {
    if WINDOW_VISIBLE.swap(visible, Ordering::Relaxed) != visible {
        let _ = target.emit("window-visible", visible);
        if let Some(window) = target_window(target) {
            set_memory_target(&window, !visible);
        }
    }
}

/// The app started hidden in the tray ("start minimized"): the window was never shown, so
/// nothing reports it hidden, and [`release_window`] would keep the page for good.
pub fn started_hidden(app: &AppHandle) {
    report_visibility(app, false);
}

fn target_window<M: Manager<Wry>>(target: &M) -> Option<tauri::WebviewWindow> {
    target.app_handle().get_webview_window("main")
}

/// The main window can be seen (not minimized, not hidden to the tray, not released).
pub fn window_visible() -> bool {
    WINDOW_VISIBLE.load(Ordering::Relaxed)
}

/// The window's web view was closed while hidden to the tray ([`release_window`]); the next
/// "show" builds it again. While set, closing the last window does not end the app.
static WINDOW_RELEASED: AtomicBool = AtomicBool::new(false);

pub fn window_released() -> bool {
    WINDOW_RELEASED.load(Ordering::SeqCst)
}

/// Size, position and maximized state of the released window, restored when it is rebuilt.
struct SavedPlacement {
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    maximized: bool,
    /// Windows: the exact normal (not maximized) frame rectangle. `set_size` on this undecorated
    /// window sizes the frame as if it had a title bar, so every release and rebuild made the
    /// window about 30 px taller until its bottom ran under the taskbar.
    #[cfg(windows)]
    normal_rect: Option<windows::Win32::Foundation::RECT>,
}

/// The window's normal (restored) frame rectangle, also while it is maximized or hidden.
#[cfg(windows)]
fn normal_rect(window: &tauri::WebviewWindow) -> Option<windows::Win32::Foundation::RECT> {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
    let hwnd = windows::Win32::Foundation::HWND(window.hwnd().ok()?.0);
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(hwnd, &mut placement) }.ok()?;
    Some(placement.rcNormalPosition)
}

/// Puts a (hidden) window back on its saved normal rectangle without showing it.
#[cfg(windows)]
fn set_normal_rect(window: &tauri::WebviewWindow, rect: windows::Win32::Foundation::RECT) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPlacement, SW_HIDE, WINDOWPLACEMENT};
    let Ok(handle) = window.hwnd() else {
        return false;
    };
    let placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        showCmd: SW_HIDE.0 as u32,
        rcNormalPosition: rect,
        ..Default::default()
    };
    unsafe { SetWindowPlacement(windows::Win32::Foundation::HWND(handle.0), &placement) }.is_ok()
}

static SAVED_PLACEMENT: Mutex<Option<SavedPlacement>> = Mutex::new(None);

/// The rebuilt window goes back to maximized when it is shown (maximizing a hidden window would
/// show it on Windows, and a window built in the background must stay hidden).
static PENDING_MAXIMIZE: AtomicBool = AtomicBool::new(false);

/// A window built in the background on tray hover / press ([`prewarm_main_window`]) that is not
/// shown within this time is released again. Every further hover event extends it.
const PREWARM_KEEP: Duration = Duration::from_secs(20);

/// Deadline of the window built in the background, `None` when there is none (or it was shown).
static PREWARM_DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
/// Bumped by every prewarm and every show, so an older release timer never acts.
static PREWARM_GEN: AtomicU64 = AtomicU64::new(0);

/// Browser process of the released page (Windows). WebView2 attaches a new web view with the same
/// profile to a browser process that is still running; while that one is shutting down (seconds
/// after the release on a busy machine) the new page never loads and the window is lost. The
/// rebuild waits for it ([`old_browser_gone`]).
#[cfg(windows)]
static RELEASED_BROWSER_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(windows)]
fn remember_browser_process(window: &tauri::WebviewWindow) {
    let _ = window.with_webview(|webview| {
        // SAFETY: plain COM call on the web view's own thread (with_webview runs there).
        unsafe {
            if let Ok(core) = webview.controller().CoreWebView2() {
                let mut pid = 0u32;
                if core.BrowserProcessId(&mut pid).is_ok() {
                    RELEASED_BROWSER_PID.store(pid, Ordering::SeqCst);
                }
            }
        }
    });
}

#[cfg(not(windows))]
fn remember_browser_process(_window: &tauri::WebviewWindow) {}

/// Lets the page record from the microphone (Playground > Speech > Record), and nothing else.
/// Windows: WebView2 would show its own prompt; the user already pressed Record, so the
/// microphone request is granted (the Windows privacy switch still decides) and other kinds
/// keep the default handling. Linux: WebKitGTK ships with media streams off, so they are
/// switched on and the audio-only request is allowed. macOS: WKWebView shows the system prompt
/// itself (`NSMicrophoneUsageDescription` in `Info.plist`). Called for every built main window.
#[cfg(windows)]
pub fn allow_microphone(window: &tauri::WebviewWindow) {
    let _ = window.with_webview(|webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
            COREWEBVIEW2_PERMISSION_STATE_ALLOW,
        };
        use webview2_com::PermissionRequestedEventHandler;
        // SAFETY: plain COM calls on the web view's own thread (with_webview runs there).
        unsafe {
            let Ok(core) = webview.controller().CoreWebView2() else {
                return;
            };
            let handler = PermissionRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                if kind == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                }
                Ok(())
            }));
            let mut token = Default::default();
            let _ = core.add_PermissionRequested(&handler, &mut token);
        }
    });
}

#[cfg(target_os = "linux")]
pub fn allow_microphone(window: &tauri::WebviewWindow) {
    let _ = window.with_webview(|webview| {
        use webkit2gtk::glib::Cast;
        use webkit2gtk::{
            PermissionRequestExt, SettingsExt, UserMediaPermissionRequest,
            UserMediaPermissionRequestExt, WebViewExt,
        };
        let view = webview.inner();
        if let Some(settings) = WebViewExt::settings(&view) {
            settings.set_enable_media_stream(true);
        }
        view.connect_permission_request(|_, request| {
            match request.downcast_ref::<UserMediaPermissionRequest>() {
                Some(media) if media.is_for_audio_device() && !media.is_for_video_device() => {
                    request.allow();
                    true
                }
                _ => false,
            }
        });
    });
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn allow_microphone(_window: &tauri::WebviewWindow) {}

/// Waits up to `timeout` for the released page's browser process to exit. `true` when it is
/// gone (or unknown), `false` when it is still running after `timeout`.
#[cfg(windows)]
fn old_browser_gone(timeout: Duration) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    let pid = RELEASED_BROWSER_PID.load(Ordering::SeqCst);
    if pid == 0 {
        return true;
    }
    // SAFETY: a process handle opened, waited on and closed here.
    let still_running = unsafe {
        match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            Ok(handle) => {
                let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
                let waited = WaitForSingleObject(handle, millis);
                let _ = CloseHandle(handle);
                waited == WAIT_TIMEOUT
            }
            Err(_) => false,
        }
    };
    if !still_running {
        RELEASED_BROWSER_PID.store(0, Ordering::SeqCst);
    }
    !still_running
}

#[cfg(not(windows))]
fn old_browser_gone(_timeout: Duration) -> bool {
    true
}

/// How long a click waits for the released page's browser process before building anyway.
const OLD_BROWSER_WAIT: Duration = Duration::from_secs(5);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The theme's page background (`--background` in src/app.css; keep in sync), painted by the
/// window and the web view before the page draws anything: no white flash in dark themes.
/// `system` follows the OS light/dark setting.
fn theme_background(settings: &AppSettings, os: Option<tauri::Theme>) -> Color {
    let theme = if settings.color_theme.is_empty() {
        settings.theme.as_str()
    } else {
        settings.color_theme.as_str()
    };
    let (r, g, b) = match theme {
        "light" => (0xec, 0xee, 0xf1),
        "oled" | "hc-dark" => (0x00, 0x00, 0x00),
        "night" => (0x14, 0x11, 0x0d),
        "hc-light" => (0xff, 0xff, 0xff),
        "system" if os == Some(tauri::Theme::Light) => (0xec, 0xee, 0xf1),
        _ => (0x0f, 0x11, 0x15),
    };
    Color(r, g, b, 0xff)
}

/// Gives the main window the theme's background (at start and whenever it is built again).
pub fn apply_window_background(window: &tauri::WebviewWindow) {
    let color = theme_background(&AppSettings::load(), window.theme().ok());
    let _ = window.set_background_color(Some(color));
}

/// Asks WebView2 to keep its memory low while nobody looks at the window (caches trimmed,
/// less memory kept for faster redraws); back to normal when it is shown again.
#[cfg(windows)]
fn set_memory_target(window: &tauri::WebviewWindow, low: bool) {
    let _ = window.with_webview(move |webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core::Interface;
        let level = if low {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
        } else {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
        };
        // SAFETY: plain COM calls on the web view's own thread (with_webview runs there).
        unsafe {
            if let Ok(core) = webview.controller().CoreWebView2() {
                if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                    let _ = core19.SetMemoryUsageTargetLevel(level);
                }
            }
        }
    });
}

#[cfg(not(windows))]
fn set_memory_target(_window: &tauri::WebviewWindow, _low: bool) {}

pub fn show_main_window(app: &AppHandle) {
    cancel_prewarm();
    let window = match app.get_webview_window("main") {
        Some(window) => window,
        None if window_released() => {
            old_browser_gone(OLD_BROWSER_WAIT);
            match rebuild_main_window(app) {
                Some(window) => window,
                None => return,
            }
        }
        None => {
            eprintln!("[Harness Studio] Warning: Window 'main' not found!");
            return;
        }
    };
    if PENDING_MAXIMIZE.swap(false, Ordering::SeqCst) {
        let _ = window.maximize();
    }
    crate::startup::mark("window show");
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    report_visibility(&window, true);
}

/// Builds the main window again after [`release_window`], where it was, hidden: shown by
/// [`show_main_window`] right away or, when built ahead by [`prewarm_main_window`], on the click.
fn rebuild_main_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    let conf = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")?;
    let background = theme_background(&AppSettings::load(), None);
    let built = tauri::WebviewWindowBuilder::from_config(app, conf)
        .and_then(|b| b.visible(false).background_color(background).build());
    let window = match built {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[Harness Studio] Window could not be rebuilt: {}", e);
            return None;
        }
    };
    WINDOW_RELEASED.store(false, Ordering::SeqCst);
    allow_microphone(&window);
    // `system`: the OS theme is known once the window exists.
    apply_window_background(&window);
    match lock(&SAVED_PLACEMENT).take() {
        Some(p) => {
            #[cfg(windows)]
            let placed = p
                .normal_rect
                .is_some_and(|rect| set_normal_rect(&window, rect));
            #[cfg(not(windows))]
            let placed = false;
            if !placed {
                let _ = window.set_size(p.size);
                let _ = window.set_position(p.position);
            }
            PENDING_MAXIMIZE.store(p.maximized, Ordering::SeqCst);
        }
        None => fit_main_window_to_work_area(app),
    }
    Some(window)
}

/// The pointer is on the tray icon (or pressed it): builds the released window hidden in the
/// background, so the click that usually follows shows a page that is already loaded instead of
/// starting WebView2 then. Not shown within [`PREWARM_KEEP`], it is released again. Nothing to
/// do when the window exists (visible, or hidden and kept).
fn prewarm_main_window(app: &AppHandle) {
    {
        let mut deadline = lock(&PREWARM_DEADLINE);
        if deadline.is_some() {
            *deadline = Some(Instant::now() + PREWARM_KEEP);
            return;
        }
    }
    if !window_released() || app.get_webview_window("main").is_some() {
        return;
    }
    // The released page's browser is still shutting down: the next hover event tries again
    // (the click waits for it).
    if !old_browser_gone(Duration::ZERO) {
        return;
    }
    if rebuild_main_window(app).is_none() {
        return;
    }
    let generation = PREWARM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    *lock(&PREWARM_DEADLINE) = Some(Instant::now() + PREWARM_KEEP);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let wait = match *lock(&PREWARM_DEADLINE) {
                Some(at) if PREWARM_GEN.load(Ordering::SeqCst) == generation => {
                    at.saturating_duration_since(Instant::now())
                }
                _ => return,
            };
            if wait.is_zero() {
                break;
            }
            tokio::time::sleep(wait).await;
        }
        // On the main thread, in order with the tray events: a click that comes first wins.
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if PREWARM_GEN.load(Ordering::SeqCst) != generation {
                return;
            }
            *lock(&PREWARM_DEADLINE) = None;
            if let Some(window) = handle.get_webview_window("main") {
                if !window.is_visible().unwrap_or(true) {
                    release_hidden_window(&window);
                }
            }
        });
    });
}

/// The window is being shown: a window built in the background stays.
fn cancel_prewarm() {
    PREWARM_GEN.fetch_add(1, Ordering::SeqCst);
    *lock(&PREWARM_DEADLINE) = None;
}

/// Closes the hidden main window's web view, remembering where it was ([`rebuild_main_window`]
/// puts it back there). Returns whether it was closed.
fn release_hidden_window(window: &tauri::WebviewWindow) -> bool {
    if let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) {
        // Built in the background and not shown yet: its maximize is still pending.
        let pending = PENDING_MAXIMIZE.swap(false, Ordering::SeqCst);
        *lock(&SAVED_PLACEMENT) = Some(SavedPlacement {
            position,
            size,
            maximized: pending || window.is_maximized().unwrap_or(false),
            #[cfg(windows)]
            normal_rect: normal_rect(window),
        });
    }
    remember_browser_process(window);
    WINDOW_RELEASED.store(true, Ordering::SeqCst);
    if let Err(e) = window.destroy() {
        WINDOW_RELEASED.store(false, Ordering::SeqCst);
        eprintln!("[Harness Studio] Window could not be released: {}", e);
        return false;
    }
    true
}

/// Closes the main window's web view while it is hidden to the tray (asked by the UI when it
/// has nothing in flight, unless `startup.keep_window_in_tray`): the tray then costs only this
/// process, not the WebView2 processes. Servers, downloads and every other backend task keep
/// running; clicking the tray icon builds the window again (or shows the one built in the
/// background while the pointer was on the icon, [`prewarm_main_window`]).
///
/// Returns `released`, `busy` (a server is still working on a request, e.g. a reply the page
/// streams: ask again later) or `kept` (visible again, no tray, or the setting keeps it).
#[tauri::command]
pub async fn release_window(
    app: AppHandle,
    telemetry: tauri::State<'_, Arc<tokio::sync::Mutex<crate::telemetry::TelemetryPoller>>>,
    registry: tauri::State<'_, Arc<ServerRegistry>>,
) -> Result<&'static str, String> {
    if window_visible() || !tray_available() || AppSettings::load().startup.keep_window_in_tray {
        return Ok("kept");
    }
    let busy = {
        let poller = telemetry.lock().await;
        registry.active_servers().iter().any(|(id, _)| {
            let s = poller.last(id);
            s.requests_processing > 0 || s.active_slots > 0
        })
    };
    if busy {
        return Ok("busy");
    }
    let Some(window) = app.get_webview_window("main") else {
        return Ok("kept");
    };
    if window.is_visible().unwrap_or(true) {
        return Ok("kept");
    }
    Ok(if release_hidden_window(&window) {
        "released"
    } else {
        "kept"
    })
}

/// Shrinks the main window to its monitor's work area (the screen minus panels / taskbar) when
/// the configured size does not fit, e.g. 1150x760 on a 1366x768 laptop with a top panel, where
/// the bottom of the window (the wizard's "Continue" button) was off screen. Never below the
/// configured minimum size; centered afterwards. A window that fits but sticks out of the work
/// area is centered too.
pub fn fit_main_window_to_work_area(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let monitor = match window.current_monitor() {
        Ok(Some(m)) => Some(m),
        _ => window.primary_monitor().ok().flatten(),
    };
    // The configured size, not `outer_size()`: right after `show` GTK still reports 0x0.
    let (Some(monitor), Some(conf)) = (monitor, app.config().app.windows.first()) else {
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let want_w = (conf.width * scale) as u32;
    let want_h = (conf.height * scale) as u32;
    if want_w <= area.size.width && want_h <= area.size.height {
        // It fits, but Windows opens a new window at its default cascade spot: (192, 120) on a
        // 1920x1080 screen at 125 %, where the 960 px tall window ran 60 px under the taskbar
        // and hid the bottom of the page (the chat composer). Such a window is centered.
        if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
            let inside = pos.x >= area.position.x
                && pos.y >= area.position.y
                && pos.x + size.width as i32 <= area.position.x + area.size.width as i32
                && pos.y + size.height as i32 <= area.position.y + area.size.height as i32;
            let fits = size.width <= area.size.width && size.height <= area.size.height;
            if size.width > 0
                && size.height > 0
                && fits
                && !inside
                && !window.is_maximized().unwrap_or(false)
            {
                let x = area.position.x + (area.size.width - size.width) as i32 / 2;
                let y = area.position.y + (area.size.height - size.height) as i32 / 2;
                let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
            }
        }
        return;
    }
    let min_w = (conf.min_width.unwrap_or(0.0) * scale) as u32;
    let min_h = (conf.min_height.unwrap_or(0.0) * scale) as u32;
    let width = want_w.min(area.size.width).max(min_w);
    let height = want_h.min(area.size.height).max(min_h);
    let _ = window.set_size(tauri::PhysicalSize::new(width, height));
    let x = area.position.x + (area.size.width as i32 - width as i32).max(0) / 2;
    let y = area.position.y + (area.size.height as i32 - height as i32).max(0) / 2;
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

async fn quit(app: AppHandle) {
    let registry = app.state::<Arc<ServerRegistry>>().inner().clone();
    registry.stop_all().await;
    app.exit(0);
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let l = labels(&AppSettings::load().language);
    let items = TrayItems {
        show: MenuItem::with_id(app, "show", l.show, true, None::<&str>)?,
        stop_all: MenuItem::with_id(app, "stop_all", l.stop_all, true, None::<&str>)?,
        quit: MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?,
    };
    let menu = Menu::with_items(
        app,
        &[
            &items.show,
            &items.stop_all,
            &PredefinedMenuItem::separator(app)?,
            &items.quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(format!("Harness Studio — {}", l.idle))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "stop_all" => {
                let registry = app.state::<Arc<ServerRegistry>>().inner().clone();
                tauri::async_runtime::spawn(async move { registry.stop_all().await });
            }
            "quit" => {
                tauri::async_runtime::spawn(quit(app.clone()));
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_main_window(tray.app_handle()),
            // The pointer on the icon, or a press (left: click, right: the menu's "Show"): a
            // released window starts loading now. Windows and macOS report hover; the Linux
            // StatusNotifierItem tray (ksni) reports only the click (`Up`), handled above.
            TrayIconEvent::Enter { .. }
            | TrayIconEvent::Move { .. }
            | TrayIconEvent::Click {
                button_state: MouseButtonState::Down,
                ..
            } => prewarm_main_window(tray.app_handle()),
            _ => {}
        });
    // macOS: a black-and-clear template image the menu bar tints for light and dark bars
    // (source `icons/source/tray-template.svg`); Windows and Linux keep the colored app icon.
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png")).ok();
    #[cfg(not(target_os = "macos"))]
    let icon = app.default_window_icon().cloned().or_else(|| {
        tauri::image::Image::from_path("icons/32x32.png")
            .or_else(|_| tauri::image::Image::from_path("src-tauri/icons/32x32.png"))
            .ok()
    });
    if let Some(icon) = icon {
        builder = builder
            .icon(icon)
            .icon_as_template(cfg!(target_os = "macos"));
    }
    builder.build(app)?;
    app.manage(items);
    TRAY_READY.store(true, Ordering::Relaxed);
    Ok(())
}

/// Texts of the macOS menu bar. The standard items keep their roles (Hide, Quit, Copy, Paste, ...);
/// only the labels follow the UI language, which macOS would otherwise take from the system.
#[cfg(target_os = "macos")]
struct MenuLabels {
    about: &'static str,
    services: &'static str,
    hide: &'static str,
    hide_others: &'static str,
    quit: &'static str,
    file: &'static str,
    close_window: &'static str,
    edit: &'static str,
    undo: &'static str,
    redo: &'static str,
    cut: &'static str,
    copy: &'static str,
    paste: &'static str,
    select_all: &'static str,
    view: &'static str,
    fullscreen: &'static str,
    window: &'static str,
    minimize: &'static str,
    zoom: &'static str,
    help: &'static str,
}

#[cfg(target_os = "macos")]
fn menu_labels(language: &str) -> MenuLabels {
    if language == "en" {
        MenuLabels {
            about: "About Harness Studio",
            services: "Services",
            hide: "Hide Harness Studio",
            hide_others: "Hide Others",
            quit: "Quit Harness Studio",
            file: "File",
            close_window: "Close Window",
            edit: "Edit",
            undo: "Undo",
            redo: "Redo",
            cut: "Cut",
            copy: "Copy",
            paste: "Paste",
            select_all: "Select All",
            view: "View",
            fullscreen: "Enter Full Screen",
            window: "Window",
            minimize: "Minimize",
            zoom: "Zoom",
            help: "Help",
        }
    } else {
        MenuLabels {
            about: "Harness Studio Hakkında",
            services: "Servisler",
            hide: "Harness Studio'yu Gizle",
            hide_others: "Diğerlerini Gizle",
            quit: "Harness Studio'dan Çık",
            file: "Dosya",
            close_window: "Pencereyi Kapat",
            edit: "Düzen",
            undo: "Geri Al",
            redo: "Yinele",
            cut: "Kes",
            copy: "Kopyala",
            paste: "Yapıştır",
            select_all: "Tümünü Seç",
            view: "Görünüm",
            fullscreen: "Tam Ekran Yap",
            window: "Pencere",
            minimize: "Simge Durumuna Küçült",
            zoom: "Yakınlaştır",
            help: "Yardım",
        }
    }
}

/// The macOS menu bar in `language` (same structure as Tauri's default menu).
#[cfg(target_os = "macos")]
fn build_app_menu(app: &AppHandle, language: &str) -> tauri::Result<Menu<Wry>> {
    use tauri::menu::{AboutMetadata, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID};
    let l = menu_labels(language);
    let info = app.package_info();
    let about = AboutMetadata {
        name: Some(info.name.clone()),
        version: Some(info.version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        ..Default::default()
    };
    let sep = || PredefinedMenuItem::separator(app);
    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                info.name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app, Some(l.about), Some(about))?,
                    &sep()?,
                    &PredefinedMenuItem::services(app, Some(l.services))?,
                    &sep()?,
                    &PredefinedMenuItem::hide(app, Some(l.hide))?,
                    &PredefinedMenuItem::hide_others(app, Some(l.hide_others))?,
                    &sep()?,
                    &PredefinedMenuItem::quit(app, Some(l.quit))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                l.file,
                true,
                &[&PredefinedMenuItem::close_window(
                    app,
                    Some(l.close_window),
                )?],
            )?,
            &Submenu::with_items(
                app,
                l.edit,
                true,
                &[
                    &PredefinedMenuItem::undo(app, Some(l.undo))?,
                    &PredefinedMenuItem::redo(app, Some(l.redo))?,
                    &sep()?,
                    &PredefinedMenuItem::cut(app, Some(l.cut))?,
                    &PredefinedMenuItem::copy(app, Some(l.copy))?,
                    &PredefinedMenuItem::paste(app, Some(l.paste))?,
                    &PredefinedMenuItem::select_all(app, Some(l.select_all))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                l.view,
                true,
                &[&PredefinedMenuItem::fullscreen(app, Some(l.fullscreen))?],
            )?,
            &Submenu::with_id_and_items(
                app,
                WINDOW_SUBMENU_ID,
                l.window,
                true,
                &[
                    &PredefinedMenuItem::minimize(app, Some(l.minimize))?,
                    &PredefinedMenuItem::maximize(app, Some(l.zoom))?,
                    &sep()?,
                    &PredefinedMenuItem::close_window(app, Some(l.close_window))?,
                ],
            )?,
            &Submenu::with_id_and_items(app, HELP_SUBMENU_ID, l.help, true, &[])?,
        ],
    )
}

/// Sets the macOS menu bar in the UI language (called at startup and when the language changes).
/// Other platforms have no application menu.
pub fn set_app_menu(app: &AppHandle, language: &str) {
    #[cfg(target_os = "macos")]
    match build_app_menu(app, language) {
        Ok(menu) => {
            if let Err(e) = app.set_menu(menu) {
                eprintln!("[Harness Studio] App menu not set: {e}");
            }
        }
        Err(e) => eprintln!("[Harness Studio] App menu not built: {e}"),
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, language);
}

/// Re-labels the tray menu (and on macOS the menu bar) after a language change.
pub fn update_tray_language(app: &AppHandle, language: &str) {
    set_app_menu(app, language);
    let l = labels(language);
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.show.set_text(l.show);
        let _ = items.stop_all.set_text(l.stop_all);
        let _ = items.quit.set_text(l.quit);
    }
}

/// Whether a tray icon exists (the Settings page explains why "hide to tray" does nothing).
#[tauri::command]
pub fn is_tray_available() -> bool {
    tray_available()
}

/// Answer to the close question: `tray`, `quit` or `cancel`. With `remember` the choice is saved
/// (`startup.close_action`) and the question is not asked again; it can be changed in
/// Settings > Appearance and startup.
#[tauri::command]
pub async fn resolve_window_close(
    app: AppHandle,
    action: String,
    remember: bool,
) -> Result<AppSettings, String> {
    CLOSE_PROMPT_OPEN.store(false, Ordering::SeqCst);
    let chosen = match action.as_str() {
        "tray" => Some(CloseAction::Tray),
        "quit" => Some(CloseAction::Quit),
        _ => None,
    };
    let mut settings = AppSettings::load();
    if let (Some(choice), true) = (chosen, remember) {
        settings.startup.set_close_action(choice);
        settings.save()?;
    }
    match chosen {
        Some(CloseAction::Tray) if tray_available() => {
            if let Some(window) = app.get_webview_window("main") {
                hide_window_to_tray(&window);
            }
        }
        Some(_) => {
            tauri::async_runtime::spawn(quit(app.clone()));
        }
        None => {}
    }
    Ok(settings)
}

pub fn update_tray_tooltip(app: &AppHandle, running: usize) {
    let l = labels(&AppSettings::load().language);
    let text = if running == 0 {
        format!("Harness Studio — {}", l.idle)
    } else {
        format!("Harness Studio — {} {}", running, l.running)
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Hides the window to the tray; the page may then release itself ([`release_window`]).
fn hide_window_to_tray<M: Manager<Wry> + Emitter<Wry>>(window: &M) {
    if let Some(w) = target_window(window) {
        let _ = w.hide();
    }
    report_visibility(window, false);
    let _ = window.emit("window-in-tray", ());
}

/// Closing the main window: hide to the tray, quit, or ask (the `close-requested` event opens
/// the question in the frontend, answered with [`resolve_window_close`]). Quitting always goes
/// through [`quit`] (servers stopped, then exit) instead of relying on the last-window-closed
/// exit of the platform.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let action = AppSettings::load().startup.close_action();
            match action {
                CloseAction::Tray if tray_available() => hide_window_to_tray(window),
                CloseAction::Ask if tray_available() => {
                    // A repeated Close while the question is open only brings the window
                    // back; it must never turn into a Quit.
                    if CLOSE_PROMPT_OPEN.swap(true, Ordering::SeqCst) {
                        let _ = window.set_focus();
                    } else if window.emit("close-requested", ()).is_err() {
                        CLOSE_PROMPT_OPEN.store(false, Ordering::SeqCst);
                        tauri::async_runtime::spawn(quit(window.app_handle().clone()));
                    }
                }
                _ => {
                    tauri::async_runtime::spawn(quit(window.app_handle().clone()));
                }
            }
        }
        // Minimizing (and restoring) resizes the window; only a change is forwarded.
        WindowEvent::Resized(_) => {
            let minimized = window.is_minimized().unwrap_or(false);
            report_visibility(window, !minimized);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::with_associated_bundle_id;

    /// What tauri-plugin-autostart (auto-launch 0.5) writes for a LaunchAgent.
    const AGENT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n  <key>Label</key>\n  <string>com.harnessstudio.app</string>\n  \
<key>ProgramArguments</key>\n  <array>\n    \
<string>/Applications/Harness Studio.app/Contents/MacOS/harness-studio</string>\n    \
<string>--minimized</string>\n  </array>\n  <key>RunAtLoad</key>\n  <true/>\n\
</dict>\n</plist>";

    #[test]
    fn adds_bundle_id_before_dict_closes_and_keeps_other_keys() {
        let out = with_associated_bundle_id(AGENT, "com.harnessstudio.app").unwrap();
        let expected = AGENT.replace(
            "  <true/>\n</dict>",
            "  <true/>\n  <key>AssociatedBundleIdentifiers</key>\n  <array>\n    \
<string>com.harnessstudio.app</string>\n  </array>\n</dict>",
        );
        assert_eq!(out, expected);
        for kept in [
            "<key>Label</key>",
            "<key>ProgramArguments</key>",
            "--minimized",
            "<true/>",
        ] {
            assert!(out.contains(kept), "{kept} lost");
        }
        assert!(out.ends_with("</dict>\n</plist>"));
    }

    #[test]
    fn is_idempotent() {
        let once = with_associated_bundle_id(AGENT, "com.harnessstudio.app").unwrap();
        assert_eq!(
            with_associated_bundle_id(&once, "com.harnessstudio.app"),
            None
        );
    }

    #[test]
    fn leaves_non_xml_and_malformed_alone() {
        assert_eq!(
            with_associated_bundle_id("bplist00\u{0}\u{1}", "com.x"),
            None
        );
        assert_eq!(
            with_associated_bundle_id("<plist version=\"1.0\"><dict>", "com.x"),
            None
        );
        assert_eq!(with_associated_bundle_id(AGENT, ""), None);
    }

    #[test]
    fn escapes_the_identifier() {
        let out = with_associated_bundle_id(AGENT, "a<b&c").unwrap();
        assert!(out.contains("<string>a&lt;b&amp;c</string>"));
    }
}
