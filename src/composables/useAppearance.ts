import { reactive, ref, watch } from 'vue';

export const themes = [
  {
    id: 'graphite',
    name: 'Graphit',
    description: 'Ruhig und neutral',
    background: '#16171b',
    panel: '#2b3039',
    accent: '#c3cde2',
  },
  {
    id: 'midnight',
    name: 'Mitternacht',
    description: 'Tiefblau mit Violett',
    background: '#101421',
    panel: '#283249',
    accent: '#b6a0ff',
  },
  {
    id: 'nord',
    name: 'Nord',
    description: 'Kühle, weiche Farben',
    background: '#242b36',
    panel: '#414c5e',
    accent: '#88c0d0',
  },
  {
    id: 'carbon',
    name: 'Carbon',
    description: 'Neutrales Dunkelgrau, klare blaue Akzente',
    background: '#111315',
    panel: '#292e35',
    accent: '#78a5ed',
  },
  {
    id: 'matrix',
    name: 'Matrix',
    description: 'Tiefschwarz mit leuchtendem Grün',
    background: '#050b08',
    panel: '#14291b',
    accent: '#6dff9e',
  },
  {
    id: 'light',
    name: 'Licht',
    description: 'Hell und klar',
    background: '#f3f5f9',
    panel: '#dce4f0',
    accent: '#3866b5',
  },
] as const;
export type ThemeId = (typeof themes)[number]['id'];
export const textSizes = [
  { id: 'xs', name: 'XS', pixels: 12 },
  { id: 'sm', name: 'SM', pixels: 13 },
  { id: 'md', name: 'MD', pixels: 14 },
  { id: 'lg', name: 'LG', pixels: 16 },
  { id: 'xl', name: 'XL', pixels: 18 },
] as const;
export type TextSize = (typeof textSizes)[number]['id'];
const storageKey = 'noryn.appearance.v1';
const defaults = { theme: 'graphite' as ThemeId, textSize: 'md' as TextSize };
export const storageAvailable = ref(true);
function validate(value: unknown) {
  const candidate = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  return {
    theme: themes.find((theme) => theme.id === candidate.theme)?.id || defaults.theme,
    textSize: textSizes.find((size) => size.id === candidate.textSize)?.id || defaults.textSize,
  };
}
let saved = { ...defaults };
try {
  saved = validate(JSON.parse(localStorage.getItem(storageKey) || '{}'));
} catch {
  // Broken or unavailable preferences never prevent opening the vault.
}
export const appearance = reactive(saved);
export function baseTextPixels() {
  return textSizes.find((size) => size.id === appearance.textSize)?.pixels || 14;
}
function apply() {
  document.documentElement.dataset.theme = appearance.theme;
  document.documentElement.dataset.textSize = appearance.textSize;
  document.documentElement.style.fontSize = `${baseTextPixels()}px`;
}
export function resetAppearance() {
  Object.assign(appearance, defaults);
}
apply();
const stop = watch(
  appearance,
  () => {
    apply();
    try {
      localStorage.setItem(storageKey, JSON.stringify(appearance));
      storageAvailable.value = true;
    } catch {
      storageAvailable.value = false;
    }
  },
  { flush: 'sync' },
);
if (import.meta.hot) import.meta.hot.dispose(stop);
