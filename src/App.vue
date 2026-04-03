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
const REMINDER_WINDOW_SIZE = { width: 248, height: 248 };

let unlistenReminderFired = null;
let reminderTimer = null;
let bubbleTimer = null;

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
  inset: -4px;
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
  margin: 0;
  font-size: 11px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  color: #475569;
}

.bubble-title {
  margin: 4px 0;
  font-size: 16px;
  font-weight: 600;
}

.bubble-body {
  margin: 0;
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
