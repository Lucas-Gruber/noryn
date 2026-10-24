<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { copyNodes, descendants, store } from '../composables/useNoryn';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import SelectMenu from './SelectMenu.vue';
const props = defineProps<{ sourceIds: string[]; label: string; revision: number }>();
const emit = defineEmits<{ close: [] }>();
const originalWorkspace = store.view.nodes.find((n) => props.sourceIds.includes(n.id))?.workspace;
const workspace = ref(
  originalWorkspace === 'private' ? store.view.teams[0]?.id || 'private' : 'private',
);
const parentId = ref('');
const busy = ref(false);
const error = ref('');
const options = computed(() => [
  { value: 'private', label: 'Privater Workspace' },
  ...store.view.teams.map((t) => ({ value: t.id, label: `Team · ${t.name}` })),
]);
const count = computed(() => new Set(props.sourceIds.flatMap((id) => [...descendants(id)])).size);
const folders = computed(() => {
  function path(id: string): string {
    const parts: string[] = [],
      seen = new Set<string>();
    let node = store.view.nodes.find((n) => n.id === id);
    while (node && !seen.has(node.id)) {
      parts.unshift(node.name);
      seen.add(node.id);
      node = store.view.nodes.find((n) => n.id === node?.parentId);
    }
    return parts.join(' / ');
  }
  return [
    { value: '', label: 'Oberste Ebene' },
    ...store.view.nodes
      .filter((n) => n.kind === 'folder' && n.workspace === workspace.value)
      .map((n) => ({ value: n.id, label: path(n.id) }))
      .sort((a, b) => a.label.localeCompare(b.label)),
  ];
});
watch(workspace, () => {
  parentId.value = '';
  error.value = '';
});
async function submit() {
  busy.value = true;
  error.value = '';
  try {
    await copyNodes(props.sourceIds, workspace.value, parentId.value || null, props.revision);
    emit('close');
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <Modal title="In Workspace kopieren" :subtitle="label" @close="!busy && emit('close')">
    <form class="form-body" @submit.prevent="submit">
      <p>
        {{ count }} Einträge werden mit ihrer Ordnerstruktur kopiert. Die Quelle bleibt erhalten.
      </p>
      <label
        >Ziel-Workspace<SelectMenu
          v-model="workspace"
          :options="options"
          label="Ziel-Workspace"
          :disabled="busy"
      /></label>
      <label
        >Zielordner<SelectMenu
          v-model="parentId"
          :options="folders"
          label="Zielordner"
          :disabled="busy"
      /></label>
      <p v-if="workspace !== 'private'" class="security-note">
        Verbindungsdaten, Notizen und Kategorien werden im ausgewählten Team geteilt. Bei aktivem
        Live-Sync werden die Kopien automatisch synchronisiert.
      </p>
      <p class="muted">
        Zugangsdaten, persönliche Kontoauswahl, Team-Schlüssel und bisheriger Verlauf werden nicht
        kopiert. Wähle deine Konten anschließend am Ziel aus.
      </p>
      <p v-if="error" class="error-message" role="alert">{{ error }}</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" :disabled="busy" @click="emit('close')">
          Abbrechen
        </button>
        <button class="button primary" :disabled="busy || !count">
          <Icon name="Copy" :size="16" />Kopieren
        </button>
      </div>
    </form>
  </Modal>
</template>
