<script setup>
/**
 * 宠物主窗口：
 * - 仅负责宠物显示、拖拽与提醒气泡反馈
 * - 提醒表单/列表已迁移到独立 reminder 窗口
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { isTauriEnvironment, onReminderFired } from "./platformBridge";
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

const petStore = usePetStore();
const reminderStore = useReminderStore();
const tauriWindow = isTauriEnvironment ? getCurrentWindow() : null;
const powerMode = computed(() => petStore.powerMode);
const { state: motionState, overlayIntensity, setState } = useMotionController(powerMode);

const idleFrames = motionManifest.idle?.frames ?? [];
const manualFrameIndex = ref(0);
const showBubble = ref(false);
const COMPACT_WINDOW_SIZE = { width: 168, height: 168 };
const REMINDER_WINDOW_SIZE = { width: 236, height: 252 };
const DRAG_THRESHOLD = 6;

let unlistenReminderFired = null;
let reminderTimer = null;
let bubbleTimer = null;
let suppressClickUntil = 0;

const currentFrameSrc = computed(() => {
  if (!idleFrames.length) return "";
  const frameIndex = manualFrameIndex.value % idleFrames.length;
  const path = idleFrames[frameIndex];
  return frameUrlMap[path] ?? "";
});

async function setMainWindowSize(nextSize) {
  if (!tauriWindow) return;

  try {
    const scaleFactor = await tauriWindow.scaleFactor();
    const [currentPos, currentSize] = await Promise.all([
      tauriWindow.outerPosition(),
      tauriWindow.outerSize(),
    ]);
    const logicalPos = currentPos.toLogical(scaleFactor);
    const logicalSize = currentSize.toLogical(scaleFactor);
    const widthChanged = Math.round(logicalSize.width) !== nextSize.width;
    const heightChanged = Math.round(logicalSize.height) !== nextSize.height;

    if (!widthChanged && !heightChanged) return;

    const anchorCenterX = logicalPos.x + logicalSize.width / 2;
    const anchorBottomY = logicalPos.y + logicalSize.height;
    const nextPosition = new LogicalPosition(
      Math.round(anchorCenterX - nextSize.width / 2),
      Math.round(anchorBottomY - nextSize.height)
    );

    await tauriWindow.setSize(new LogicalSize(nextSize.width, nextSize.height));
    await tauriWindow.setPosition(nextPosition);
  } catch (error) {
    console.error("调整主窗口尺寸失败", error);
  }
}

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

function handleAvatarClick() {
  const now = window.performance?.now?.() ?? Date.now();
  // 拖拽结束后浏览器可能仍会补发 click，这里直接忽略。
  if (now < suppressClickUntil) return;

  if (idleFrames.length > 1) {
    manualFrameIndex.value = (manualFrameIndex.value + 1) % idleFrames.length;
  }
  setState("react");
  window.setTimeout(() => updateMotionState(), 500);
}

function handleReminderFired(payload) {
  reminderStore.markFired(payload);
  petStore.pulseReminder(true);
  void setMainWindowSize(REMINDER_WINDOW_SIZE);

  if (reminderTimer) clearTimeout(reminderTimer);
  reminderTimer = window.setTimeout(() => {
    petStore.pulseReminder(false);
  }, 5000);

  showBubble.value = true;
  if (bubbleTimer) clearTimeout(bubbleTimer);
  bubbleTimer = window.setTimeout(() => {
    showBubble.value = false;
    void setMainWindowSize(COMPACT_WINDOW_SIZE);
  }, 6000);
}

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

  return new Promise((resolve) => {
    let didDrag = false;

    const move = (moveEvent) => {
      const dx = moveEvent.screenX - startMouse.x;
      const dy = moveEvent.screenY - startMouse.y;
      const movedEnough = Math.hypot(dx, dy) >= DRAG_THRESHOLD;

      if (!didDrag && !movedEnough) return;
      if (!didDrag) {
        didDrag = true;
        petStore.setDragging(true);
        setState("drag");
      }

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
      resolve(didDrag);
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
  });
}

async function beginDrag(event) {
  if (!tauriWindow) {
    console.warn("Tauri 窗口句柄不可用，跳过拖拽");
    return;
  }

  event.preventDefault();

  try {
    const didDrag = await fallbackManualDrag(event);
    if (didDrag) {
      suppressClickUntil = (window.performance?.now?.() ?? Date.now()) + 250;
    }
  } finally {
    if (petStore.dragging) {
      petStore.setDragging(false);
    }
    updateMotionState();
  }
}

onMounted(async () => {
  void setMainWindowSize(COMPACT_WINDOW_SIZE);
  if (isTauriEnvironment) {
    unlistenReminderFired = await onReminderFired((payload) => handleReminderFired(payload));
  }

  updateMotionState();
});

onBeforeUnmount(() => {
  unlistenReminderFired?.();
  if (reminderTimer) clearTimeout(reminderTimer);
  if (bubbleTimer) clearTimeout(bubbleTimer);
});

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
          <span class="bubble-signal" aria-hidden="true" />
          <p class="bubble-label">signal</p>
          <p class="bubble-title">{{ reminderStore.lastFired.title }}</p>
          <p class="bubble-body">
            {{ reminderStore.lastFired.message || "记得处理一下刚到的提醒。" }}
          </p>
        </div>
      </Transition>

      <div
        class="pet-avatar"
        :class="[`state-${motionState}`, { dragging: petStore.dragging }]"
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
  </div>
</template>

<style scoped>
.pet-shell {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  user-select: none;
  background: transparent;
  overflow: visible;
  position: relative;
}

.pet-stage {
  position: relative;
  width: 168px;
  height: 168px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  background: transparent;
  overflow: visible;
}

.pet-avatar {
  position: relative;
  width: 168px;
  height: 168px;
  border-radius: 50%;
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

.pet-avatar.state-drag {
  transform: scale(1.03);
}

.pet-avatar.state-react .pet-body,
.pet-avatar.state-reminderPulse .pet-body {
  transform: scale(1.02);
}

.pet-avatar.state-sleep .pet-body {
  opacity: 0.92;
  filter: saturate(0.82);
}

.pet-body {
  width: 156px;
  height: 156px;
  position: relative;
  overflow: hidden;
  border-radius: 50% 50% 45% 45%;
  background: radial-gradient(circle at 30% 30%, #fdfdfd, #b0b8ff);
  transition: transform 0.25s ease, filter 0.25s ease, opacity 0.25s ease;
}

.pet-body.has-frame {
  width: 164px;
  height: 164px;
  background: transparent;
  border-radius: 0;
  overflow: visible;
  display: flex;
  align-items: flex-end;
  justify-content: center;
}

.pet-frame {
  width: auto;
  height: 100%;
  max-width: 100%;
  display: block;
  pointer-events: none;
  user-select: none;
  filter: drop-shadow(0 14px 18px rgba(20, 23, 32, 0.42));
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
  right: -15px;
  bottom: 15px;
  width: 40px;
  height: 40px;
  border-radius: 50%;
  background: rgba(176, 184, 255, 0.65);
  filter: blur(3px);
}

.reminder-pulse {
  position: absolute;
  inset: -7px;
  border-radius: 50%;
  border: 2px solid rgba(65, 255, 211, 0.52);
  box-shadow:
    0 0 0 8px rgba(65, 255, 211, 0.06),
    0 0 26px rgba(65, 255, 211, 0.18);
  animation: pulse 1.5s cubic-bezier(0.22, 1, 0.36, 1) infinite;
}

.reminder-bubble {
  position: absolute;
  top: -84px;
  right: -6px;
  width: 166px;
  padding: 12px 14px 13px;
  border-radius: 22px;
  color: #12202b;
  background:
    linear-gradient(180deg, rgba(254, 250, 241, 0.98), rgba(245, 239, 227, 0.96));
  box-shadow:
    0 18px 42px rgba(4, 6, 12, 0.34),
    0 0 0 1px rgba(255, 255, 255, 0.34);
  pointer-events: none;
}

.reminder-bubble::after {
  content: "";
  position: absolute;
  bottom: -8px;
  right: 28px;
  transform: rotate(45deg);
  width: 16px;
  height: 16px;
  background: inherit;
  border-radius: 4px;
}

.bubble-signal {
  display: inline-flex;
  width: 8px;
  height: 8px;
  margin-bottom: 8px;
  border-radius: 50%;
  background: linear-gradient(180deg, #f0b257, #3fd0b7);
  box-shadow: 0 0 0 5px rgba(63, 208, 183, 0.12);
}

.bubble-label {
  margin: 0;
  font-size: 10px;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: #667782;
}

.bubble-title {
  margin: 2px 0 5px;
  font-size: 22px;
  font-weight: 800;
  line-height: 1.06;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.bubble-body {
  margin: 0;
  font-size: 13px;
  line-height: 1.35;
  color: #4b5b65;
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.bubble-enter-active,
.bubble-leave-active {
  transition:
    opacity 0.28s ease,
    transform 0.32s cubic-bezier(0.22, 1, 0.36, 1),
    filter 0.28s ease;
}

.bubble-enter-from,
.bubble-leave-to {
  opacity: 0;
  transform: translate3d(6px, -12px, 0) scale(0.95);
  filter: blur(10px);
}

@keyframes pulse {
  0% {
    transform: scale(0.92);
    opacity: 0.78;
  }
  70% {
    transform: scale(1.06);
    opacity: 0.26;
  }
  100% {
    transform: scale(1.1);
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .pet-avatar,
  .pet-body,
  .reminder-bubble,
  .bubble-enter-active,
  .bubble-leave-active,
  .reminder-pulse {
    transition: none !important;
    animation: none !important;
  }
}
</style>
