import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { parseConnectionLink, type ConnectionLink } from '../connectionLinks';
import { desktop } from './useNoryn';

export const pendingLinks = ref<ConnectionLink[]>([]);
export const linkError = ref('');
export function receiveLinks(urls: string[]) {
  for (const url of urls.slice(0, 16)) {
    try {
      const link = parseConnectionLink(url);
      if (
        pendingLinks.value.length < 16 &&
        !pendingLinks.value.some((pending) => JSON.stringify(pending) === JSON.stringify(link))
      )
        pendingLinks.value.push(link);
    } catch (error) {
      linkError.value = String(error);
    }
  }
}
if (desktop) {
  let disposed = false;
  let cleanup: (() => void) | undefined;
  void (async () => {
    try {
      cleanup = await listen<string[]>('deep-link://new-url', (event) =>
        receiveLinks(event.payload),
      );
      if (disposed) {
        cleanup();
        return;
      }
      const current = await invoke<string[] | null>('plugin:deep-link|get_current');
      if (!disposed && current) receiveLinks(current);
    } catch (error) {
      if (!disposed) linkError.value = String(error);
    }
  })();
  if (import.meta.hot)
    import.meta.hot.dispose(() => {
      disposed = true;
      cleanup?.();
    });
}
