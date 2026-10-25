import { createApp } from 'vue';
import App from './App.vue';
import './composables/useAppearance';
import './composables/useLiveSync';
import './composables/useScripts';
import './composables/useScriptEditors';
import './composables/useUpdater';
import './fonts.css';
import './themes.css';
import './style.css';
import './settings.css';

createApp(App).mount('#app');
