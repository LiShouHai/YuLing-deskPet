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
const composerOpen = ref(false);
const completedOpen = ref(false);
const errorMessage = ref("");
const statusMessage = ref("正在同步提醒…");

let unlistenReminderUpdated = null;
let unlistenReminderFired = null;
let unlistenCloseRequested = null;

const activeItems = computed(() =>
  reminderStore.items.filter((item) => ["pending", "notified"].includes(item.status))
);

const completedItems = computed(() =>
  reminderStore.items.filter((item) => item.status === "completed")
);

const pendingCount = computed(
  () => reminderStore.items.filter((item) => item.status === "pending").length
);

const notifiedCount = computed(
  () => reminderStore.items.filter((item) => item.status === "notified").length
);

const nextReminderText = computed(() => {
  const nextItem = activeItems.value[0];
  return nextItem ? formatDisplayTime(nextItem.remind_at) : "暂无待提醒事项";
});

const recentSignal = computed(
  () => reminderStore.lastFired ?? reminderStore.items.find((item) => item.status === "notified") ?? null
);

const recentSignalText = computed(() => {
  if (!recentSignal.value) return "";
  return `最近信号 ${recentSignal.value.title}`;
});

const headerMessage = computed(() => errorMessage.value || statusMessage.value);

const headerTone = computed(() => {
  if (errorMessage.value) return "is-error";
  if (recentSignal.value) return "is-live";
  return "is-neutral";
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

function getCardEyebrow(status) {
  return status === "notified" ? "刚刚到点" : "等待触发";
}

function toggleComposer() {
  composerOpen.value = !composerOpen.value;
  if (composerOpen.value) ensureDefaultReminderTime();
}

function toggleCompleted() {
  completedOpen.value = !completedOpen.value;
}

async function syncReminders() {
  statusMessage.value = "正在同步提醒…";
  await reminderStore.fetchReminders();

  statusMessage.value = activeItems.value.length
    ? `待处理 ${activeItems.value.length} 条提醒`
    : reminderStore.items.length
      ? "当前没有待处理提醒"
      : "还没有任何提醒";

  ensureDefaultReminderTime();
}

async function submitReminder() {
  errorMessage.value = "";
  try {
    await reminderStore.addReminder();
    ensureDefaultReminderTime();
    statusMessage.value = "提醒已保存";
    composerOpen.value = false;
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
  composerOpen.value = !activeItems.value.length;

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
      <header class="window-header">
        <div class="header-copy">
          <p class="eyebrow">Pet Dock</p>
          <h1 id="reminder-title">提醒列表</h1>
          <p class="subtitle">下一条：{{ nextReminderText }}</p>
        </div>

        <div class="header-tools">
          <p class="status-pill" :class="headerTone">{{ headerMessage }}</p>
          <button class="close-btn" type="button" aria-label="关闭提醒窗口" @click="hideWindow">
            ×
          </button>
        </div>
      </header>

      <section class="control-strip" aria-label="提醒统计">
        <div class="summary-panel">
          <article class="summary-pill">
            <span>待处理</span>
            <strong>{{ activeItems.length }}</strong>
          </article>
          <article class="summary-pill">
            <span>已触发</span>
            <strong>{{ notifiedCount }}</strong>
          </article>
          <article class="summary-pill">
            <span>总数</span>
            <strong>{{ reminderStore.items.length }}</strong>
          </article>
          <article v-if="recentSignalText" class="summary-pill signal-pill">
            <span>最近</span>
            <strong>{{ recentSignalText }}</strong>
          </article>
        </div>

        <button class="launch-btn" type="button" @click="toggleComposer">
          {{ composerOpen ? "收起新建" : "+ 新建提醒" }}
        </button>
      </section>

      <section class="list-panel">
        <div class="panel-heading">
          <div>
            <h2>待处理提醒</h2>
            <p class="list-caption">优先处理已触发与待触发事项</p>
          </div>
          <button
            type="button"
            class="ghost-btn"
            :disabled="reminderStore.loading"
            @click="syncReminders"
          >
            刷新
          </button>
        </div>

        <div v-if="!reminderStore.loading && !activeItems.length" class="empty-state">
          <strong>{{ reminderStore.items.length ? "当前没有待处理提醒" : "还没有提醒" }}</strong>
          <p>
            {{
              reminderStore.items.length
                ? "可以展开已完成分组回看历史，或者新建下一条提醒。"
                : "先创建一条，让隅灵开始提醒你。"
            }}
          </p>
        </div>

        <TransitionGroup v-else tag="ul" name="card" class="reminder-list">
          <li
            v-for="item in activeItems"
            :key="item.id"
            class="reminder-card"
            :class="`status-${item.status}`"
          >
            <div class="card-header">
              <div class="card-copy">
                <p class="card-eyebrow">{{ getCardEyebrow(item.status) }}</p>
                <strong>{{ item.title }}</strong>
                <p>{{ item.message || "没有备注，按时处理就好。" }}</p>
              </div>
              <span class="status-badge" :class="`is-${item.status}`">
                {{ formatStatus(item.status) }}
              </span>
            </div>

            <div class="card-footer">
              <p class="remind-at">{{ formatDisplayTime(item.remind_at) }}</p>

              <div class="card-actions">
                <button
                  type="button"
                  class="action-btn"
                  :disabled="item.status === 'completed' || reminderStore.loading"
                  @click="completeItem(item.id)"
                >
                  完成
                </button>
                <button
                  type="button"
                  class="action-btn"
                  :disabled="item.status === 'completed' || reminderStore.loading"
                  @click="snoozeItem(item.id)"
                >
                  延后
                </button>
                <button
                  type="button"
                  class="action-btn danger"
                  :disabled="reminderStore.loading"
                  @click="removeItem(item.id)"
                >
                  删除
                </button>
              </div>
            </div>
          </li>
        </TransitionGroup>
      </section>

      <section class="dock-stack">
        <article class="dock-card" :class="{ open: composerOpen }">
          <button class="dock-toggle" type="button" @click="toggleComposer">
            <div>
              <p class="dock-label">次级区域</p>
              <strong>新建提醒</strong>
            </div>
            <span class="dock-state">{{ composerOpen ? "收起" : "展开" }}</span>
          </button>

          <Transition name="dock">
            <form v-if="composerOpen" class="composer-panel" @submit.prevent="submitReminder">
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

              <div class="composer-actions">
                <p v-if="errorMessage" class="error-text">{{ errorMessage }}</p>
                <button class="primary-btn" type="submit" :disabled="reminderStore.submitting">
                  {{ reminderStore.submitting ? "保存中…" : "保存提醒" }}
                </button>
              </div>
            </form>
          </Transition>
        </article>

        <article class="dock-card" :class="{ open: completedOpen }">
          <button class="dock-toggle" type="button" @click="toggleCompleted">
            <div>
              <p class="dock-label">历史记录</p>
              <strong>已完成</strong>
            </div>
            <span class="dock-state">{{ completedItems.length }} 条</span>
          </button>

          <Transition name="dock">
            <div v-if="completedOpen" class="completed-panel">
              <p v-if="!completedItems.length" class="completed-empty">暂无已完成提醒</p>

              <ul v-else class="completed-list">
                <li v-for="item in completedItems" :key="item.id" class="completed-item">
                  <div class="completed-copy">
                    <strong>{{ item.title }}</strong>
                    <p>{{ formatDisplayTime(item.remind_at) }}</p>
                  </div>

                  <button
                    type="button"
                    class="ghost-btn danger-btn"
                    :disabled="reminderStore.loading"
                    @click="removeItem(item.id)"
                  >
                    删除
                  </button>
                </li>
              </ul>
            </div>
          </Transition>
        </article>
      </section>
    </section>
  </div>
</template>

<style scoped>
.reminder-shell {
  --panel-bg: rgba(8, 12, 21, 0.98);
  --panel-border: rgba(255, 255, 255, 0.08);
  --soft-text: rgba(217, 228, 241, 0.72);
  --muted-text: rgba(159, 176, 200, 0.74);
  --accent-amber: #f0b257;
  --accent-mint: #40d8b9;
  --accent-danger: #f3a2a0;
  width: 100%;
  height: 100%;
  padding: 16px;
  background:
    radial-gradient(circle at top left, rgba(240, 178, 87, 0.18), transparent 28%),
    radial-gradient(circle at top right, rgba(64, 216, 185, 0.12), transparent 30%),
    linear-gradient(180deg, rgba(8, 11, 20, 0.76), rgba(4, 7, 14, 0.94));
  font-family:
    "PingFang SC",
    "Hiragino Sans GB",
    "Microsoft YaHei",
    sans-serif;
}

.reminder-window {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  display: grid;
  grid-template-rows: auto auto minmax(0, 1fr) auto;
  gap: 10px;
  padding: 14px;
  border-radius: 30px;
  color: #f7fbff;
  background:
    linear-gradient(180deg, rgba(13, 18, 31, 0.98), rgba(7, 10, 19, 0.98));
  border: 1px solid var(--panel-border);
  box-shadow:
    0 28px 60px rgba(1, 4, 9, 0.5),
    inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

.reminder-window::before {
  content: "";
  position: absolute;
  inset: 0 0 auto;
  height: 88px;
  background:
    linear-gradient(90deg, rgba(240, 178, 87, 0.22), rgba(64, 216, 185, 0.12), transparent);
  opacity: 0.8;
  pointer-events: none;
}

.window-header,
.control-strip,
.list-panel,
.dock-stack {
  position: relative;
  z-index: 1;
}

.window-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.header-copy h1,
.panel-heading h2 {
  margin: 0;
}

.eyebrow {
  margin: 0 0 6px;
  font-size: 10px;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: rgba(255, 210, 139, 0.84);
}

.subtitle,
.list-caption,
.signal-copy p,
.card-copy p,
.remind-at,
.completed-copy p {
  margin: 0;
  color: var(--muted-text);
}

.subtitle {
  margin-top: 4px;
  font-size: 12px;
}

.header-tools {
  display: flex;
  align-items: center;
  gap: 6px;
}

.status-pill {
  max-width: 158px;
  margin: 0;
  padding: 7px 11px;
  border-radius: 999px;
  font-size: 10px;
  line-height: 1.2;
  color: #dfe7f2;
  background: rgba(255, 255, 255, 0.08);
}

.status-pill.is-live {
  color: #ffdeaa;
  background: rgba(240, 178, 87, 0.12);
}

.status-pill.is-error {
  color: #ffd3d1;
  background: rgba(243, 162, 160, 0.14);
}

.close-btn,
.ghost-btn,
.action-btn,
.dock-toggle,
.launch-btn,
.primary-btn {
  border: none;
  transition:
    transform 140ms ease,
    background-color 180ms ease,
    box-shadow 180ms ease,
    opacity 180ms ease;
}

.close-btn {
  width: 32px;
  height: 32px;
  border-radius: 999px;
  font-size: 20px;
  line-height: 1;
  color: rgba(248, 250, 252, 0.88);
  background: rgba(255, 255, 255, 0.08);
}

.control-strip {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 8px;
  align-items: center;
}

.summary-panel {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.summary-pill,
.list-panel,
.dock-card {
  border-radius: 22px;
  border: 1px solid var(--panel-border);
  background: rgba(255, 255, 255, 0.04);
}

.summary-pill {
  padding: 7px 10px;
  min-width: 74px;
}

.summary-pill span {
  display: block;
  margin-bottom: 3px;
  font-size: 10px;
  color: var(--soft-text);
}

.summary-pill strong {
  font-size: 15px;
  font-weight: 700;
}

.signal-pill {
  min-width: 122px;
  background: linear-gradient(135deg, rgba(240, 178, 87, 0.12), rgba(64, 216, 185, 0.08));
}

.signal-pill strong {
  font-size: 13px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.launch-btn {
  align-self: stretch;
  min-height: 46px;
  padding: 0 14px;
  border-radius: 16px;
  color: #151921;
  font-size: 13px;
  font-weight: 800;
  background: linear-gradient(135deg, var(--accent-amber), #ef8b65);
  box-shadow: 0 10px 20px rgba(239, 139, 101, 0.24);
}

.signal-label,
.card-eyebrow,
.dock-label {
  margin: 0 0 6px;
  font-size: 10px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: rgba(255, 210, 139, 0.8);
}

.signal-copy strong,
.completed-copy strong {
  display: block;
  margin-bottom: 4px;
  font-size: 15px;
}

.list-panel {
  min-height: 0;
  padding: 12px;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  gap: 10px;
}

.panel-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.list-caption {
  margin-top: 4px;
  font-size: 11px;
}

.ghost-btn {
  padding: 8px 12px;
  border-radius: 14px;
  font-size: 12px;
  color: #e7eff8;
  background: rgba(255, 255, 255, 0.08);
}

.empty-state {
  min-height: 160px;
  display: grid;
  place-items: center;
  text-align: center;
  border-radius: 18px;
  padding: 16px;
  background: rgba(255, 255, 255, 0.03);
}

.empty-state strong {
  font-size: 16px;
}

.empty-state p {
  margin: 8px 0 0;
  color: var(--muted-text);
  line-height: 1.6;
}

.reminder-list,
.completed-list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.reminder-list {
  min-height: 0;
  overflow-y: auto;
  display: grid;
  gap: 8px;
  align-content: start;
  padding-right: 4px;
}

.reminder-card {
  position: relative;
  overflow: hidden;
  padding: 11px 12px;
  border-radius: 18px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background:
    linear-gradient(180deg, rgba(10, 17, 30, 0.96), rgba(7, 12, 21, 0.98));
}

.reminder-card::before {
  content: "";
  position: absolute;
  inset: 0 auto 0 0;
  width: 4px;
  background: linear-gradient(180deg, var(--accent-amber), var(--accent-mint));
}

.reminder-card.status-notified {
  box-shadow: inset 0 0 0 1px rgba(240, 178, 87, 0.16);
}

.card-header,
.card-footer,
.completed-item,
.dock-toggle,
.composer-actions {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.card-copy {
  min-width: 0;
}

.card-copy strong {
  display: block;
  margin-bottom: 3px;
  font-size: 15px;
  line-height: 1.15;
}

.card-copy p {
  font-size: 11px;
  line-height: 1.45;
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.status-badge {
  flex-shrink: 0;
  padding: 4px 8px;
  border-radius: 999px;
  font-size: 10px;
  font-weight: 700;
  background: rgba(255, 255, 255, 0.08);
}

.status-badge.is-pending {
  color: #ffe09d;
}

.status-badge.is-notified {
  color: #ffc38d;
}

.card-footer {
  margin-top: 8px;
  align-items: center;
}

.remind-at {
  font-size: 11px;
  white-space: nowrap;
}

.card-actions {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.action-btn {
  min-height: 30px;
  padding: 0 10px;
  border-radius: 10px;
  font-size: 11px;
  color: #e8f0f8;
  background: rgba(255, 255, 255, 0.08);
}

.action-btn.danger,
.danger-btn {
  color: var(--accent-danger);
}

.dock-stack {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.dock-card {
  overflow: hidden;
}

.dock-card.open {
  grid-column: 1 / -1;
  background: rgba(255, 255, 255, 0.05);
}

.dock-toggle {
  width: 100%;
  min-height: 74px;
  padding: 10px 12px;
  color: #f7fbff;
  background: transparent;
}

.dock-toggle strong {
  font-size: 14px;
}

.dock-state {
  align-self: center;
  font-size: 11px;
  color: var(--soft-text);
}

.composer-panel,
.completed-panel {
  padding: 0 14px 14px;
}

.composer-panel {
  display: grid;
  gap: 8px;
}

.field {
  display: grid;
  gap: 6px;
  font-size: 12px;
  color: var(--soft-text);
}

.field input,
.field textarea {
  width: 100%;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 14px;
  padding: 10px 12px;
  color: #f7fbff;
  background: rgba(11, 18, 31, 0.8);
  outline: none;
}

.field input:focus,
.field textarea:focus {
  border-color: rgba(64, 216, 185, 0.6);
  box-shadow: 0 0 0 3px rgba(64, 216, 185, 0.12);
}

.field textarea {
  resize: none;
}

.composer-actions {
  align-items: center;
}

.error-text {
  margin: 0;
  font-size: 12px;
  color: #ffd3d1;
}

.primary-btn {
  min-height: 38px;
  padding: 0 16px;
  border-radius: 14px;
  font-size: 13px;
  font-weight: 800;
  color: #12161d;
  background: linear-gradient(135deg, var(--accent-amber), #ef8b65);
}

.completed-list {
  display: grid;
  gap: 8px;
  max-height: 160px;
  overflow-y: auto;
  padding-right: 4px;
}

.completed-item {
  padding: 11px 12px;
  border-radius: 16px;
  align-items: center;
  background: rgba(11, 18, 31, 0.68);
}

.completed-empty {
  margin: 0;
  color: var(--muted-text);
  font-size: 12px;
}

.completed-copy p {
  font-size: 12px;
}

@media (max-height: 760px) {
  .reminder-shell {
    padding: 12px;
  }

  .reminder-window {
    gap: 8px;
    padding: 12px;
  }

  .window-header {
    gap: 8px;
  }

  .summary-pill {
    min-width: 68px;
  }

  .dock-toggle {
    min-height: 66px;
  }
}

.close-btn:hover,
.ghost-btn:hover,
.action-btn:hover,
.dock-toggle:hover {
  background: rgba(255, 255, 255, 0.12);
}

.launch-btn:hover,
.primary-btn:hover {
  transform: translateY(-1px);
  box-shadow: 0 14px 24px rgba(239, 139, 101, 0.28);
}

.close-btn:active,
.ghost-btn:active,
.action-btn:active,
.dock-toggle:active,
.launch-btn:active,
.primary-btn:active {
  transform: scale(0.98);
}

.primary-btn:disabled,
.ghost-btn:disabled,
.action-btn:disabled {
  cursor: not-allowed;
  opacity: 0.55;
  box-shadow: none;
}

.card-enter-active,
.card-leave-active {
  transition:
    transform 260ms cubic-bezier(0.22, 1, 0.36, 1),
    opacity 220ms ease,
    filter 220ms ease;
}

.card-enter-from,
.card-leave-to {
  opacity: 0;
  transform: translateY(12px) scale(0.98);
  filter: blur(8px);
}

.card-move {
  transition: transform 260ms cubic-bezier(0.22, 1, 0.36, 1);
}

.dock-enter-active,
.dock-leave-active {
  overflow: hidden;
  transition:
    max-height 280ms cubic-bezier(0.22, 1, 0.36, 1),
    opacity 220ms ease,
    transform 240ms ease;
}

.dock-enter-from,
.dock-leave-to {
  max-height: 0;
  opacity: 0;
  transform: translateY(-8px);
}

.dock-enter-to,
.dock-leave-from {
  max-height: 320px;
  opacity: 1;
  transform: translateY(0);
}

@media (prefers-reduced-motion: reduce) {
  .close-btn,
  .ghost-btn,
  .action-btn,
  .dock-toggle,
  .launch-btn,
  .primary-btn,
  .card-enter-active,
  .card-leave-active,
  .card-move,
  .dock-enter-active,
  .dock-leave-active {
    transition: none !important;
  }
}
</style>
