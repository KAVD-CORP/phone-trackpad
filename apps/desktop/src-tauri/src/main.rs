#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use trackpad_core::{DEFAULT_TCP_PORT, DEFAULT_UDP_PORT};

/// Embedded backend state. `Backend` holds the mDNS advertisement alive.
struct AppState {
    backend: Mutex<Option<trackpad_desktop::service::Backend>>,
}

#[derive(serde::Serialize)]
struct CeremonyDto {
    qr_svg: String,
    short_code: String,
    expires_at_ms: u64,
}

#[derive(serde::Serialize)]
struct PendingDto {
    id: u64,
    device_name: String,
}

#[derive(serde::Serialize)]
struct DeviceDto {
    name: String,
    key: String,
}

#[derive(serde::Serialize)]
struct StatusDto {
    service_up: bool,
    pairing_active: bool,
    pending: Vec<PendingDto>,
    trusted: Vec<DeviceDto>,
    live: Option<String>,
}

fn with_shared<T>(
    state: &State<AppState>,
    f: impl FnOnce(&trackpad_desktop::pairing_server::PairingShared) -> T,
) -> Result<T, String> {
    let backend = state.backend.lock().map_err(|e| e.to_string())?;
    let backend = backend
        .as_ref()
        .ok_or_else(|| "service not started".to_string())?;
    let shared = backend.shared.lock().map_err(|e| e.to_string())?;
    Ok(f(&shared))
}

fn ensure_booted(state: &State<AppState>) -> Result<(), String> {
    let mut backend = state.backend.lock().map_err(|e| e.to_string())?;
    if backend.is_none() {
        *backend = Some(trackpad_desktop::service::boot(
            DEFAULT_UDP_PORT,
            DEFAULT_TCP_PORT,
            true,
        ));
    }
    Ok(())
}

#[tauri::command]
fn boot_service(state: State<AppState>) -> Result<String, String> {
    ensure_booted(&state)?;
    with_shared(&state, |s| s.device_name().to_string())
}

#[tauri::command]
fn begin_pairing(state: State<AppState>) -> Result<CeremonyDto, String> {
    let backend = state.backend.lock().map_err(|e| e.to_string())?;
    let backend = backend
        .as_ref()
        .ok_or_else(|| "service not started".to_string())?;
    let mut shared = backend.shared.lock().map_err(|e| e.to_string())?;
    let c = shared.begin_ceremony().map_err(|e| e.to_string())?;
    Ok(CeremonyDto {
        qr_svg: c.qr_svg,
        short_code: c.short_code,
        expires_at_ms: c.expires_at_ms,
    })
}

#[tauri::command]
fn pairing_status(state: State<AppState>) -> Result<StatusDto, String> {
    with_shared(&state, |s| StatusDto {
        service_up: true,
        pairing_active: s.ceremony_active(),
        pending: s
            .pending()
            .iter()
            .map(|p| PendingDto {
                id: p.id,
                device_name: p.device_name.clone(),
            })
            .collect(),
        trusted: s
            .trust_peers()
            .iter()
            .map(|p| DeviceDto {
                name: p.name.clone(),
                key: trackpad_desktop::service::hex(&p.pubkey),
            })
            .collect(),
        live: s.live().map(|l| l.peer_name.clone()),
    })
}

#[tauri::command]
fn approve_device(state: State<AppState>, id: u64, ok: bool) -> Result<bool, String> {
    let backend = state.backend.lock().map_err(|e| e.to_string())?;
    let backend = backend
        .as_ref()
        .ok_or_else(|| "service not started".to_string())?;
    let mut shared = backend.shared.lock().map_err(|e| e.to_string())?;
    Ok(shared.approve(id, ok))
}

