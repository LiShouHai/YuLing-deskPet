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
  onReminderFired,
  onReminderUpdated,
  onAutostartUpdate,
  onMonitorUpdate,
  setAutostart,
} from "./platformBridge";
import { usePetStore } from "./stores/petStore";
import { useReminderStore } from "./stores/reminderStore";
import { useMotionController } from "./motion/useMotionController";
import motionManifest from "./assets/motion/manifest.json";

const frameModules = import.meta.glob("./assets/motion/frames/*.png", {
  eager: true,
  import: "default",
});
const frameUrlMap = Object.fromEntries(
  Object.entries(frameModules).map(([key, value]) => [
    key.replace("./assets/motion/", ""),
    value,
  ])
);

// Pinia store 承载跨组件共享状态
const petStore = usePetStore();
const reminderStore = useReminderStore();
// Tauri 提供的窗口句柄，用于操控位置等特性
const tauriWindow = isTauriEnvironment ? getCurrentWindow() : null;
// 通过计算属性获取监视器列表和电源模式，保持响应式
const monitors = computed(() => petStore.monitors);
const powerMode = computed(() => petStore.powerMode);
const showReminderPanel = ref(false);
const reminderError = ref("");
// MotionController 提供当前动画状态、覆盖层强度以及状态 setter
const { state: motionState, overlayIntensity, setState } = useMotionController(powerMode);

let unlistenMonitors = null;
let unlistenAutostart = null;
let reminderTimer = null;
let unlistenReminderFired = null;
let unlistenReminderUpdated = null;

const idleFrames = motionManifest.idle?.frames ?? [];
const manualFrameIndex = ref(0);

/**
 * 解析当前状态对应的帧图片
 */
const currentFrameSrc = computed(() => {
  const frames = idleFrames;
  if (!frames.length) return "";
  const index = manualFrameIndex.value % frames.length;
  const path = frames[index];
  return frameUrlMap[path] ?? "";
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
  if (idleFrames.length > 1) {
    manualFrameIndex.value = (manualFrameIndex.value + 1) % idleFrames.length;
  }
  setState("react");
  setTimeout(() => updateMotionState(), 500);
}

/**
 * 切换提醒面板
 */
function toggleReminderPanel() {
  showReminderPanel.value = !showReminderPanel.value;
}

function formatRemindTime(timestamp) {
  return new Date(timestamp).toLocaleString();
}

async function handleReminderSubmit() {
  reminderError.value = "";
  try {
    await reminderStore.addReminder();
  } catch (error) {
    reminderError.value = error?.message ?? "创建提醒失败";
  }
}

async function handleComplete(id) {
  await reminderStore.complete(id);
}

async function handleSnooze(id) {
  await reminderStore.snooze(id, 5 * 60 * 1000);
}

async function handleDelete(id) {
  await reminderStore.remove(id);
}

function handleReminderFired(payload) {
  reminderStore.markFired(payload);
  petStore.pulseReminder(true);
  if (reminderTimer) clearTimeout(reminderTimer);
  reminderTimer = setTimeout(() => petStore.pulseReminder(false), 5000);
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
  await reminderStore.fetchReminders();

  if (isTauriEnvironment) {
    unlistenMonitors = await onMonitorUpdate((payload) => {
      petStore.setMonitors(payload ?? []);
    });

    unlistenAutostart = await onAutostartUpdate((payload) => {
      petStore.setAutostart(Boolean(payload));
    });

    unlistenReminderFired = await onReminderFired((payload) => handleReminderFired(payload));
    unlistenReminderUpdated = await onReminderUpdated(() => reminderStore.fetchReminders());
  }

  updateMotionState();
});

/**
 * 生命周期：卸载时释放监听、计时器，防止内存泄漏
 */
