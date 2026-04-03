import { createApp } from "vue";
import { createPinia } from "pinia";
import ReminderApp from "./ReminderApp.vue";
import "../style.css";

const app = createApp(ReminderApp);

app.use(createPinia());
app.mount("#app");
