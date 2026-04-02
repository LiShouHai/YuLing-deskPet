<script setup>
/**
 * 桌宠主界面组件：
 * - 负责窗口内所有交互（拖拽、点击、双击控制面板）
 * - 承担监视器和自启动状态展示
 * - 驱动 MotionController 控制宠物动画
 * 由于采用 `<script setup>`，声明即导出，需保持结构清晰。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { getCurrentWindow, LogicalPosition } from "@tauri-apps/api/window";
import {
  fetchMonitors,
  getAutostartStatus,
  isTauriEnvironment,
  onReminderFired,
  onReminderUpdated,
  onReminderToggle,
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
// MotionController 提供当前动画状态、覆盖层强度以及状态 setter
const { state: motionState, overlayIntensity, setState } = useMotionController(powerMode);

let unlistenMonitors = null;
let unlistenAutostart = null;
let reminderTimer = null;
let unlistenReminderFired = null;
let unlistenReminderUpdated = null;
let unlistenReminderToggle = null;
const showBubble = ref(false);
const showReminderModal = ref(false);
const overlayInteractive = ref(false);
const PANEL_DEFAULT = { width: 360, height: 420 };
const PANEL_MIN = { width: 320, height: 260 };
const PANEL_MARGIN = 12;
const reminderPanelRect = reactive({
  width: PANEL_DEFAULT.width,
  height: PANEL_DEFAULT.height,
  top: PANEL_MARGIN,
  left: PANEL_MARGIN,
});
const isPanelDragging = ref(false);
const resizeHandles = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];
const reminderError = ref("");
let bubbleTimer = null;
const handleKeydown = (event) => {
  if (event.key === "Escape" && showReminderModal.value) {
    closeReminderModal();
  }
};
const handleWindowResize = () => {
  if (showReminderModal.value) {
    clampPanelWithinViewport();
  }
};

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
function handleReminderFired(payload) {
  reminderStore.markFired(payload);
  petStore.pulseReminder(true);
  if (reminderTimer) clearTimeout(reminderTimer);
  reminderTimer = setTimeout(() => petStore.pulseReminder(false), 5000);
  showBubble.value = true;
  if (bubbleTimer) clearTimeout(bubbleTimer);
  bubbleTimer = setTimeout(() => {
    showBubble.value = false;
  }, 6000);
}

function closeReminderModal() {
  showReminderModal.value = false;
  overlayInteractive.value = false;
  isPanelDragging.value = false;
}

function clampPanelWithinViewport() {
  if (typeof window === "undefined") return;
  const vw = Math.max(window.innerWidth || 0, PANEL_MIN.width + PANEL_MARGIN * 2);
  const vh = Math.max(window.innerHeight || 0, PANEL_MIN.height + PANEL_MARGIN * 2);
  const maxLeft = Math.max(vw - reminderPanelRect.width - PANEL_MARGIN, PANEL_MARGIN);
  const maxTop = Math.max(vh - reminderPanelRect.height - PANEL_MARGIN, PANEL_MARGIN);
  reminderPanelRect.left = Math.min(Math.max(reminderPanelRect.left, PANEL_MARGIN), maxLeft);
  reminderPanelRect.top = Math.min(Math.max(reminderPanelRect.top, PANEL_MARGIN), maxTop);
}

function centerReminderPanel() {
  if (typeof window === "undefined") return;
  const vw = window.innerWidth || reminderPanelRect.width;
  const vh = window.innerHeight || reminderPanelRect.height;
  reminderPanelRect.left = Math.max((vw - reminderPanelRect.width) / 2, PANEL_MARGIN);
  reminderPanelRect.top = Math.max((vh - reminderPanelRect.height) / 2, PANEL_MARGIN);
}

function resetReminderPanel() {
  reminderPanelRect.width = PANEL_DEFAULT.width;
  reminderPanelRect.height = PANEL_DEFAULT.height;
  centerReminderPanel();
}

async function openReminderModal() {
  resetReminderPanel();
  showReminderModal.value = true;
  overlayInteractive.value = false;
  await nextTick();
  centerReminderPanel();
  if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
    window.requestAnimationFrame(() => {
      overlayInteractive.value = true;
    });
  } else {
    overlayInteractive.value = true;
  }
}

function beginPanelDrag(event) {
  if (event.button !== 0) return;
  event.preventDefault();
  isPanelDragging.value = true;
  const startPointer = { x: event.clientX, y: event.clientY };
  const startRect = { left: reminderPanelRect.left, top: reminderPanelRect.top };

  const move = (moveEvent) => {
    const dx = moveEvent.clientX - startPointer.x;
    const dy = moveEvent.clientY - startPointer.y;
    reminderPanelRect.left = startRect.left + dx;
    reminderPanelRect.top = startRect.top + dy;
    clampPanelWithinViewport();
  };

  const up = () => {
    isPanelDragging.value = false;
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
  };

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up, { once: true });
}

function handleHeaderPointerDown(event) {
  const target = event.target;
  if (target && typeof target.closest === "function") {
    if (target.closest("button")) {
      return;
    }
  }
  beginPanelDrag(event);
}

function beginPanelResize(event, direction) {
  if (event.button !== 0) return;
  event.preventDefault();
  const startPointer = { x: event.clientX, y: event.clientY };
  const startRect = {
    width: reminderPanelRect.width,
    height: reminderPanelRect.height,
    left: reminderPanelRect.left,
    top: reminderPanelRect.top,
  };

  const move = (moveEvent) => {
    const dx = moveEvent.clientX - startPointer.x;
    const dy = moveEvent.clientY - startPointer.y;

    if (direction.includes("e")) {
      reminderPanelRect.width = Math.max(PANEL_MIN.width, startRect.width + dx);
    }
    if (direction.includes("s")) {
      reminderPanelRect.height = Math.max(PANEL_MIN.height, startRect.height + dy);
    }
    if (direction.includes("w")) {
      const nextWidth = Math.max(PANEL_MIN.width, startRect.width - dx);
      const delta = startRect.width - nextWidth;
      reminderPanelRect.width = nextWidth;
      reminderPanelRect.left = startRect.left + delta;
    }
    if (direction.includes("n")) {
      const nextHeight = Math.max(PANEL_MIN.height, startRect.height - dy);
      const delta = startRect.height - nextHeight;
      reminderPanelRect.height = nextHeight;
      reminderPanelRect.top = startRect.top + delta;
    }
    clampPanelWithinViewport();
  };

  const up = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
  };

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up, { once: true });
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

  if (typeof window !== "undefined") {
    window.addEventListener("keydown", handleKeydown);
    window.addEventListener("resize", handleWindowResize);
  }

  if (isTauriEnvironment) {
    unlistenMonitors = await onMonitorUpdate((payload) => {
      petStore.setMonitors(payload ?? []);
    });

    unlistenAutostart = await onAutostartUpdate((payload) => {
      petStore.setAutostart(Boolean(payload));
    });

    unlistenReminderFired = await onReminderFired((payload) => handleReminderFired(payload));
    unlistenReminderUpdated = await onReminderUpdated(() => reminderStore.fetchReminders());
    unlistenReminderToggle = await onReminderToggle(() => {
      console.info("收到托盘提醒列表事件，开启提醒面板");
      openReminderModal();
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
  unlistenReminderFired?.();
  unlistenReminderUpdated?.();
  unlistenReminderToggle?.();
  if (reminderTimer) clearTimeout(reminderTimer);
  if (bubbleTimer) clearTimeout(bubbleTimer);
  if (typeof window !== "undefined") {
    window.removeEventListener("keydown", handleKeydown);
    window.removeEventListener("resize", handleWindowResize);
  }
});

// 监听关键状态，变化时更新动画状态
watch(
  () => [petStore.dragging, petStore.reminderActive, petStore.powerMode],
  () => updateMotionState(),
  { deep: true }
);
</script>

<template>
  <div class="pet-shell">
    <div class="pet-stage">
      <Transition name="bubble">
        <div v-if="showBubble && reminderStore.lastFired" class="reminder-bubble">
          <p class="bubble-label">提醒</p>
          <p class="bubble-title">{{ reminderStore.lastFired.title }}</p>
          <p class="bubble-body">
            {{ reminderStore.lastFired.message || "记得活动一下身体～" }}
          </p>
        </div>
      </Transition>
      <div
        class="pet-avatar"
        :class="[`state-${motionState}`, { dragging: petStore.dragging }]"
        :style="{
          boxShadow: `0 25px 45px rgba(23, 25, 35, ${0.35 + overlayIntensity * 0.25})`,
        }"
        @pointerdown="beginDrag"
        @click="handleAvatarClick"
      >
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
    </div>
    <Transition name="modal">
      <div
        v-if="showReminderModal"
        class="reminder-overlay"
        :class="{ 'is-live': overlayInteractive }"
        @click.self="overlayInteractive && closeReminderModal()"
      >
        <section
          class="reminder-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby="reminder-panel-title"
          :class="{ dragging: isPanelDragging }"
          :style="{
            width: reminderPanelRect.width + 'px',
            height: reminderPanelRect.height + 'px',
            top: reminderPanelRect.top + 'px',
            left: reminderPanelRect.left + 'px',
          }"
          @pointerdown.stop
        >
          <div class="modal-hero-bar" />
          <header class="modal-header" @pointerdown="handleHeaderPointerDown">
            <div>
              <p class="modal-label">提醒列表</p>
              <h3 id="reminder-panel-title">{{ reminderStore.items.length || "0" }} 项任务</h3>
            </div>
            <button type="button" class="icon-btn" @click="closeReminderModal" aria-label="关闭提醒面板">
              ✕
            </button>
          </header>
          <form class="modal-form" @submit.prevent="handleReminderSubmit">
            <label class="field">
              <span>提醒标题</span>
              <input
                v-model="reminderStore.composer.title"
                type="text"
                placeholder="喝水、伸展"
                required
              />
            </label>
            <label class="field">
              <span>备注（可选）</span>
              <textarea
                v-model="reminderStore.composer.message"
                rows="2"
                placeholder="补充提示语"
              />
            </label>
            <label class="field">
              <span>提醒时间</span>
              <input
                v-model="reminderStore.composer.remindAt"
                type="datetime-local"
                required
              />
            </label>
            <button class="primary-btn" type="submit" :disabled="reminderStore.submitting">
              {{ reminderStore.submitting ? "保存中…" : "保存提醒" }}
            </button>
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
                <button type="button" @click="handleSnooze(item.id)">延后5分钟</button>
                <button type="button" @click="handleDelete(item.id)">删除</button>
              </div>
            </li>
            <li v-if="!reminderStore.items.length" class="placeholder">暂无提醒，使用上方表单创建。</li>
          </ul>
          <div
            v-for="handle in resizeHandles"
            :key="handle"
            class="resize-handle"
            :class="`handle-${handle}`"
            @pointerdown.prevent="beginPanelResize($event, handle)"
          />
        </section>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.pet-shell {
  position: relative;
  width: fit-content;
  height: fit-content;
  padding: 0;
  user-select: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
}

.pet-stage {
  position: relative;
  display: inline-flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 0;
  width: fit-content;
  height: fit-content;
  border-radius: 16px;
  background: transparent;
  border: none;
  box-shadow: none;
}

.pet-avatar {
  position: relative;
  width: 168px;
  height: 168px;
  border-radius: 50%;
  background: transparent;
  border: none;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.25s ease;
  cursor: grab;
  touch-action: none;
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

.reminder-bubble {
  position: absolute;
  top: 0;
  left: 50%;
  transform: translate(-50%, -110%);
  width: 220px;
  padding: 12px 16px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.95);
  color: #0d1220;
  box-shadow: 0 16px 40px rgba(5, 6, 11, 0.4);
  border: 1px solid rgba(255, 255, 255, 0.6);
}

.reminder-bubble::after {
  content: "";
  position: absolute;
  bottom: -8px;
  left: 50%;
  transform: translateX(-50%) rotate(45deg);
  width: 16px;
  height: 16px;
  background: inherit;
  border-bottom: 1px solid rgba(255, 255, 255, 0.6);
  border-right: 1px solid rgba(255, 255, 255, 0.6);
}

.bubble-label {
  font-size: 11px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  color: #475569;
}

.bubble-title {
  font-size: 16px;
  font-weight: 600;
  margin: 4px 0;
}

.bubble-body {
  font-size: 13px;
  opacity: 0.8;
}

.bubble-enter-active,
.bubble-leave-active {
  transition: opacity 0.25s ease, transform 0.25s ease;
}

.bubble-enter-from,
.bubble-leave-to {
  opacity: 0;
  transform: translate(-50%, -120%) scale(0.92);
}

.reminder-overlay {
  position: fixed;
  inset: 0;
  background: rgba(4, 6, 12, 0.6);
  backdrop-filter: blur(14px) saturate(140%);
  z-index: 1200;
  transition: opacity 0.25s ease;
}

.reminder-overlay:not(.is-live) {
  pointer-events: none;
}

.reminder-modal {
  position: absolute;
  padding: 24px;
  border-radius: 28px;
  background: linear-gradient(180deg, rgba(18, 23, 37, 0.95), rgba(7, 10, 18, 0.98));
  border: 1px solid rgba(255, 255, 255, 0.08);
  box-shadow: 0 35px 80px rgba(2, 3, 6, 0.75);
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow: hidden;
  animation: reminder-pop 0.25s ease;
  transition: box-shadow 0.2s ease;
}

.reminder-modal.dragging {
  box-shadow: 0 20px 45px rgba(0, 0, 0, 0.6);
  cursor: grabbing;
}

.modal-hero-bar {
  width: 60%;
  height: 6px;
  border-radius: 999px;
  background: linear-gradient(90deg, #f97316, #fbbf24, #60a5fa);
  opacity: 0.9;
  animation: hero-flow 0.4s ease forwards;
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  user-select: none;
  cursor: grab;
}

.reminder-modal.dragging .modal-header {
  cursor: grabbing;
}

.modal-label {
  font-size: 12px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  opacity: 0.6;
}

.modal-header h3 {
  margin: 4px 0 0;
  font-size: 20px;
  font-weight: 600;
}

.icon-btn {
  border: none;
  width: 28px;
  height: 28px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.12);
  color: inherit;
  cursor: pointer;
}

.modal-form {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.75);
}

.field input,
.field textarea {
  width: 100%;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.15);
  background: rgba(255, 255, 255, 0.05);
  color: inherit;
  padding: 6px 8px;
  font-size: 12px;
}

.primary-btn {
  align-self: flex-end;
  border: none;
  border-radius: 999px;
  padding: 4px 12px;
  background: linear-gradient(120deg, #f97316, #fbbf24);
  color: #0b0d11;
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
  flex: 1;
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

.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.25s ease, transform 0.25s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
  transform: scale(0.96);
}

.resize-handle {
  position: absolute;
  width: 14px;
  height: 14px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.16);
  border: 1px solid rgba(255, 255, 255, 0.25);
  backdrop-filter: blur(6px);
  box-shadow: 0 6px 15px rgba(0, 0, 0, 0.35);
}

.resize-handle:hover {
  background: rgba(255, 255, 255, 0.35);
}

.handle-n,
.handle-s {
  left: 50%;
  transform: translateX(-50%);
  cursor: ns-resize;
}

.handle-n {
  top: -8px;
}

.handle-s {
  bottom: -8px;
}

.handle-e,
.handle-w {
  top: 50%;
  transform: translateY(-50%);
  cursor: ew-resize;
}

.handle-e {
  right: -8px;
}

.handle-w {
  left: -8px;
}

.handle-ne {
  top: -8px;
  right: -8px;
  cursor: nesw-resize;
}

.handle-nw {
  top: -8px;
  left: -8px;
  cursor: nwse-resize;
}

.handle-se {
  bottom: -8px;
  right: -8px;
  cursor: nwse-resize;
}

.handle-sw {
  bottom: -8px;
  left: -8px;
  cursor: nesw-resize;
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

@keyframes reminder-pop {
  0% {
    opacity: 0;
    transform: translateY(12px) scale(0.96);
  }
  100% {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

@keyframes hero-flow {
  from {
    width: 40%;
    opacity: 0.4;
  }
  to {
    width: 100%;
    opacity: 1;
  }
}
</style>

.sheet-enter-active,
.sheet-leave-active {
  transition: opacity 0.25s ease, transform 0.25s ease;
}

.sheet-enter-from,
.sheet-leave-to {
  opacity: 0;
  transform: translateY(12px) scale(0.98);
}

.reminder-toast {
  width: 100%;
  padding: 12px;
  border-radius: 16px;
  background: rgba(23, 32, 44, 0.8);
  border: 1px solid rgba(255, 255, 255, 0.05);
  text-align: center;
}

.toast-label {
  font-size: 11px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  opacity: 0.6;
  margin-bottom: 4px;
}

.toast-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 2px;
}
