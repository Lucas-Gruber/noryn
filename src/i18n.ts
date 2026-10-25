import { computed, ref, watch } from 'vue';
import english from './locales/en.json';
import germanEditorPhrases from './locales/editor.de.json';
export type Language = 'system' | 'de' | 'en';
const storageKey = 'noryn.locale.v1';
export function systemLanguage(languages: readonly string[]): 'de' | 'en' {
  for (const language of languages) {
    const base = language.toLowerCase().split(/[-_]/)[0];
    if (base === 'de' || base === 'en') return base;
  }
  return 'en';
}
function detected() {
  return systemLanguage(
    typeof navigator === 'undefined'
      ? ['en']
      : navigator.languages?.length
        ? navigator.languages
        : [navigator.language],
  );
}
const system = ref(detected());
let saved: Language = 'system';
try {
  const value = JSON.parse(localStorage.getItem(storageKey) || 'null');
  if (['system', 'de', 'en'].includes(value)) saved = value;
} catch {
  /* Unavailable preferences never block the vault. */
}
export const language = ref<Language>(saved);
export const locale = computed(() => (language.value === 'system' ? system.value : language.value));
export const localeStorageAvailable = ref(true);
const catalog: Record<string, string> = english;
/** German source messages are stable gettext-style keys; parameters stay data. */
export function t(
  message: string,
  parameters: Record<string, unknown> = {},
  count?: number,
): string {
  let text = locale.value === 'en' ? (catalog[message] ?? message) : message;
  if (count !== undefined && text.includes('|')) {
    const forms = text.split('|');
    text = forms[new Intl.PluralRules(locale.value).select(count) === 'one' ? 0 : 1] || forms[0]!;
  }
  return text.replace(/\{(p\d+|[a-zA-Z][a-zA-Z0-9_]*)\}/g, (match, key: string) =>
    Object.hasOwn(parameters, key) ? String(parameters[key]) : match,
  );
}
export function formatLocale() {
  return locale.value === 'de' ? 'de-DE' : 'en-US';
}
function apply() {
  if (typeof document !== 'undefined') document.documentElement.lang = locale.value;
}
apply();
const stop = watch(
  language,
  () => {
    apply();
    try {
      localStorage.setItem(storageKey, JSON.stringify(language.value));
      localeStorageAvailable.value = true;
    } catch {
      localeStorageAvailable.value = false;
    }
  },
  { flush: 'sync' },
);
function changed() {
  system.value = detected();
  apply();
}
if (typeof window !== 'undefined') window.addEventListener('languagechange', changed);
if (import.meta.hot)
  import.meta.hot.dispose(() => {
    stop();
    if (typeof window !== 'undefined') window.removeEventListener('languagechange', changed);
  });

const sourceForEnglish = new Map(
  Object.entries(catalog).map(([source, translated]) => [translated, source]),
);
/** Only for application messages that may have been produced before switching. */
export function messageText(message: string) {
  return t(Object.hasOwn(catalog, message) ? message : sourceForEnglish.get(message) || message);
}
/** Native/OS diagnostics stay available verbatim when no app translation exists. */
export function errorText(reason: unknown): string {
  const original = String(reason);
  if (locale.value === 'de') return messageText(original);
  if (Object.hasOwn(catalog, original) || sourceForEnglish.has(original))
    return messageText(original);
  // Translate known application context while retaining technical causes and paths.
  const split = original.indexOf(': ');
  if (split > 0 && Object.hasOwn(catalog, original.slice(0, split)))
    return t(original.slice(0, split)) + ': ' + original.slice(split + 2);
  return t('Details im Original') + ': ' + original;
}

export function editorPhrases() {
  return locale.value === 'de' ? germanEditorPhrases : {};
}
