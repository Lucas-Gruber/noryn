<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, RemoteEntry } from '../types';
import { desktop, notify } from '../composables/useNoryn';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
interface Document {
  id: string;
  name: string;
  path: string;
  remote: boolean;
  textCopy: boolean;
  autoFinish: boolean;
  editorNote: string;
  error?: string;
}
const props = defineProps<{ session: ClientSession }>();
const emit = defineEmits<{ saved: [] }>();
const documents = ref<Document[]>([]),
  busy = ref(''),
  error = ref('');
const closing = ref<Document | null>(null);
let disposed = false;
const waiting = new Set<string>();
function completed(document: Document, changed: boolean) {
  documents.value = documents.value.filter((d) => d.id !== document.id);
  if (changed) emit('saved');
  notify(
    changed
      ? `${document.name}: Änderungen übernommen, Arbeitskopie gelöscht.`
      : `${document.name}: unverändert geschlossen, Arbeitskopie gelöscht.`,
  );
}
async function watchEditor(document: Document) {
  if (!desktop || !document.autoFinish || waiting.has(document.id)) return;
  waiting.add(document.id);
  try {
    const changed = await invoke<boolean>('text_wait', {
      id: props.session.id,
      documentId: document.id,
    });
    if (!disposed && documents.value.some((d) => d.id === document.id))
      completed(document, changed);
  } catch (reason) {
    if (!disposed && documents.value.some((d) => d.id === document.id)) {
      document.autoFinish = false;
      document.error = `${String(reason)}. Arbeitskopie bleibt erhalten.`;
    }
  } finally {
    waiting.delete(document.id);
  }
}
watch(
  () => documents.value.length,
  (count) => (props.session.editedFiles = count),
  { immediate: true },
);
async function openFile(side: 'local' | 'remote', entry: RemoteEntry) {
  if (busy.value) return;
  busy.value = '@open';
  error.value = '';
  try {
    const document = desktop
      ? await invoke<Document>('text_open', { id: props.session.id, side, path: entry.path })
      : {
          id: entry.path,
          name: entry.name,
          path: entry.path,
          remote: side === 'remote',
          textCopy: false,
          autoFinish: false,
          editorNote: 'Vorschau: Im Editor speichern und schließen, dann Bearbeitung beenden.',
        };
    if (disposed) return;
    const existing = documents.value.find((d) => d.id === document.id);
    if (existing) Object.assign(existing, document);
    const current = existing || document;
    if (!existing) documents.value.push(document);
    void watchEditor(documents.value.find((d) => d.id === current.id)!);
    notify(
      desktop
        ? document.autoFinish
          ? 'Arbeitskopie geöffnet. Nach dem Schließen werden Änderungen übernommen.'
          : 'Arbeitskopie geöffnet. Im Editor speichern und schließen, dann Bearbeitung beenden.'
        : 'Vorschau: Externe Programme sind in der Desktop-App verfügbar.',
    );
  } catch (reason) {
    if (!disposed) error.value = String(reason);
  } finally {
    busy.value = '';
  }
}
async function reopen(document: Document) {
  if (busy.value) return;
  busy.value = document.id;
  error.value = '';
  try {
    if (desktop) {
      Object.assign(
        document,
        await invoke<Document>('text_reopen', { id: props.session.id, documentId: document.id }),
      );
      document.error = '';
      void watchEditor(document);
    } else notify('Vorschau: Öffnet die Arbeitskopie im Standardprogramm.');
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = '';
  }
}
async function finish(document: Document) {
  if (busy.value || document.autoFinish) return;
  busy.value = document.id;
  document.error = '';
  try {
    const changed = desktop
      ? await invoke<boolean>('text_finish', { id: props.session.id, documentId: document.id })
      : false;
    if (!disposed) completed(document, changed);
  } catch (reason) {
    if (!disposed) document.error = `${String(reason)}. Arbeitskopie bleibt erhalten.`;
  } finally {
    busy.value = '';
  }
}
async function close() {
  if (!closing.value || busy.value) return;
  const document = closing.value;
  busy.value = document.id;
  error.value = '';
  try {
    if (desktop) await invoke('text_close', { id: props.session.id, documentId: document.id });
    documents.value = documents.value.filter((d) => d.id !== document.id);
    closing.value = null;
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = '';
  }
}
onBeforeUnmount(() => {
  disposed = true;
});
defineExpose({ openFile });
</script>
<template>
  <div v-if="documents.length || busy || error" class="external-documents">
    <div class="external-documents-heading">
      <strong><Icon name="ExternalLink" :size="14" />Extern geöffnet</strong
      ><span class="muted">Im Editor speichern und schließen</span>
    </div>
    <p v-if="documents.length" class="external-documents-hint">
      Arbeitskopien · Text bis 8 MiB · Original wird vor dem Speichern auf Änderungen geprüft.
    </p>
    <div v-for="document in documents" :key="document.id" class="external-document">
      <span class="external-document-name" :title="document.path"
        ><Icon name="FileText" :size="15" /><span class="ellipsis">{{ document.name }}</span
        ><span class="small-badge">{{ document.remote ? 'Remote' : 'Lokal' }}</span></span
      >
      <button
        class="icon-button"
        :disabled="!!busy || document.autoFinish"
        :title="`${document.name} erneut im Standardprogramm öffnen`"
        @click="reopen(document)"
      >
        <Icon name="ExternalLink" :size="14" />
      </button>
      <button
        class="button secondary small"
        :disabled="!!busy || document.autoFinish || (document.remote && !session.connected)"
        title="Nach dem Speichern und Schließen im Editor: Änderungen übernehmen und Arbeitskopie löschen"
        @click="finish(document)"
      >
        <Icon
          :name="busy === document.id ? 'LoaderCircle' : document.remote ? 'Upload' : 'Check'"
          :class="{ spin: busy === document.id }"
          :size="14"
        />{{ document.autoFinish ? 'Wartet auf Editor …' : 'Bearbeitung beenden' }}
      </button>
      <button
        class="icon-button"
        :disabled="!!busy"
        :title="`${document.name}: Arbeitskopie schließen`"
        @click="closing = document"
      >
        <Icon name="X" :size="14" />
      </button>
      <p class="muted external-text-copy">{{ document.editorNote }}</p>
      <p v-if="document.error" class="inline-error external-text-copy" role="alert">
        {{ document.error }}
      </p>
      <p v-if="document.textCopy" class="muted external-text-copy">
        Als .txt geöffnet, damit ausführbare oder unbekannte Dateizuordnungen keinen Code starten.
      </p>
    </div>
    <p v-if="busy === '@open'" class="muted">Textdatei wird geöffnet …</p>
    <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
  </div>
  <Modal
    v-if="closing"
    title="Arbeitskopie verwerfen?"
    :subtitle="closing.name"
    :busy="!!busy"
    @close="closing = null"
  >
    <div class="form-body">
      <p>
        Die lokale Arbeitskopie wird ohne weiteres Zurückschreiben gelöscht. Noch nicht übernommene
        Änderungen gehen verloren. Bitte auch das externe Dokument schließen.
      </p>
      <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
      <div class="modal-actions">
        <button class="button secondary" :disabled="!!busy" @click="closing = null">Zurück</button
        ><button class="button danger" :disabled="!!busy" @click="close">
          Arbeitskopie verwerfen
        </button>
      </div>
    </div>
  </Modal>
</template>
<style scoped>
.external-documents {
  flex-shrink: 0;
  padding: 0.7rem 0.8rem;
  border-bottom: 1px solid var(--border);
  background: var(--panel);
  max-height: 40%;
  overflow: auto;
}
.external-documents-heading,
.external-document {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  flex-wrap: wrap;
}
.external-documents-heading {
  justify-content: space-between;
  gap: 0.5rem;
  font-size: 0.85rem;
}
.external-documents-heading strong {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}
.external-documents-hint {
  font-size: 0.85rem;
  color: var(--muted);
  margin: 0.4rem 0;
}
.external-document {
  padding: 0.3rem 0;
}
.external-document-name {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  flex: 1;
  min-width: 100px;
}
.external-text-copy {
  flex-basis: 100%;
  font-size: 0.85rem;
  margin: 0.2rem 0;
}
</style>
