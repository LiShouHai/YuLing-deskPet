//! 隅灵桌宠 PlatformBridge：
//! - 暴露给前端的 Tauri 命令（显示器、自启动、坐标转换）
//! - 系统托盘初始化与事件处理
//! - 监视器轮询广播，保证多屏信息实时刷新

mod reminder;

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{
    async_runtime::spawn,
    image::Image,
    menu::{MenuBuilder, MenuEvent},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartExt};

const MONITOR_EVENT: &str = "platform:monitors-updated";
const AUTOSTART_EVENT: &str = "platform:autostart-updated";
const TRAY_ID_SHOW: &str = "tray-show";
const TRAY_ID_AUTOSTART: &str = "tray-autostart";
const TRAY_ID_REMINDERS: &str = "tray-reminders";
const TRAY_ID_QUIT: &str = "tray-quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayIconAction {
    None,
    RevealMainWindow,
}

/// 屏幕坐标点（物理像素）
#[derive(Debug, Clone, Serialize, PartialEq)]
struct MonitorPoint {
    x: i32,
    y: i32,
}

/// 屏幕尺寸
#[derive(Debug, Clone, Serialize, PartialEq)]
struct MonitorSize {
    width: u32,
    height: u32,
}

/// 屏幕可用区域，包含坐标与尺寸
#[derive(Debug, Clone, Serialize, PartialEq)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// 聚合的屏幕描述信息
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

/// 返回所有屏幕描述
#[tauri::command]
fn platform_get_monitors(app: AppHandle) -> Result<Vec<MonitorDescriptor>, String> {
    collect_monitors(&app).map_err(|err| err.to_string())
}

/// 将逻辑坐标转换为物理坐标
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

/// 将物理坐标转换为逻辑坐标
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

/// 切换开机自启状态，并将结果广播给前端
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

/// 读取当前自启状态
#[tauri::command]
fn platform_get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|err| err.to_string())
}

/// 工具函数：从 runtime 获取完整屏幕列表
fn collect_monitors(app: &AppHandle) -> tauri::Result<Vec<MonitorDescriptor>> {
    let monitors = app.available_monitors()?;
    Ok(monitors
        .into_iter()
        .enumerate()
        .map(|(idx, monitor)| {
            let position = monitor.position().clone();
            let size = monitor.size().clone();
            let work_area = monitor.work_area().clone();
            MonitorDescriptor {
                id: format!("monitor-{}", idx),
                name: monitor.name().cloned(),
                scale_factor: monitor.scale_factor(),
                position: MonitorPoint {
                    x: position.x,
                    y: position.y,
                },
                size: MonitorSize {
                    width: size.width,
                    height: size.height,
                },
                work_area: Some(MonitorRect {
                    x: work_area.position.x,
                    y: work_area.position.y,
                    width: work_area.size.width,
                    height: work_area.size.height,
                }),
            }
        })
        .collect())
}

/// 后台轮询屏幕变化并通过事件推送到前端
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
            // 轮询间隔 2 秒，兼顾实时性与性能
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

