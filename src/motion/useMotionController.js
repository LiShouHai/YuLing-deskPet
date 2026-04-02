/**
 * MotionController
 * 负责在前端模拟动画状态机：
 * - 解析 manifest，获得不同状态对应的帧率/可中断性
 * - 提供 `setState` 以外部驱动动画切换
 * - 根据 powerMode 降帧，减少功耗
 */
import { onMounted, onUnmounted, ref, watch } from "vue";
import manifest from "../assets/motion/manifest.json";

/**
 * 状态优先级字典：
 * 数值越高表示不可轻易被切换，一般用于强交互状态。
 */
const STATE_PRIORITY = {
  drag: 80,
  reminderPulse: 60,
  react: 40,
  idle: 10,
  sleep: 5
};

/**
 * 创建 Motion 控制器 Hook
 * @param {Ref<string>} powerModeRef - Pinia 中的电源模式
 */
export function useMotionController(powerModeRef) {
  // 当前动画状态（idle / drag / react ...）
  const state = ref("idle");
  // 背景光晕强度，用于视觉反馈
  const overlayIntensity = ref(0);
  // 纯粹的帧计数，便于后续帧动画扩展
  const frame = ref(0);
  // 最近一次循环的时间戳
  const lastBeat = ref(0);
  // 实际生效帧率（受状态与低功耗模式影响）
  const effectiveFps = ref(manifest.idle.fps);
  let rafId;
  let last = 0;

  /**
   * 核心 requestAnimationFrame 循环
   * - 根据 delta 推算帧进度，避免不同刷新率差异
   * - 通过 sin 波形生成柔和的光晕强度
   */
  const loop = (timestamp) => {
    if (!last) last = timestamp; // 首帧初始化基准时间
    const delta = timestamp - last; // 单次 RAF 间隔
    last = timestamp;
    const targetFps = effectiveFps.value || 24;
    const step = delta / (1000 / targetFps); // 将时间差换算为帧进度
    frame.value = (frame.value + step) % 100; // 维持 0-100 的循环帧
    overlayIntensity.value = 0.5 + 0.5 * Math.sin(timestamp / 600); // 呼吸感
    lastBeat.value = timestamp;
    rafId = requestAnimationFrame(loop); // 继续下一次循环
  };

  // 挂载时启动动画循环
  onMounted(() => {
    rafId = requestAnimationFrame(loop);
  });

  // 卸载时清理 RAF，防止内存泄漏
  onUnmounted(() => {
    if (rafId) cancelAnimationFrame(rafId);
  });

  /**
   * 切换状态时尊重优先级和中断规则
   * - 低优先级状态不可覆盖高优先级且不可中断的状态
   */
  const setState = (next) => {
    if (state.value === next) return; // 无需重复设置
    const currentPriority = STATE_PRIORITY[state.value] ?? 0;
    const nextPriority = STATE_PRIORITY[next] ?? 0;
    if (nextPriority < currentPriority && !(manifest[state.value]?.interruptible)) {
      return; // 禁止低优先级状态打断
    }
    state.value = next;
    effectiveFps.value = manifest[next]?.fps ?? 24;
    frame.value = 0;
    last = 0;
  };

  // 监听电源模式，低功耗时强制帧率不超过 12fps
  watch(
    () => powerModeRef.value,
    (mode) => {
      if (mode === "low") {
        effectiveFps.value = Math.min(effectiveFps.value, 12);
      } else {
        effectiveFps.value = manifest[state.value]?.fps ?? 24;
      }
    }
  );

  return {
    state,
    frame,
    overlayIntensity,
    lastBeat,
    setState
  };
}
