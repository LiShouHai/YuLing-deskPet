<script setup>
/**
 * 桌宠主界面组件：
 * - 负责窗口内所有交互（拖拽、点击、双击控制面板）
 * - 承担监视器和自启动状态展示
 * - 驱动 MotionController 控制宠物动画
 * 由于采用 `<script setup>`，声明即导出，需保持结构清晰。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { getCurrentWindow, LogicalPosition } from "@tauri-apps/api/window";
import {
  fetchMonitors,
  getAutostartStatus,
  isTauriEnvironment,
  onAutostartUpdate,
  onMonitorUpdate,
  setAutostart,
} from "./platformBridge";
import { usePetStore } from "./stores/petStore";
import { useMotionController } from "./motion/useMotionController";
import motionManifest from "./assets/motion/manifest.json";

// Pinia store 承载跨组件共享状态
const petStore = usePetStore();
// Tauri 提供的窗口句柄，用于操控位置等特性
const tauriWindow = isTauriEnvironment ? getCurrentWindow() : null;
// 控制信息面板显隐，双击宠物切换
const showControlPanel = ref(true);
// 通过计算属性获取监视器列表和电源模式，保持响应式
const monitors = computed(() => petStore.monitors);
const powerMode = computed(() => petStore.powerMode);
// MotionController 提供当前动画状态、覆盖层强度以及状态 setter
const { state: motionState, overlayIntensity, frame, setState } = useMotionController(powerMode);

let unlistenMonitors = null;
let unlistenAutostart = null;
let reminderTimer = null;

const frameProgress = computed(() => (frame.value % 100) / 100);

/**
 * 解析当前状态对应的帧图片
 */
const currentFrameSrc = computed(() => {
  const stateConfig = motionManifest[motionState.value] ?? motionManifest.idle;
  const frames = stateConfig?.frames ?? [];
  if (!frames.length) return "";
  const index = Math.floor(frameProgress.value * frames.length) % frames.length;
  const path = frames[index];
  return new URL(`./assets/motion/${path}`, import.meta.url).href;
});

/**
 * 将监视器对象转换为简短的显示文案
 * @param {object} monitor - 后端返回的监视器描述
 * @returns {string} - 包含名称、分辨率、缩放比的信息
 */
function formatMonitorLabel(monitor) {
  const size = `${monitor.size.width} × ${monitor.size.height}`;
  const scale = Number(monitor.scale_factor ?? 1)
    .toFixed(2)
    .replace(/\.0+$/, "");
  return `${monitor.name ?? "未命名显示器"} · ${size} @${scale}x`;
}

/**
 * 从 PlatformBridge 同步监视器列表并写入 store
 * - 请求过程中更新状态文案，便于用户感知
 * - 捕获异常并打印，避免 UI 崩溃
 */
async function syncMonitors() {
  if (!isTauriEnvironment) {
    petStore.statusText = "浏览器预览模式：无法获取系统显示器";
    petStore.setMonitors([]);
    return;
  }
  try {
    petStore.statusText = "同步显示器信息…";
    const list = await fetchMonitors();
    petStore.setMonitors(list);
  } catch (error) {
    petStore.statusText = "读取显示器信息失败";
    console.error("monitor sync failed", error);
  }
}

/**
 * 查询当前系统自启动设置
 */
async function syncAutostart() {
  if (!isTauriEnvironment) {
    petStore.setAutostart(false);
    return;
  }
  try {
    const enabled = await getAutostartStatus();
    petStore.setAutostart(enabled);
  } catch (error) {
    console.error("autostart status failed", error);
  }
}

/**
 * 切换自启动状态
 * - 直接调用 Rust 端命令并以返回结果为准
 */
async function handleAutostartToggle() {
  if (!isTauriEnvironment) {
    console.warn("当前在浏览器预览模式，无法设置系统自启");
    return;
  }
  try {
    const next = await setAutostart(!petStore.autostartEnabled);
    petStore.setAutostart(next);
  } catch (error) {
    console.error("failed to set autostart", error);
  }
}

/**
 * 切换低功耗模式
 * - 仅修改 store，由 MotionController watch 负责降帧
 */
function toggleLowPower() {
  petStore.togglePowerMode();
}

/**
 * 触发提醒脉冲动画，占位用
 * - 设置提醒标记，若已有计时器需先清除
 */
