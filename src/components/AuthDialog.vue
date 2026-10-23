<script setup lang="ts">
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { AuthChallenge } from '../types';
import { action, store } from '../composables/useNoryn';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
const props = defineProps<{ challenge: AuthChallenge }>();
const answers = ref(props.challenge.prompts.map(() => ''));
const busy = ref(false);
async function reply(cancel = false) {
  busy.value = true;
  await action(async () => {
    await invoke('auth_respond', {
      id: props.challenge.id,
      answers: cancel ? null : answers.value,
    });
    answers.value = [];
    store.authPrompt = null;
  });
  busy.value = false;
}
</script>
<template>
  <Modal
    title="Anmeldung bestätigen"
    :subtitle="`${challenge.username || 'SSH'} · ${challenge.host}`"
    @close="reply(true)"
    ><form class="form-body" @submit.prevent="reply()">
      <div class="auth-intro">
        <Icon name="ShieldCheck" :size="24" />
        <p>
          {{
            challenge.instructions ||
            'Der Server benötigt weitere Angaben, beispielsweise einen Einmalcode deiner Authenticator-App.'
          }}
        </p>
      </div>
      <label v-for="(prompt, index) in challenge.prompts" :key="index"
        >{{ prompt.text || `Antwort ${index + 1}`
        }}<input
          v-model="answers[index]"
          :type="prompt.echo ? 'text' : 'password'"
          autocomplete="one-time-code"
          :autofocus="index === 0"
          required
      /></label>
      <p class="muted">
        Diese Antworten werden nur für die aktuelle Anmeldung verwendet und nicht gespeichert.
      </p>
      <div class="modal-actions">
        <button type="button" class="button secondary" :disabled="busy" @click="reply(true)">
          Abbrechen</button
        ><button class="button primary" :disabled="busy">Anmeldung fortsetzen</button>
      </div>
    </form></Modal
  >
</template>
