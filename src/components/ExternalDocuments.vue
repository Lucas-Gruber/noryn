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
}
const props = defineProps<{ session: ClientSession }>();
const emit = defineEmits<{ saved: [] }>();
const documents = ref<Document[]>([]),
  busy = ref(''),
  error = ref('');
const closing = ref<Document | null>(null);
let disposed = false;
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
        };
    if (disposed) return;
    if (!documents.value.some((d) => d.id === document.id)) documents.value.push(document);
    notify(
      desktop
        ? 'Arbeitskopie im Standardprogramm geöffnet. Erst dort speichern, dann hier zurückschreiben.'
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
    if (desktop) await invoke('text_reopen', { id: props.session.id, documentId: document.id });
    else notify('Vorschau: Öffnet die Arbeitskopie im Standardprogramm.');
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = '';
  }
}
async function save(document: Document) {
  if (busy.value) return;
  busy.value = document.id;
  error.value = '';
  try {
    if (!desktop) {
      notify('Vorschau: In der Desktop-App werden Änderungen bewusst zurückgeschrieben.');
      return;
    }
    const changed = await invoke<boolean>('text_save', {
      id: props.session.id,
      documentId: document.id,
    });
    if (!disposed) {
      notify(
        changed
          ? document.remote
            ? 'Änderungen auf den Server hochgeladen.'
            : 'Änderungen in die lokale Originaldatei gespeichert.'
          : 'Keine gespeicherten Änderungen in der Arbeitskopie.',
      );
      if (changed) emit('saved');
    }
  } catch (reason) {
    if (!disposed) error.value = String(reason);
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
      ><span class="muted">Im Editor speichern → hier zurückschreiben</span>
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
        :disabled="!!busy"
        :title="`${document.name} erneut im Standardprogramm öffnen`"
        @click="reopen(document)"
      >
        <Icon name="ExternalLink" :size="14" />
      </button>
      <button
        class="button secondary small"
        :disabled="!!busy || (document.remote && !session.connected)"
        :title="
          document.remote
            ? 'Gespeicherte Änderungen auf den Server hochladen'
            : 'Gespeicherte Änderungen in die lokale Originaldatei übernehmen'
        "
        @click="save(document)"
      >
        <Icon
          :name="busy === document.id ? 'LoaderCircle' : document.remote ? 'Upload' : 'Check'"
          :class="{ spin: busy === document.id }"
          :size="14"
        />{{ document.remote ? 'Hochladen' : 'Speichern' }}
      </button>
      <button
        class="icon-button"
        :disabled="!!busy"
        :title="`${document.name}: Arbeitskopie schließen`"
        @click="closing = document"
      >
        <Icon name="X" :size="14" />
      </button>
      <p v-if="document.textCopy" class="muted external-text-copy">
        Als .txt geöffnet, damit ausführbare oder unbekannte Dateizuordnungen keinen Code starten.
      </p>
    </div>
    <p v-if="busy === '@open'" class="muted">Textdatei wird geöffnet …</p>
    <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
  </div>
  <Modal
    v-if="closing"
    title="Arbeitskopie schließen?"
    :subtitle="closing.name"
    :busy="!!busy"
    @close="closing = null"
  >
    <div class="form-body">
      <p>
        Die lokale Arbeitskopie wird gelöscht. Änderungen im externen Editor zuerst speichern und
        hier {{ closing.remote ? 'hochladen' : 'in die Originaldatei übernehmen' }}. Es wird nichts
        automatisch zurückgeschrieben.
      </p>
      <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
      <div class="modal-actions">
        <button class="button secondary" :disabled="!!busy" @click="closing = null">Zurück</button
        ><button class="button danger" :disabled="!!busy" @click="close">
          Arbeitskopie schließen
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
