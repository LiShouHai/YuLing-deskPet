<script setup>
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isTauriEnvironment,
  onReminderFired,
  onReminderUpdated,
} from "../platformBridge";
import { useReminderStore } from "../stores/reminderStore";

const reminderStore = useReminderStore();
const tauriWindow = isTauriEnvironment ? getCurrentWindow() : null;
const errorMessage = ref("");
const statusMessage = ref("正在同步提醒…");

let unlistenReminderUpdated = null;
let unlistenReminderFired = null;
let unlistenCloseRequested = null;

const pendingCount = computed(
  () => reminderStore.items.filter((item) => item.status === "pending").length
);

const nextReminderText = computed(() => {
  const nextItem = reminderStore.items.find((item) =>
    ["pending", "notified"].includes(item.status)
  );
  return nextItem ? formatDisplayTime(nextItem.remind_at) : "暂无待提醒事项";
});

function formatDisplayTime(timestamp) {
  return new Date(timestamp).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatDateTimeInput(timestamp) {
  const date = new Date(timestamp);
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

function ensureDefaultReminderTime() {
  if (!reminderStore.composer.remindAt) {
    reminderStore.composer.remindAt = formatDateTimeInput(Date.now() + 30 * 60_000);
  }
}

function formatStatus(status) {
  switch (status) {
    case "pending":
      return "待触发";
    case "notified":
      return "已触发";
    case "completed":
      return "已完成";
    default:
      return status;
  }
}

async function syncReminders() {
  statusMessage.value = "正在同步提醒…";
  await reminderStore.fetchReminders();
  statusMessage.value = reminderStore.items.length
    ? `已加载 ${reminderStore.items.length} 条提醒`
    : "还没有任何提醒";
  ensureDefaultReminderTime();
}

async function submitReminder() {
  errorMessage.value = "";
  try {
    await reminderStore.addReminder();
    ensureDefaultReminderTime();
    statusMessage.value = "提醒已保存";
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : "保存提醒失败";
  }
}

async function completeItem(id) {
  errorMessage.value = "";
  try {
    await reminderStore.complete(id);
    statusMessage.value = "提醒已完成";
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : "完成提醒失败";
  }
}

async function snoozeItem(id) {
  errorMessage.value = "";
  try {
    await reminderStore.snooze(id, 10 * 60_000);
    statusMessage.value = "已延后 10 分钟";
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : "延后提醒失败";
  }
}

async function removeItem(id) {
  errorMessage.value = "";
  try {
    await reminderStore.remove(id);
    statusMessage.value = "提醒已删除";
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : "删除提醒失败";
  }
}

async function hideWindow() {
  if (!tauriWindow) return;
  try {
    await tauriWindow.hide();
  } catch (error) {
    console.error("隐藏提醒窗口失败", error);
    statusMessage.value = "关闭提醒窗口失败";
  }
}

function handleKeydown(event) {
  if (event.key === "Escape") {
    void hideWindow();
  }
}

onMounted(async () => {
  ensureDefaultReminderTime();
  await syncReminders();

  if (tauriWindow) {
    unlistenCloseRequested = await tauriWindow.onCloseRequested(async (event) => {
      event.preventDefault();
      await hideWindow();
    });
  }

  if (isTauriEnvironment) {
    unlistenReminderUpdated = await onReminderUpdated(async () => {
      await syncReminders();
    });

    unlistenReminderFired = await onReminderFired(async (payload) => {
      reminderStore.markFired(payload);
      statusMessage.value = `提醒触发：${payload.title}`;
      await syncReminders();
    });
  }

  window.addEventListener("keydown", handleKeydown);
});

onBeforeUnmount(() => {
  unlistenReminderUpdated?.();
  unlistenReminderFired?.();
  unlistenCloseRequested?.();
  window.removeEventListener("keydown", handleKeydown);
});
</script>

<template>
  <div class="reminder-shell">
    <section class="reminder-window" role="dialog" aria-labelledby="reminder-title">
      <div class="hero-bar" />

      <header class="window-header">
        <div class="header-copy">
          <p class="eyebrow">Reminder Console</p>
          <h1 id="reminder-title">提醒列表</h1>
          <p class="subtitle">下一条：{{ nextReminderText }}</p>
        </div>
        <button class="close-btn" aria-label="关闭提醒窗口" @click="hideWindow">×</button>
      </header>

      <section class="summary-panel" aria-label="提醒统计">
        <article class="summary-card">
          <span>待触发</span>
          <strong>{{ pendingCount }}</strong>
        </article>
        <article class="summary-card">
          <span>总数</span>
          <strong>{{ reminderStore.items.length }}</strong>
        </article>
      </section>

      <p class="status-line">{{ statusMessage }}</p>

      <section v-if="reminderStore.lastFired" class="fired-banner">
        <p class="fired-label">最近触发</p>
        <strong>{{ reminderStore.lastFired.title }}</strong>
        <p>{{ reminderStore.lastFired.message || "记得处理这条提醒。" }}</p>
      </section>

      <form class="composer-panel" @submit.prevent="submitReminder">
        <div class="panel-heading">
          <h2>新建提醒</h2>
          <button
            type="button"
            class="ghost-btn"
            :disabled="reminderStore.loading"
            @click="syncReminders"
          >
            刷新
          </button>
        </div>

        <label class="field">
          <span>标题</span>
          <input
            v-model="reminderStore.composer.title"
            type="text"
            maxlength="40"
            placeholder="比如：喝水、开会、站起来活动"
          />
        </label>

        <label class="field">
          <span>备注</span>
          <textarea
            v-model="reminderStore.composer.message"
            rows="3"
            maxlength="120"
            placeholder="可选，补充一点上下文"
          />
        </label>

        <label class="field">
          <span>提醒时间</span>
          <input v-model="reminderStore.composer.remindAt" type="datetime-local" />
        </label>

        <p v-if="errorMessage" class="error-text">{{ errorMessage }}</p>

        <button class="primary-btn" type="submit" :disabled="reminderStore.submitting">
          {{ reminderStore.submitting ? "保存中…" : "保存提醒" }}
        </button>
      </form>

      <section class="list-panel">
        <div class="panel-heading">
          <h2>提醒列表</h2>
          <span class="list-caption">支持完成、延后 10 分钟、删除</span>
        </div>

        <div v-if="!reminderStore.loading && !reminderStore.items.length" class="empty-state">
          还没有提醒，先创建一条吧。
        </div>

        <ul v-else class="reminder-list">
          <li
            v-for="item in reminderStore.items"
            :key="item.id"
            class="reminder-card"
            :class="`status-${item.status}`"
          >
            <div class="card-header">
              <div class="card-copy">
                <strong>{{ item.title }}</strong>
                <p>{{ item.message || "无备注" }}</p>
              </div>
              <span class="status-badge" :class="`is-${item.status}`">
                {{ formatStatus(item.status) }}
              </span>
            </div>

            <p class="remind-at">{{ formatDisplayTime(item.remind_at) }}</p>

            <div class="card-actions">
              <button
                class="action-btn"
                :disabled="item.status === 'completed' || reminderStore.loading"
                @click="completeItem(item.id)"
              >
                完成
              </button>
              <button
                class="action-btn"
                :disabled="item.status === 'completed' || reminderStore.loading"
                @click="snoozeItem(item.id)"
              >
                延后
              </button>
              <button
                class="action-btn danger"
                :disabled="reminderStore.loading"
                @click="removeItem(item.id)"
              >
                删除
              </button>
            </div>
          </li>
        </ul>
      </section>
    </section>
  </div>
</template>

<style scoped>
.reminder-shell {
  width: 100%;
  height: 100%;
  padding: 18px;
  background:
    radial-gradient(circle at top left, rgba(245, 158, 11, 0.2), transparent 34%),
    radial-gradient(circle at bottom right, rgba(59, 130, 246, 0.22), transparent 38%),
    linear-gradient(180deg, rgba(8, 10, 18, 0.72), rgba(4, 6, 12, 0.9));
}

.reminder-window {
  width: 100%;
  height: 100%;
  min-height: 0;
  border-radius: 30px;
  padding: 18px;
  overflow-y: scroll;
  display: flex;
  flex-direction: column;
  gap: 14px;
  color: #f8fafc;
  background: linear-gradient(180deg, rgba(12, 17, 31, 0.96), rgba(4, 7, 15, 0.98));
  border: 1px solid rgba(255, 255, 255, 0.08);
  box-shadow: 0 26px 60px rgba(1, 4, 9, 0.55);
  backdrop-filter: blur(20px) saturate(145%);
}

.hero-bar {
  width: 42%;
  height: 6px;
  border-radius: 999px;
  background: linear-gradient(90deg, #fb7185, #f59e0b, #38bdf8);
  box-shadow: 0 0 18px rgba(251, 113, 133, 0.35);
}

.window-header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
}

.header-copy h1,
.panel-heading h2 {
  margin: 0;
}

.eyebrow {
  margin: 0 0 6px;
  font-size: 11px;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: rgba(248, 250, 252, 0.55);
}

.subtitle,
.status-line,
.fired-banner p,
.card-copy p,
.list-caption {
  margin: 0;
  color: rgba(226, 232, 240, 0.72);
}

.subtitle {
  margin-top: 6px;
  font-size: 13px;
}

.close-btn {
  width: 32px;
  height: 32px;
  border-radius: 999px;
  font-size: 22px;
  line-height: 1;
  color: rgba(248, 250, 252, 0.86);
  background: rgba(255, 255, 255, 0.08);
}

.close-btn:hover,
.ghost-btn:hover,
.action-btn:hover {
  background: rgba(255, 255, 255, 0.14);
}

.summary-panel {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.summary-card,
.fired-banner,
.composer-panel,
.list-panel {
  border-radius: 22px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
}

.summary-card {
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.summary-card span {
  font-size: 12px;
  color: rgba(226, 232, 240, 0.72);
}

.summary-card strong {
  font-size: 26px;
  font-weight: 700;
}

.status-line {
  padding: 0 4px;
  font-size: 12px;
}

.fired-banner {
  padding: 12px 14px;
  background: linear-gradient(135deg, rgba(251, 113, 133, 0.12), rgba(56, 189, 248, 0.08));
}

.fired-label {
  margin-bottom: 6px;
  font-size: 11px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
}

.composer-panel,
.list-panel {
  padding: 14px;
}

.composer-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.list-panel {
  flex: 1;
  min-height: 180px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.panel-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.ghost-btn {
  padding: 6px 12px;
  border-radius: 999px;
  font-size: 12px;
  background: rgba(255, 255, 255, 0.08);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: rgba(226, 232, 240, 0.82);
}

.field input,
.field textarea {
  width: 100%;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 14px;
  padding: 10px 12px;
  color: inherit;
  background: rgba(15, 23, 42, 0.62);
  outline: none;
}

.field input:focus,
.field textarea:focus {
  border-color: rgba(56, 189, 248, 0.6);
  box-shadow: 0 0 0 3px rgba(56, 189, 248, 0.12);
}

.field textarea {
  resize: none;
}

.error-text {
  margin: 0;
  font-size: 12px;
  color: #fda4af;
}

.primary-btn {
  align-self: flex-end;
  padding: 10px 16px;
  border-radius: 999px;
  color: #04111f;
  font-weight: 700;
  background: linear-gradient(120deg, #fb7185, #f59e0b);
}

.primary-btn:disabled,
.ghost-btn:disabled,
.action-btn:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 18px;
  color: rgba(226, 232, 240, 0.6);
  background: rgba(255, 255, 255, 0.03);
}

.reminder-list {
  list-style: none;
  margin: 0;
  padding: 0 4px 0 0;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow-y: auto;
}

.reminder-card {
  padding: 12px;
  border-radius: 18px;
  background: rgba(15, 23, 42, 0.58);
  border: 1px solid rgba(255, 255, 255, 0.08);
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.reminder-card.status-notified {
  border-color: rgba(251, 146, 60, 0.45);
}

.reminder-card.status-completed {
  opacity: 0.72;
}

.card-header {
  display: flex;
  justify-content: space-between;
  gap: 10px;
}

.card-copy strong {
  display: block;
  margin-bottom: 4px;
  font-size: 15px;
}

.card-copy p {
  font-size: 12px;
}

.status-badge {
  flex-shrink: 0;
  align-self: flex-start;
  padding: 4px 8px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  background: rgba(255, 255, 255, 0.08);
}

.status-badge.is-pending {
  color: #fde68a;
}

.status-badge.is-notified {
  color: #fdba74;
}

.status-badge.is-completed {
  color: #86efac;
}

.remind-at {
  margin: 0;
  font-size: 12px;
  color: rgba(226, 232, 240, 0.72);
}

.card-actions {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}

.action-btn {
  padding: 8px 0;
  border-radius: 12px;
  font-size: 12px;
  background: rgba(255, 255, 255, 0.08);
}

.action-btn.danger {
  color: #fda4af;
}
</style>
