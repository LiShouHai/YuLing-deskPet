/**
 * 应用入口：
 * - 创建 Vue 实例
 * - 注入 Pinia 状态树
 * - 绑定全局样式
 * 该文件保持精简，所有业务逻辑均拆分至组件与 store。
 */
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "./style.css";

// 创建根应用实例，后续所有组件挂载在该实例之下
const app = createApp(App);

// 注入全局状态管理，确保桌宠状态可在任意组件读取
app.use(createPinia());

// 将应用挂载到 Vite 生成的 #app 容器
app.mount("#app");