onBeforeUnmount(() => {
  unlistenMonitors?.();
  unlistenAutostart?.();
  unlistenReminderFired?.();
  unlistenReminderUpdated?.();
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
  <!-- 根容器：仅展示宠物本体 -->
  <div class="pet-shell">
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

    <button class="reminder-toggle" type="button" @click.stop="toggleReminderPanel">
      {{ showReminderPanel ? "收起提醒" : "提醒面板" }}
    </button>

    <section v-if="showReminderPanel" class="reminder-panel">
      <form class="reminder-form" @submit.prevent="handleReminderSubmit">
        <input
          v-model="reminderStore.composer.title"
          type="text"
          placeholder="提醒标题"
          required
        />
        <textarea
          v-model="reminderStore.composer.message"
          rows="2"
          placeholder="备注（可选）"
        />
        <input
          v-model="reminderStore.composer.remindAt"
          type="datetime-local"
          required
        />
        <button type="submit" :disabled="reminderStore.submitting">保存提醒</button>
        <p v-if="reminderError" class="reminder-error">{{ reminderError }}</p>
      </form>
      <ul class="reminder-list">
        <li v-for="item in reminderStore.items" :key="item.id">
          <div class="reminder-info">
            <strong>{{ item.title }}</strong>
            <small>{{ formatRemindTime(item.remind_at) }}</small>
            <p v-if="item.message">{{ item.message }}</p>
          </div>
          <div class="reminder-actions">
            <button type="button" @click="handleComplete(item.id)">完成</button>
            <button type="button" @click="handleSnooze(item.id)">+5分钟</button>
            <button type="button" @click="handleDelete(item.id)">删除</button>
          </div>
        </li>
        <li v-if="!reminderStore.items.length" class="placeholder">暂无提醒</li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
.pet-shell {
  position: relative;
  width: 220px;
  height: 220px;
  padding: 20px;
  box-sizing: border-box;
  user-select: none;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
}

.pet-avatar {
  position: relative;
  width: 180px;
  height: 180px;
  border-radius: 120px;
  background: linear-gradient(160deg, rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0.04));
  border: 1px solid rgba(255, 255, 255, 0.15);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.25s ease;
  box-shadow: 0 20px 45px rgba(23, 25, 35, 0.35);
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

.reminder-toggle {
  position: absolute;
  right: 12px;
  bottom: 16px;
  border: none;
  border-radius: 999px;
  padding: 4px 12px;
  font-size: 12px;
  letter-spacing: 0.08em;
  background: rgba(255, 255, 255, 0.15);
  color: #fdfdfd;
  cursor: pointer;
  transition: background 0.2s ease;
}

.reminder-toggle:hover {
  background: rgba(255, 255, 255, 0.3);
}

.reminder-panel {
  position: absolute;
  top: 210px;
  right: 0;
  width: 240px;
  padding: 14px;
  border-radius: 16px;
  background: rgba(10, 10, 15, 0.78);
  border: 1px solid rgba(255, 255, 255, 0.08);
  box-shadow: 0 15px 35px rgba(0, 0, 0, 0.35);
  color: #fdfdfd;
  backdrop-filter: blur(18px);
  z-index: 2;
}

.reminder-form {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 10px;
}

.reminder-form input,
.reminder-form textarea {
  width: 100%;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.15);
  background: rgba(255, 255, 255, 0.05);
  color: inherit;
  padding: 6px 8px;
  font-size: 12px;
}

.reminder-form button {
  align-self: flex-end;
  border: none;
  border-radius: 999px;
  padding: 4px 10px;
  background: rgba(45, 229, 189, 0.8);
  color: #081316;
  font-size: 12px;
  cursor: pointer;
}

.reminder-error {
  color: #ff9696;
  font-size: 11px;
}

.reminder-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 200px;
  overflow-y: auto;
}

.reminder-list li {
  border-radius: 12px;
  padding: 8px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.reminder-info strong {
  font-size: 13px;
}

.reminder-info small {
  font-size: 11px;
  opacity: 0.7;
}

.reminder-actions {
  display: flex;
  gap: 6px;
}

.reminder-actions button {
  flex: 1;
  border: none;
  border-radius: 8px;
  padding: 4px;
  font-size: 11px;
  cursor: pointer;
  background: rgba(255, 255, 255, 0.12);
  color: inherit;
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
