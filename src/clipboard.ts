import { invoke, isTauri } from '@tauri-apps/api/core';
export async function copyText(text: string) {
  if (isTauri()) await invoke('clipboard_write_text', { text });
  else await navigator.clipboard.writeText(text);
}
export async function pasteText(): Promise<string> {
  return isTauri() ? invoke<string>('clipboard_read_text') : navigator.clipboard.readText();
}
/** Only keydown performs I/O; both phases suppress terminal input for shortcuts. */
export function clipboardKey(
  event: KeyboardEvent,
  copy: () => Promise<void>,
  paste: () => Promise<void>,
  error: (cause: unknown) => void,
) {
  const copyChord =
    !event.altKey &&
    ((event.ctrlKey && event.shiftKey && !event.metaKey) ||
      (event.metaKey && !event.ctrlKey && !event.shiftKey));
  const key = event.key.toLowerCase();
  const operation =
    copyChord && key === 'c'
      ? copy
      : copyChord && key === 'v'
        ? paste
        : !event.ctrlKey && !event.metaKey && !event.altKey && event.shiftKey && key === 'insert'
          ? paste
          : undefined;
  if (!operation) return true;
  event.preventDefault();
  event.stopPropagation();
  if (event.type === 'keydown' && !event.repeat) void operation().catch(error);
  return false;
}