/// 显示主窗口并聚焦
fn reveal_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 显示提醒窗口并重新居中，避免窗口被用户移出可视区域
fn reveal_reminder_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("reminder") {
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 托盘菜单中“切换自启”项的处理
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

/// 统一处理托盘菜单事件
fn handle_tray_menu_event(app: &AppHandle, event: &MenuEvent) {
    match event.id().0.as_str() {
        TRAY_ID_SHOW => reveal_main_window(app),
        TRAY_ID_REMINDERS => reveal_reminder_window(app),
        TRAY_ID_AUTOSTART => handle_autostart_toggle(app),
        TRAY_ID_QUIT => app.exit(0),
        _ => {}
    }
}

fn tray_icon_action(event: &TrayIconEvent) -> TrayIconAction {
    match event {
        TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => TrayIconAction::RevealMainWindow,
        _ => TrayIconAction::None,
    }
}

/// 初始化系统托盘：菜单、图标、点击行为
fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(desktop)]
    {
        let menu = MenuBuilder::new(app)
            .text(TRAY_ID_SHOW, "显示隅灵")
            .text(TRAY_ID_REMINDERS, "提醒列表")
            .text(TRAY_ID_AUTOSTART, "切换开机自启")
            .text(TRAY_ID_QUIT, "退出")
            .build()?;

        let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;

        TrayIconBuilder::new()
            .menu(&menu)
            .icon(icon)
            .show_menu_on_left_click(true)
            .on_menu_event(|app, event| handle_tray_menu_event(app, &event))
            .on_tray_icon_event(|tray, event| {
                if tray_icon_action(&event) == TrayIconAction::RevealMainWindow {
                    let app = tray.app_handle();
                    reveal_main_window(&app);
                }
            })
            .build(app)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{tray_icon_action, TrayIconAction};
    use tauri::{
        tray::{MouseButton, MouseButtonState, TrayIconEvent, TrayIconId},
        PhysicalPosition, PhysicalSize, Position, Rect, Size,
    };

    fn sample_rect() -> Rect {
        Rect {
            position: Position::Physical(PhysicalPosition::new(0, 0)),
            size: Size::Physical(PhysicalSize::new(16, 16)),
        }
    }

    #[test]
    fn single_left_click_does_not_steal_menu_focus() {
        let event = TrayIconEvent::Click {
            id: TrayIconId::new("tray"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: sample_rect(),
            button: MouseButton::Left,
            button_state: MouseButtonState::Down,
        };

        assert_eq!(tray_icon_action(&event), TrayIconAction::None);
    }

    #[test]
    fn left_button_double_click_reveals_main_window() {
        let event = TrayIconEvent::DoubleClick {
            id: TrayIconId::new("tray"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: sample_rect(),
            button: MouseButton::Left,
        };

        assert_eq!(tray_icon_action(&event), TrayIconAction::RevealMainWindow);
    }

    #[test]
    fn non_left_double_click_is_ignored() {
        let event = TrayIconEvent::DoubleClick {
            id: TrayIconId::new("tray"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: sample_rect(),
            button: MouseButton::Right,
        };

        assert_eq!(tray_icon_action(&event), TrayIconAction::None);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle();
            app.manage(reminder::ReminderScheduler::default());
            start_monitor_broadcast(handle.clone()); // 启动屏幕轮询
            init_tray(&handle)?; // 建立托盘与菜单
            reminder::init_database(&handle)?;
            reminder::start_scheduler(handle.clone());
            Ok(())
        })
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            platform_get_monitors,
            platform_logical_to_physical,
            platform_physical_to_logical,
            platform_set_autostart,
            platform_get_autostart,
            reminder_create,
            reminder_list,
            reminder_complete,
            reminder_delete,
            reminder_snooze
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
async fn reminder_create(
    app: AppHandle,
    payload: reminder::ReminderInput,
) -> Result<reminder::ReminderRecord, String> {
    reminder::create_reminder(app, payload).await
}

#[tauri::command]
async fn reminder_list(app: AppHandle) -> Result<Vec<reminder::ReminderRecord>, String> {
    reminder::list_reminders(app).await
}

#[tauri::command]
async fn reminder_complete(
    app: AppHandle,
    payload: reminder::ReminderIdPayload,
) -> Result<bool, String> {
    reminder::complete_reminder(app, payload).await
}

#[tauri::command]
async fn reminder_delete(
    app: AppHandle,
    payload: reminder::ReminderIdPayload,
) -> Result<bool, String> {
    reminder::delete_reminder(app, payload).await
}

#[tauri::command]
async fn reminder_snooze(
    app: AppHandle,
    payload: reminder::ReminderSnoozePayload,
) -> Result<reminder::ReminderRecord, String> {
    reminder::snooze_reminder(app, payload).await
}
