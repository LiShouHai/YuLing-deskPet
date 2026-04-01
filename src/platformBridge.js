import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export async function fetchMonitors() {
  return invoke("platform_get_monitors");
}

export async function logicalToPhysical(scaleFactor, point) {
  return invoke("platform_logical_to_physical", {
    scale_factor: scaleFactor,
    position: point,
  });
}

export async function physicalToLogical(scaleFactor, point) {
  return invoke("platform_physical_to_logical", {
    scale_factor: scaleFactor,
    position: point,
  });
}

export async function getAutostartStatus() {
  return invoke("platform_get_autostart");
}

export async function setAutostart(enabled) {
  return invoke("platform_set_autostart", { enabled });
}

export async function onMonitorUpdate(callback) {
  return listen("platform:monitors-updated", (event) => {
    callback(event.payload);
  });
}

export async function onAutostartUpdate(callback) {
  return listen("platform:autostart-updated", (event) => {
    callback(event.payload);
  });
}
