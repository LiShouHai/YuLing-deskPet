<script setup>
import { onBeforeUnmount, onMounted, ref } from "vue";
import {
  fetchMonitors,
  getAutostartStatus,
  onAutostartUpdate,
  onMonitorUpdate,
  setAutostart,
} from "./platformBridge";

const monitors = ref([]);
const isAutostartEnabled = ref(false);
const statusText = ref("启动中…");
const lastUpdated = ref(null);

let unlistenMonitors = null;
let unlistenAutostart = null;

function formatMonitorLabel(monitor) {
  const size = `${monitor.size.width} × ${monitor.size.height}`;
  const scale = Number(monitor.scale_factor ?? 1)
    .toFixed(2)
    .replace(/\.0+$/, "");
  return `${monitor.name ?? "未命名显示器"} · ${size} @${scale}x`;
}

async function syncMonitors() {
  try {
    statusText.value = "同步显示器信息…";
    monitors.value = await fetchMonitors();
    lastUpdated.value = new Date().toLocaleTimeString();
    statusText.value = `捕获到 ${monitors.value.length} 块显示器`;
  } catch (error) {
    statusText.value = "读取显示器信息失败";
    console.error("monitor sync failed", error);
  }
}

async function syncAutostart() {
  try {
    isAutostartEnabled.value = await getAutostartStatus();
  } catch (error) {
    console.error("autostart status failed", error);
  }
}

async function handleAutostartToggle() {
  try {
    isAutostartEnabled.value = await setAutostart(!isAutostartEnabled.value);
  } catch (error) {
    console.error("failed to set autostart", error);
  }
}

onMounted(async () => {
  await Promise.all([syncMonitors(), syncAutostart()]);

  unlistenMonitors = await onMonitorUpdate((payload) => {
    monitors.value = payload ?? [];
    lastUpdated.value = new Date().toLocaleTimeString();
  });

  unlistenAutostart = await onAutostartUpdate((payload) => {
    isAutostartEnabled.value = Boolean(payload);
  });
});

onBeforeUnmount(() => {
  unlistenMonitors?.();
  unlistenAutostart?.();
});
</script>

<template>
  <div class="pet-shell">
    <div class="halo"></div>
    <section class="status-panel glass">
      <p class="status-label">运行状态</p>
      <p class="status-value">{{ statusText }}</p>
      <button class="ghost-btn" type="button" @click="handleAutostartToggle">
        {{ isAutostartEnabled ? "关闭" : "开启" }} 开机自启
      </button>
      <p v-if="lastUpdated" class="hint">最近同步：{{ lastUpdated }}</p>
    </section>

    <section class="monitor-panel glass">
      <header>显示器 ({{ monitors.length }})</header>
      <ul>
        <li v-for="monitor in monitors" :key="monitor.id">
          <div class="dot" />
          <div class="info">
            <p>{{ formatMonitorLabel(monitor) }}</p>
            <small v-if="monitor.work_area">
              可用区域：{{ monitor.work_area.width }}×{{ monitor.work_area.height }}
            </small>
          </div>
        </li>
        <li v-if="!monitors.length" class="placeholder">
          暂无显示器数据，等待系统回传…
        </li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
.pet-shell {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 16px;
  box-sizing: border-box;
  user-select: none;
}

.halo {
  position: absolute;
  inset: 0;
  background: radial-gradient(circle at 30% 30%, rgba(255, 255, 255, 0.4), transparent 65%);
  filter: blur(30px);
  z-index: 0;
  pointer-events: none;
}

.glass {
  position: relative;
  z-index: 1;
  backdrop-filter: blur(30px);
  background: rgba(18, 18, 27, 0.65);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 18px;
  padding: 16px;
  box-shadow: 0 15px 40px rgba(0, 0, 0, 0.35);
  color: #f4f6fb;
}

.status-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.status-label {
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: rgba(255, 255, 255, 0.65);
}

.status-value {
  font-size: 18px;
  font-weight: 600;
}

.ghost-btn {
  margin-top: 4px;
  align-self: flex-start;
  border-radius: 999px;
  padding: 6px 16px;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.2);
  color: inherit;
  font-size: 13px;
  letter-spacing: 0.05em;
  transition: background 0.2s ease, border 0.2s ease;
}

.ghost-btn:hover {
  background: rgba(255, 255, 255, 0.2);
  border-color: rgba(255, 255, 255, 0.35);
}

.monitor-panel ul {
  list-style: none;
  padding: 0;
  margin: 8px 0 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 180px;
  overflow: hidden;
}

.monitor-panel li {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}

.monitor-panel header {
  font-size: 14px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.9);
}

.info small {
  color: rgba(255, 255, 255, 0.6);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #27e3b9;
  box-shadow: 0 0 6px rgba(39, 227, 185, 0.9);
}

.placeholder {
  opacity: 0.6;
  font-style: italic;
}

.hint {
  font-size: 11px;
  opacity: 0.55;
}
</style>
