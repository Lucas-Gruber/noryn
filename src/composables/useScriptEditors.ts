import { reactive, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { desktop, store } from './useNoryn';
import { scheduleScripts } from './useScripts';
import type { ScriptInfo, VaultView } from '../types';
interface Editor {
  id: string;
  teamId: string;
  name: string;
  autoFinish: boolean;
  editorNote: string;
  error?: string;
  busy?: boolean;
}
export const scriptEditors = reactive<Editor[]>([]);
const waiting = new Set<string>();
let epoch = 0;
function observe(editor: Editor) {
  if (!editor.autoFinish || waiting.has(editor.id)) return;
  waiting.add(editor.id);
  const current = epoch;
  void invoke<{ view: VaultView; changed: boolean }>('script_edit_wait', { documentId: editor.id })
    .then((result) => {
      if (current !== epoch || !store.unlocked) return;
      store.view = result.view;
      const index = scriptEditors.findIndex((e) => e.id === editor.id);
      if (index >= 0) scriptEditors.splice(index, 1);
      if (result.changed) scheduleScripts(editor.teamId);
    })
    .catch((error) => {
      if (current === epoch && store.unlocked) {
        editor.error = String(error);
        editor.autoFinish = false;
      }
    })
    .finally(() => waiting.delete(editor.id));
}
export async function openScriptEditor(teamId: string, entry: ScriptInfo) {
  const current = epoch;
  const result = await invoke<Editor>('script_edit_open', {
    teamId,
    entryId: entry.id,
    expected: entry.fingerprint,
  });
  if (current !== epoch || !store.unlocked) return;
  let editor = scriptEditors.find((e) => e.id === result.id);
  if (editor) Object.assign(editor, result, { error: '' });
  else {
    scriptEditors.push(result);
    editor = scriptEditors[scriptEditors.length - 1]!;
  }
  observe(editor);
}
export async function finishScriptEditor(editor: Editor) {
  const current = epoch;
  editor.busy = true;
  editor.error = '';
  try {
    const result = await invoke<{ view: VaultView; changed: boolean }>('script_edit_finish', {
      documentId: editor.id,
    });
    if (current !== epoch || !store.unlocked) return;
    store.view = result.view;
    scriptEditors.splice(scriptEditors.indexOf(editor), 1);
    if (result.changed) scheduleScripts(editor.teamId);
  } catch (error) {
    editor.error = String(error);
  } finally {
    editor.busy = false;
  }
}
export async function discardScriptEditor(editor: Editor) {
  try {
    await invoke('script_edit_discard', { documentId: editor.id });
    const index = scriptEditors.indexOf(editor);
    if (index >= 0) scriptEditors.splice(index, 1);
  } catch (error) {
    editor.error = String(error);
  }
}
watch(
  () => store.unlocked,
  async (unlocked) => {
    epoch++;
    const current = epoch;
    scriptEditors.splice(0);
    waiting.clear();
    if (!desktop || !unlocked) return;
    try {
      const result = await invoke<Editor[]>('script_edit_list');
      if (current !== epoch || !store.unlocked) return;
      scriptEditors.push(...result);
      for (const editor of scriptEditors) observe(editor);
    } catch {
      /* The list is restored on the next unlock. */
    }
  },
  { immediate: true },
);
