/**
 * 桌宠全局状态中心：
 * - 统一保存显示器、自启、电源模式、拖拽等状态
 * - 提供动作简化组件逻辑
 */
import { defineStore } from "pinia";

export const usePetStore = defineStore("pet", {
  state: () => ({
    // 由 PlatformBridge 返回的监视器数组
    monitors: [],
    // 状态提示文案，主要用于提醒列表面板
    statusText: "启动中…",
    // 最近一次同步时间
    lastUpdated: null,
    // 系统级自启动开关
    autostartEnabled: false,
    // 电源模式（normal | low）
    powerMode: "normal",
    // 是否处于拖拽状态
    dragging: false,
    // 是否显示提醒脉冲效果
    reminderActive: false
  }),
  actions: {
    /**
     * 写入最新监视器数组并同步状态
     * @param {Array} list - 原始监视器列表
     */
    setMonitors(list) {
      this.monitors = Array.isArray(list) ? list : [];
      const count = this.monitors.length;
      this.statusText = count ? `捕获到 ${count} 块显示器` : "等待系统反馈";
      this.lastUpdated = new Date().toLocaleTimeString();
    },
    /**
     * 更新自启动开关
     * @param {boolean} flag - true 表示启用
     */
    setAutostart(flag) {
      this.autostartEnabled = !!flag;
    },
    /**
     * 切换电源模式（动画降帧依赖该状态）
     */
    togglePowerMode() {
      this.powerMode = this.powerMode === "normal" ? "low" : "normal";
    },
    /**
     * 标记当前是否正在拖拽
     * @param {boolean} flag
     */
    setDragging(flag) {
      this.dragging = flag;
    },
    /**
     * 控制提醒脉冲显隐
     * @param {boolean} active
     */
    pulseReminder(active) {
      this.reminderActive = !!active;
    }
  }
});
