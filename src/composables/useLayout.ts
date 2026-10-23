import { reactive, watch } from 'vue';
const key = 'noryn.layout.v1';
let saved: Record<string, unknown> = {};
try {
  saved = JSON.parse(localStorage.getItem(key) || '{}') || {};
} catch {
  /* Preferences are optional. */
}
function number(value: unknown, fallback: number, min: number, max: number) {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.max(min, Math.min(max, value))
    : fallback;
}
export const layout = reactive({
  sidebarWidth: number(saved.sidebarWidth, 240, 200, 380),
  terminalRatio: number(saved.terminalRatio, 0.5, 0.1, 0.9),
});
let timer: ReturnType<typeof setTimeout>;
watch(layout, () => {
  clearTimeout(timer);
  timer = setTimeout(() => {
    try {
      localStorage.setItem(key, JSON.stringify(layout));
    } catch {
      /* Still resize when storage is unavailable. */
    }
  }, 180);
});
