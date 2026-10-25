<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { Channel, invoke } from '@tauri-apps/api/core';
import { desktop, notify } from '../composables/useNoryn';
import type { ClientSession, RemoteEntry } from '../types';
import Modal from './Modal.vue';
type Action = 'copy' | 'move' | 'delete' | 'newFile' | 'newFolder';
interface Preview {
  fingerprint: string;
  files: number;
  directories: number;
  links: number;
  bytes: number;
}
const props = defineProps<{
  session: ClientSession;
  side: 'local' | 'remote';
  separator: string;
}>();
const emit = defineEmits<{ done: [] }>();
const visible = ref(false),
  busy = ref(false),
  error = ref(''),
  target = ref(''),
  confirmed = ref(false);
const source = ref(''),
  action = ref<Action>('copy'),
  preview = ref<Preview>();
const titles: Record<Action, string> = {
  copy: 'Kopieren',
  move: 'Verschieben / umbenennen',
  delete: 'Löschen',
  newFile: 'Neue Datei',
  newFolder: 'Neuer Ordner',
};
const title = computed(() => titles[action.value]);
let job: string | undefined;
let generation = 0;
function open(kind: Action, directory: string, entry?: RemoteEntry) {
  if (busy.value) return;
  action.value = kind;
  source.value = entry?.path || '';
  preview.value = undefined;
  error.value = '';
  confirmed.value = false;
  target.value =
    directory.replace(/[\\/]$/, '') +
    props.separator +
    (entry
      ? entry.name + (kind === 'copy' ? '.copy' : '')
      : kind === 'newFile'
        ? 'new-file.txt'
        : 'new-folder');
  visible.value = true;
}
function cancel() {
  if (job && desktop) void invoke('remote_transfer_cancel', { transferId: job });
}
function close() {
  if (busy.value) return;
  generation++;
  visible.value = false;
}
watch(target, () => {
  preview.value = undefined;
  confirmed.value = false;
});
async function run() {
  if (busy.value || (preview.value && action.value === 'delete' && !confirmed.value)) return;
  if (!desktop) {
    error.value = 'Browser-Vorschau: Dateiaktionen sind nur in der Desktop-App verfügbar.';
    return;
  }
  const ticket = ++generation;
  const applying = !!preview.value;
  const request = {
    action: action.value,
    source: source.value,
    destination: action.value === 'delete' ? '' : target.value,
    local: props.side === 'local',
  };
  busy.value = true;
  error.value = '';
  job = crypto.randomUUID();
  try {
    const result = await invoke<Preview>('file_action', {
      id: props.session.id,
      jobId: job,
      request,
      expected: preview.value?.fingerprint || null,
      output: new Channel(),
    });
    if (ticket !== generation) return;
    if (applying) {
      visible.value = false;
      notify('Dateiaktion abgeschlossen');
      emit('done');
    } else preview.value = result;
  } catch (cause) {
    if (ticket === generation) {
      error.value =
        String(cause) +
        (applying
          ? '\nTeiländerungen können erhalten bleiben. Ziel vor einem erneuten Versuch prüfen; vorhandene Ziele werden nicht zusammengeführt oder überschrieben.'
          : '');
      preview.value = undefined;
      emit('done');
    }
  } finally {
    if (ticket === generation) {
      busy.value = false;
      job = undefined;
    }
  }
}
onBeforeUnmount(() => {
  generation++;
  cancel();
});
defineExpose({ open });
</script>
<template>
  <Modal
    v-if="visible"
    :title="title"
    :subtitle="side === 'local' ? 'Lokales Dateisystem' : 'Entferntes Dateisystem'"
    :busy="busy"
    @close="close"
  >
    <form @submit.prevent="run">
      <div class="form-body">
        <label v-if="source" class="full"
          >Quelle<code class="file-action-path">{{ source }}</code></label
        >
        <label v-if="action !== 'delete'" class="full"
          >Absoluter Zielpfad<input v-model="target" :disabled="busy" required spellcheck="false"
        /></label>
        <p class="muted full">
          {{
            action === 'delete'
              ? 'Dateien und Unterordner werden endgültig entfernt. Verknüpfungen werden nur selbst gelöscht.'
              : 'Vorhandene Ziele werden nicht überschrieben. Kopien überspringen verschachtelte Verknüpfungen. Verschieben zwischen Dateisystemen kann abgelehnt werden.'
          }}
        </p>
        <div v-if="preview" class="full file-action-preview">
          <p>
            {{ preview.files }} Dateien · {{ preview.directories }} Ordner ·
            {{ preview.bytes.toLocaleString() }} Bytes
          </p>
          <p v-if="preview.links">
            {{ preview.links }} Verknüpfungen
            {{ action === 'copy' ? 'werden übersprungen.' : 'werden nicht verfolgt.' }}
          </p>
          <label v-if="action === 'delete'" class="checkbox-row"
            ><input v-model="confirmed" type="checkbox" :disabled="busy" />Diesen Eintrag
            einschließlich aller Unterordner endgültig löschen</label
          >
        </div>
        <p v-if="error" class="inline-error full" role="alert">{{ error }}</p>
      </div>
      <div class="modal-actions">
        <button v-if="busy" type="button" class="button secondary" @click="cancel">
          Vorgang abbrechen
        </button>
        <button v-else type="button" class="button secondary" @click="close">Schließen</button>
        <button
          class="button primary"
          :class="{ danger: action === 'delete' && preview }"
          :disabled="busy || (!!preview && action === 'delete' && !confirmed)"
        >
          {{ busy ? 'Wird verarbeitet …' : preview ? title : 'Vorschau prüfen' }}
        </button>
      </div>
    </form>
  </Modal>
</template>
<style scoped>
.file-action-path {
  display: block;
  overflow-wrap: anywhere;
  margin-top: 0.5rem;
}
.file-action-preview {
  padding: 0.8rem;
  border: 1px solid var(--border);
  border-radius: 8px;
}
.inline-error {
  white-space: pre-wrap;
}
</style>
