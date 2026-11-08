<script setup lang="ts">
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { desktop } from '../composables/useNoryn';
import { errorText, t as tr } from '../i18n';
import Icon from './Icon.vue';
const busy = ref(false),
  message = ref(''),
  error = ref('');
async function register() {
  busy.value = true;
  message.value = '';
  error.value = '';
  try {
    message.value = await invoke<string>('links_register');
  } catch (cause) {
    error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <section class="settings-section" aria-labelledby="connection-links-heading">
    <div class="settings-section-head">
      <div>
        <h2 id="connection-links-heading">{{ tr('Verbindungslinks') }}</h2>
        <p>
          {{
            tr(
              'ssh://, sftp://, ftp://, ftps:// und rdp:// mit Noryn öffnen. Die Anmeldung startet erst nach deiner Bestätigung.',
            )
          }}
        </p>
      </div>
      <button class="button secondary" :disabled="busy || !desktop" @click="register">
        <Icon name="Link" :size="16" />{{ tr('Mit Noryn öffnen') }}
      </button>
    </div>
    <p class="muted">
      {{
        tr(
          'Links lassen sich auch in das Adressfeld von „Schnell verbinden“ einfügen. Passwörter gehören in den Anmeldedialog.',
        )
      }}
    </p>
    <p v-if="message" role="status">{{ tr(message) }}</p>
    <p v-if="error" class="inline-error" role="alert">{{ errorText(error) }}</p>
  </section>
</template>
