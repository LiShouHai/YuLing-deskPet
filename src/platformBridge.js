/**
 * PlatformBridge
 * 用于封装与 Rust 侧的 Invoke / Event 通信，保证前端逻辑整洁。
 * 函数命名保持 platform_xxx 对应的 tauri 命令，便于检索。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * 判断当前环境是否为 Tauri WebView。
 * 纯浏览器模式（直接访问 Vite dev server）时，该值为 false。
 */
export const isTauriEnvironment =
  typeof window !== "undefined" && typeof window.__TAURI_INTERNALS__ !== "undefined";

const noopUnlisten = () => {};

/**
 * 获取当前可用显示器列表
 */
export async function fetchMonitors() {
  if (!isTauriEnvironment) return [];
  return invoke("platform_get_monitors");
}

/**
 * 将逻辑坐标（CSS 像素）转换为物理坐标（真实像素）
 * @param {number} scaleFactor - 显示器缩放系数
 * @param {{x:number,y:number}} point - 逻辑坐标
 */
export async function logicalToPhysical(scaleFactor, point) {
  if (!isTauriEnvironment) return point;
  return invoke("platform_logical_to_physical", {
    scale_factor: scaleFactor,
    position: point
  });
}

/**
 * 将物理坐标转换为逻辑坐标
 */
export async function physicalToLogical(scaleFactor, point) {
  if (!isTauriEnvironment) return point;
  return invoke("platform_physical_to_logical", {
    scale_factor: scaleFactor,
    position: point
  });
}

/**
 * 查询系统自启动状态
 */
export async function getAutostartStatus() {
  if (!isTauriEnvironment) return false;
  return invoke("platform_get_autostart");
}

/**
 * 设置系统自启动
 * @param {boolean} enabled
 */
export async function setAutostart(enabled) {
  if (!isTauriEnvironment) return enabled;
  return invoke("platform_set_autostart", { enabled });
}

/**
 * 监听显示器更新事件
 * @param {(payload:any)=>void} callback
 * @returns {Promise<UnlistenFn>}
 */
export async function onMonitorUpdate(callback) {
  if (!isTauriEnvironment) return noopUnlisten;
  return listen("platform:monitors-updated", (event) => {
    callback(event.payload);
  });
}

/**
 * 监听自启动状态变更
 */
export async function onAutostartUpdate(callback) {
  if (!isTauriEnvironment) return noopUnlisten;
  return listen("platform:autostart-updated", (event) => {
    callback(event.payload);
  });
}