function triggerReminder() {
  if (reminderTimer) clearTimeout(reminderTimer);
  petStore.pulseReminder(true);
  reminderTimer = setTimeout(() => petStore.pulseReminder(false), 4000);
}

/**
 * 根据 store 状态推导动画表现
 * 优先级：拖拽 > 提醒 > 低功耗 > 待机
 */
function updateMotionState() {
  if (petStore.dragging) {
    setState("drag");
    return;
  }
  if (petStore.reminderActive) {
    setState("reminderPulse");
    return;
  }
  if (petStore.powerMode === "low") {
    setState("sleep");
    return;
  }
  setState("idle");
}

/**
 * 点击宠物时给一次短暂的 react 动画
 */
function handleAvatarClick() {
  setState("react");
  setTimeout(() => updateMotionState(), 500);
}

/**
 * 双击切换面板显隐
 */
function togglePanel() {
  showControlPanel.value = !showControlPanel.value;
}

/**
 * 手动拖拽方案：
 * - 监听 pointermove 手动设置窗口位置
 * - 用于 startDragging 不可用或报错时的兜底
 */
async function fallbackManualDrag(event) {
  if (!tauriWindow) return;
  const target = event.target;
  if (target?.setPointerCapture) {
    try {
      target.setPointerCapture(event.pointerId);
    } catch (captureError) {
      console.warn("指针捕获失败，不影响拖拽", captureError);
    }
  }
  const startMouse = { x: event.screenX, y: event.screenY };
  const startPos = await tauriWindow.outerPosition();
  const scaleFactor = await tauriWindow.scaleFactor();
  const logicalStart = startPos.toLogical(scaleFactor);

  await new Promise((resolve) => {
    const move = (moveEvent) => {
      const dx = moveEvent.screenX - startMouse.x;
      const dy = moveEvent.screenY - startMouse.y;
      const next = new LogicalPosition(
        Math.round(logicalStart.x + dx),
        Math.round(logicalStart.y + dy)
      );
      tauriWindow.setPosition(next);
    };

    const up = () => {
      window.removeEventListener("pointermove", move);
      if (target?.releasePointerCapture) {
        try {
          target.releasePointerCapture(event.pointerId);
        } catch (releaseError) {
          console.warn("释放指针捕获失败", releaseError);
        }
      }
      resolve();
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
  });
}

/**
 * 按下宠物开始拖拽
 * - 有 startDragging 则调用原生拖拽
 * - 否则退回手动计算位置
 */
async function beginDrag(event) {
  if (!tauriWindow) {
    console.warn("Tauri 窗口句柄不可用，跳过拖拽");
    return;
  }
  event.preventDefault();
  petStore.setDragging(true);
  setState("drag");

  try {
    await fallbackManualDrag(event);
  } finally {
    petStore.setDragging(false);
    updateMotionState();
  }
}

/**
 * 生命周期：组件挂载时同步一次数据并订阅事件
 */
onMounted(async () => {
  await Promise.all([syncMonitors(), syncAutostart()]);

  if (isTauriEnvironment) {
    unlistenMonitors = await onMonitorUpdate((payload) => {
      petStore.setMonitors(payload ?? []);
    });

    unlistenAutostart = await onAutostartUpdate((payload) => {
      petStore.setAutostart(Boolean(payload));
    });
  }

  updateMotionState();
});

/**
 * 生命周期：卸载时释放监听、计时器，防止内存泄漏
 */
onBeforeUnmount(() => {
  unlistenMonitors?.();
  unlistenAutostart?.();
  if (reminderTimer) clearTimeout(reminderTimer);
});

// 监听关键状态，变化时更新动画状态
watch(
  () => [petStore.dragging, petStore.reminderActive, petStore.powerMode],
  () => updateMotionState(),
  { deep: true }
);
</script>

<template>
  <!-- 根容器，双击触发控制面板显隐 -->
  <div class="pet-shell" @dblclick="togglePanel">
    <div
      class="pet-avatar"
      :class="[`state-${motionState}`, { dragging: petStore.dragging }]"
      :style="{
        boxShadow: `0 25px 45px rgba(23, 25, 35, ${0.35 + overlayIntensity * 0.25})`,
      }"
      @pointerdown="beginDrag"
      @click="handleAvatarClick"
    >
      <!-- 角色主体：优先展示帧动画，若无素材则回退到几何拼贴 -->
      <div class="pet-body" :class="{ 'has-frame': Boolean(currentFrameSrc) }">
        <img
          v-if="currentFrameSrc"
          class="pet-frame"
          :src="currentFrameSrc"
          alt="隅灵动画帧"
          draggable="false"
        />
        <template v-else>
          <div class="pet-face">
            <span class="eye left" />
            <span class="eye right" />
            <span class="mouth" />
          </div>
          <div class="pet-tail" />
        </template>
      </div>
      <div v-if="petStore.reminderActive" class="reminder-pulse" />
    </div>

    <!-- 灵盒面板：展示系统状态与快捷操作 -->
    <section v-if="showControlPanel" class="status-panel glass">
      <p class="status-label">运行状态</p>
      <p class="status-value">{{ petStore.statusText }}</p>
      <div class="control-row">
        <button class="ghost-btn" type="button" @click="handleAutostartToggle">
          {{ petStore.autostartEnabled ? "关闭" : "开启" }} 开机自启
        </button>
        <button class="ghost-btn" type="button" @click="toggleLowPower">
          {{ petStore.powerMode === "low" ? "恢复常速" : "低功耗" }}
        </button>
      </div>
      <div class="control-row">
        <button class="text-btn" type="button" @click="triggerReminder">触发提醒</button>
        <span v-if="petStore.lastUpdated" class="hint">最近同步：{{ petStore.lastUpdated }}</span>
      </div>
    </section>

    <!-- 显示器列表：实时展示 PlatformBridge 回传数据 -->
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
  padding: 18px;
  box-sizing: border-box;
  user-select: none;
}

