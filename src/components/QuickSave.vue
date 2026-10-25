<script setup lang="ts">
import { errorText, t as tr } from '../i18n';

import { computed, ref } from 'vue';
import type { ClientSession } from '../types';
import { saveQuickSession, store } from '../composables/useNoryn';
import Modal from './Modal.vue';
import SelectMenu from './SelectMenu.vue';
import Icon from './Icon.vue';
const props = defineProps<{ session: ClientSession }>();
const emit = defineEmits<{ close: [] }>();
const name = ref(props.session.name),
  parent = ref<string | null>(null),
  busy = ref(false),
  error = ref('');
const folders = computed(() =>
  store.view.nodes.filter((n) => n.workspace === 'private' && n.kind === 'folder'),
);
async function submit() {
  if (!name.value.trim()) {
    error.value = tr('Bitte einen Namen eingeben.');
    return;
  }
  busy.value = true;
  error.value = '';
  try {
    await saveQuickSession(props.session, name.value.trim(), parent.value);
    emit('close');
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <Modal
    :title="tr('Verbindung speichern')"
    :subtitle="tr('Im privaten Tresor · die Sitzung bleibt geöffnet')"
    :busy="busy"
    @close="$emit('close')"
  >
    <form class="form-body" novalidate @submit.prevent="submit">
      <p>
        <strong>{{ session.username }}@{{ session.host }}:{{ session.port }}</strong> ·
        {{ session.protocol.toUpperCase() }}
      </p>
      <label
        >{{ tr('Name') }}<input v-model="name" maxlength="512" :disabled="busy" autofocus
      /></label>
      <label
        >{{ tr('Privater Ordner')
        }}<SelectMenu
          v-model="parent"
          :label="tr('Privater Zielordner')"
          :disabled="busy"
          :options="[
            {
              value: null,
              get label() {
                return tr('Hauptebene');
              },
            },
            ...folders.map((n) => ({ value: n.id, label: n.name })),
          ]"
      /></label>
      <p class="muted">
        {{
          tr(
            'Direkt eingegebene Zugangsdaten werden verschlüsselt an dieser privaten Verbindung gespeichert. Ein ausgewähltes Konto wird persönlich zugeordnet.',
          )
        }}
      </p>
      <p v-if="error" class="backup-error" role="alert">{{ errorText(error) }}</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" :disabled="busy" @click="$emit('close')">
          {{ tr('Abbrechen') }}</button
        ><button class="button primary" :disabled="busy">
          <Icon name="Check" :size="16" />{{ tr('Speichern') }}
        </button>
      </div>
    </form>
  </Modal>
</template>