#[tauri::command]
fn revoke_device(state: State<AppState>, key: String) -> Result<bool, String> {
    let raw =
        trackpad_desktop::service::from_hex(key.trim()).map_err(|e| format!("bad key: {e}"))?;
    let backend = state.backend.lock().map_err(|e| e.to_string())?;
    let backend = backend
        .as_ref()
        .ok_or_else(|| "service not started".to_string())?;
    let mut shared = backend.shared.lock().map_err(|e| e.to_string())?;
    Ok(shared.revoke(&raw))
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct WebPinDto {
    pin: String,
    url: String,
    qr_svg: String,
    expires_in_secs: u64,
}

#[tauri::command]
fn web_begin(state: State<AppState>) -> Result<WebPinDto, String> {
    let backend = state.backend.lock().map_err(|e| e.to_string())?;
    let backend = backend
        .as_ref()
        .ok_or_else(|| "service not started".to_string())?;
    let mut web = backend.web.lock().map_err(|e| e.to_string())?;
    let pin = web.begin().map_err(|e| e.to_string())?;
    let ip = trackpad_desktop::qr::primary_lan_ip();
    let url = format!("http://{ip}:{}", trackpad_desktop::web::WEB_PORT);
    // The fragment never leaves the phone: Safari opens the URL, the page
    // reads the PIN locally and pairs. iPhone Camera can scan this (it is
    // just a link), unlike the Noise QR which needs the app.
    let qr_svg = trackpad_desktop::qr::qr_svg(&format!("{url}#{pin}"))
        .map_err(|e| format!("qr failed: {e}"))?;
    Ok(WebPinDto {
        pin,
        url,
        qr_svg,
        expires_in_secs: trackpad_desktop::web::WEB_PIN_TTL_MS / 1000,
    })
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())
    } else {
        manager.disable().map_err(|e| e.to_string())
    }
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show").build(app)?;
    let pair = MenuItemBuilder::with_id("pair", "Pair new phone…").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &pair, &quit])
        .build()?;
    let idle = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-idle.png"))
        .expect("tray idle icon must decode");
    TrayIconBuilder::new()
        .icon(idle)
        .tooltip("Phone Trackpad — idle")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "pair" => {
                show_main(app);
                let _ = app.emit("open-pairing", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(event, TrayIconEvent::Click { .. }) {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Poll the live session and reflect it in the tray (icon + tooltip).
/// Best-effort: tray failures never affect input.
fn spawn_tray_monitor(app: AppHandle) {
    std::thread::spawn(move || {
        let active =
            tauri::image::Image::from_bytes(include_bytes!("../icons/tray-active.png")).ok();
        let idle = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-idle.png")).ok();
        let mut was_live = false;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let live_name: Option<String> = (|| {
                let state = app.try_state::<AppState>()?;
                let shared = {
                    let backend = state.backend.lock().ok()?;
                    backend.as_ref()?.shared.clone()
                };
                let s = shared.lock().ok()?;
                s.live().map(|l| l.peer_name.clone())
            })();
            let is_live = live_name.is_some();
            if is_live == was_live {
                continue;
            }
            was_live = is_live;
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_tooltip(Some(if let Some(name) = live_name {
                    format!("Phone Trackpad — {name} connected")
                } else {
                    "Phone Trackpad — idle".to_string()
                }));
                let _ = tray.set_icon(if is_live {
                    active.clone()
                } else {
                    idle.clone()
                });
            }
        }
    });
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .manage(AppState {
            backend: Mutex::new(None),
        })
        .setup(|app| {
            // Boot the input service with the app; the window is a remote.
            let state: State<AppState> = app.state();
            *state.backend.lock().map_err(|e| e.to_string())? = Some(
                trackpad_desktop::service::boot(DEFAULT_UDP_PORT, DEFAULT_TCP_PORT, true),
            );
            build_tray(app.handle())?;
            spawn_tray_monitor(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            boot_service,
            begin_pairing,
            pairing_status,
            approve_device,
            revoke_device,
            get_autostart,
            set_autostart,
            web_begin
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}
