use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{
    async_runtime::spawn, AppHandle, CustomMenuItem, Manager, SystemTray, SystemTrayEvent,
    SystemTrayMenu, SystemTrayMenuItem,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;

const MONITOR_EVENT: &str = "platform:monitors-updated";
const AUTOSTART_EVENT: &str = "platform:autostart-updated";

#[derive(Debug, Clone, Serialize, PartialEq)]
struct MonitorPoint {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct MonitorSize {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct MonitorDescriptor {
    id: String,
    name: Option<String>,
    scale_factor: f64,
    position: MonitorPoint,
    size: MonitorSize,
    work_area: Option<MonitorRect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogicalPositionPayload {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PhysicalPositionPayload {
    x: i32,
    y: i32,
}

#[tauri::command]
fn platform_get_monitors(app: AppHandle) -> Result<Vec<MonitorDescriptor>, String> {
    collect_monitors(&app).map_err(|err| err.to_string())
}

#[tauri::command]
fn platform_logical_to_physical(
    scale_factor: f64,
    position: LogicalPositionPayload,
) -> PhysicalPositionPayload {
    PhysicalPositionPayload {
        x: (position.x * scale_factor).round() as i32,
        y: (position.y * scale_factor).round() as i32,
    }
}

#[tauri::command]
fn platform_physical_to_logical(
    scale_factor: f64,
    position: PhysicalPositionPayload,
) -> LogicalPositionPayload {
    LogicalPositionPayload {
        x: position.x as f64 / scale_factor,
        y: position.y as f64 / scale_factor,
    }
}

#[tauri::command]
fn platform_set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(|err| err.to_string())?;
    } else {
        launcher.disable().map_err(|err| err.to_string())?;
    }
    let _ = app.emit(AUTOSTART_EVENT, enabled);
    Ok(enabled)
}

#[tauri::command]
fn platform_get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|err| err.to_string())
}

fn collect_monitors(app: &AppHandle) -> tauri::Result<Vec<MonitorDescriptor>> {
    let monitors = app.available_monitors()?;
    Ok(monitors
        .into_iter()
        .enumerate()
        .map(|(idx, monitor)| {
            let position = monitor.position();
            let size = monitor.size();
            let work_area = monitor.work_area();
            MonitorDescriptor {
                id: format!("monitor-{}", idx),
                name: monitor.name(),
                scale_factor: monitor.scale_factor(),
                position: MonitorPoint {
                    x: position.x,
                    y: position.y,
                },
                size: MonitorSize {
                    width: size.width,
                    height: size.height,
                },
                work_area: work_area.map(|area| MonitorRect {
                    x: area.position.x,
                    y: area.position.y,
                    width: area.size.width,
                    height: area.size.height,
                }),
            }
        })
        .collect())
}

fn start_monitor_broadcast(app: AppHandle) {
    spawn(async move {
        let mut previous: Option<Vec<MonitorDescriptor>> = None;
        loop {
            if let Ok(monitors) = collect_monitors(&app) {
                let changed = previous.as_ref().map(|p| p != &monitors).unwrap_or(true);
                if changed {
                    let _ = app.emit(MONITOR_EVENT, monitors.clone());
                    previous = Some(monitors);
                }
            }
            tauri::async_runtime::sleep(Duration::from_secs(2)).await;
        }
    });
}

fn build_tray() -> SystemTray {
    let show = CustomMenuItem::new("show", "显示隅灵");
    let toggle_autostart = CustomMenuItem::new("toggle-autostart", "切换开机自启");
    let quit = CustomMenuItem::new("quit", "退出");
    let menu = SystemTrayMenu::new()
        .add_item(show)
        .add_item(toggle_autostart)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);
    SystemTray::new().with_menu(menu)
}

fn reveal_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn handle_autostart_toggle(app: &AppHandle) {
    if let Ok(current) = app.autolaunch().is_enabled() {
        let launcher = app.autolaunch();
        let target = !current;
        let result = if target {
            launcher.enable()
        } else {
            launcher.disable()
        };
        if result.is_ok() {
            let _ = app.emit(AUTOSTART_EVENT, target);
        }
    }
}

fn handle_tray_event(app: &AppHandle, event: SystemTrayEvent) {
    match event {
        SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
            "show" => reveal_main_window(app),
            "toggle-autostart" => handle_autostart_toggle(app),
            "quit" => app.exit(0),
            _ => {}
        },
        SystemTrayEvent::LeftClick { .. } => reveal_main_window(app),
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .system_tray(build_tray())
        .on_system_tray_event(handle_tray_event)
        .setup(|app| {
            start_monitor_broadcast(app.handle());
            Ok(())
        })
        .plugin(tauri_plugin_autostart::init(Default::default()))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            platform_get_monitors,
            platform_logical_to_physical,
            platform_physical_to_logical,
            platform_set_autostart,
            platform_get_autostart
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
