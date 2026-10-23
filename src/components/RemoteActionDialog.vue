<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, SystemReply, SystemRequest } from '../types';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
const props = defineProps<{
  session: ClientSession;
  request: SystemRequest;
  title: string;
  description: string;
  elevated: boolean;
  automatic?: boolean;
}>();
const emit = defineEmits<{
  close: [];
  completed: [reply: Extract<SystemReply, { type: 'completed' }>];
}>();
const elevated = ref(props.elevated),
  ask = ref(false),
  password = ref(''),
  busy = ref(false),
  error = ref('');
let disposed = false;
async function run() {
  busy.value = true;
  error.value = '';
  try {
    const reply: SystemReply = props.session.preview
      ? {
          type: 'completed',
          output: 'Browser-Vorschau: Aktion simuliert.',
          startTicks: null,
          storage: null,
        }
      : await invoke('ssh_system_control', {
          id: props.session.id,
          request: props.request,
          elevated: elevated.value,
          password: elevated.value && ask.value ? password.value : null,
        });
    password.value = '';
    if (disposed) return;
    if (reply.type === 'passwordRequired') {
      ask.value = true;
      error.value = reply.message || 'Für diese Aktion ist ein sudo-Passwort erforderlich.';
    } else emit('completed', reply);
  } catch (e) {
    if (!disposed) error.value = String(e);
  } finally {
    busy.value = false;
  }
}
onMounted(() => {
  if (props.automatic) void run();
});
onBeforeUnmount(() => {
  disposed = true;
  password.value = '';
});
</script>
<template>
  <Modal :title="title" :subtitle="session.host" @close="!busy && emit('close')"
    ><form class="form-body" @submit.prevent="run">
      <p>{{ description }}</p>
      <p v-if="error" class="error-message" role="alert">{{ error }}</p>
      <label class="checkbox-row"
        ><input v-model="elevated" type="checkbox" :disabled="busy" />Mit sudo ausführen</label
      >
      <p v-if="elevated" class="muted">
        Zuerst passwortloses sudo, danach das gespeicherte Benutzerpasswort. Eine gestartete Aktion
        wird nicht automatisch wiederholt.
      </p>
      <label v-if="elevated" class="checkbox-row"
        ><input v-model="ask" type="checkbox" :disabled="busy" />Passwort für diese Aktion selbst
        eingeben</label
      >
      <label v-if="elevated && ask"
        >sudo-Passwort<input
          v-model="password"
          type="password"
          autocomplete="off"
          required
          autofocus
          :disabled="busy"
      /></label>
      <div class="modal-actions">
        <button class="button secondary" type="button" :disabled="busy" @click="emit('close')">
          Abbrechen</button
        ><button class="button primary" :disabled="busy || !session.connected">
          <Icon :name="busy ? 'LoaderCircle' : 'Check'" :class="{ spin: busy }" :size="16" />{{
            busy ? 'Wird ausgeführt …' : 'Ausführen'
          }}
        </button>
      </div>
    </form></Modal
  >
</template>