.pet-avatar {
  position: relative;
  width: 180px;
  height: 180px;
  margin: 0 auto;
  border-radius: 120px;
  background: linear-gradient(160deg, rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0.04));
  backdrop-filter: blur(20px);
  border: 1px solid rgba(255, 255, 255, 0.15);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.25s ease;
}

.pet-avatar.dragging {
  cursor: grabbing;
}

.pet-body {
  width: 120px;
  height: 120px;
  border-radius: 50% 50% 45% 45%;
  background: radial-gradient(circle at 30% 30%, #fdfdfd, #b0b8ff);
  position: relative;
  animation: float 4s ease-in-out infinite;
}

.pet-body.has-frame {
  background: transparent;
  border-radius: 50%;
  animation: float 4s ease-in-out infinite;
}

.pet-frame {
  width: 120px;
  height: 120px;
  object-fit: contain;
  pointer-events: none;
  user-select: none;
  filter: drop-shadow(0 8px 12px rgba(29, 34, 58, 0.45));
}

.pet-face {
  position: absolute;
  top: 40%;
  left: 50%;
  transform: translateX(-50%);
  width: 70px;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.eye {
  width: 10px;
  height: 14px;
  border-radius: 50%;
  background: #2f2f2f;
  box-shadow: 0 0 6px rgba(0, 0, 0, 0.2);
}

.mouth {
  position: absolute;
  left: 50%;
  top: 22px;
  width: 26px;
  height: 10px;
  border: 2px solid #2f2f2f;
  border-color: #2f2f2f transparent transparent transparent;
  border-radius: 50%;
  transform: translateX(-50%);
}

.pet-tail {
  position: absolute;
  width: 40px;
  height: 40px;
  border-radius: 50%;
  right: -15px;
  bottom: 15px;
  background: rgba(176, 184, 255, 0.65);
  filter: blur(3px);
}

.reminder-pulse {
  position: absolute;
  inset: -12px;
  border-radius: 50%;
  border: 2px solid rgba(65, 255, 211, 0.6);
  animation: pulse 1.6s ease-in-out infinite;
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
  gap: 10px;
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

.control-row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
}

.ghost-btn {
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

.text-btn {
  border: none;
  background: transparent;
  color: rgba(255, 255, 255, 0.8);
  font-size: 12px;
  text-decoration: underline;
  cursor: pointer;
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

@keyframes float {
  0%,
  100% {
    transform: translateY(0px);
  }
  50% {
    transform: translateY(-6px);
  }
}

@keyframes pulse {
  0% {
    transform: scale(0.9);
    opacity: 0.8;
  }
  100% {
    transform: scale(1.1);
    opacity: 0.2;
  }
}
</style>
