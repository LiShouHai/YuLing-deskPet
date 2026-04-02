/**
 * ReminderStore
 * 管理提醒列表、表单状态以及与 PlatformBridge 的交互。
 */
import { defineStore } from "pinia";
import {
  completeReminder,
  createReminder,
  deleteReminder,
  listReminders,
  snoozeReminder,
} from "../platformBridge";

function parseDateInput(value) {
  const ts = Date.parse(value);
  return Number.isNaN(ts) ? null : ts;
}

export const useReminderStore = defineStore("reminder", {
  state: () => ({
    items: [],
    loading: false,
    submitting: false,
    composer: {
      title: "",
      message: "",
      remindAt: "",
    },
    lastFired: null,
  }),
  actions: {
    async fetchReminders() {
      if (this.loading) return;
      this.loading = true;
      try {
        this.items = await listReminders();
      } catch (error) {
        console.error("fetch reminders failed", error);
      } finally {
        this.loading = false;
      }
    },
    async addReminder() {
      if (this.submitting) return;
      const remindAt = parseDateInput(this.composer.remindAt);
      if (!this.composer.title.trim() || !remindAt) {
        throw new Error("标题与提醒时间不可为空");
      }
      this.submitting = true;
      try {
        await createReminder({
          title: this.composer.title.trim(),
          message: this.composer.message?.trim() || null,
          remind_at: remindAt,
        });
        this.composer.title = "";
        this.composer.message = "";
        this.composer.remindAt = "";
        await this.fetchReminders();
      } finally {
        this.submitting = false;
      }
    },
    async complete(id) {
      await completeReminder(id);
      await this.fetchReminders();
    },
    async remove(id) {
      await deleteReminder(id);
      await this.fetchReminders();
    },
    async snooze(id, millis) {
      const target = Date.now() + millis;
      await snoozeReminder(id, target);
      await this.fetchReminders();
    },
    markFired(reminder) {
      this.lastFired = reminder;
    },
  },
});
