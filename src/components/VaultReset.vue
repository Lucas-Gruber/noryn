<script setup lang="ts">
import { errorText, t as tr } from '../i18n';

import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { desktop, notify, resetVault, store } from '../composables/useNoryn';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
const emit = defineEmits<{ close: []; done: [] }>();
const preview = ref<{ id: string } | null>(null);
const preparing = ref(false);
const busy = ref(false);
const error = ref('');
const password = ref('');
const repeat = ref('');
const confirmation = ref('');
const ready = computed(
  () => !!preview.value && confirmation.value === 'ZURÜCKSETZEN' && !preparing.value && !busy.value,
);
let disposed = false;
async function prepare() {
  if (!desktop) {
    error.value = tr('Der lokale Tresor ist in der Desktop-App verfügbar.');
    return;
  }
  preparing.value = true;
  error.value = '';
  preview.value = null;
  try {
    const value = await invoke<{ id: string }>('vault_reset_preview');
    if (disposed) {
      await invoke('vault_reset_cancel', { id: value.id });
      return;
    }
    preview.value = value;
  } catch (reason) {
    if (!disposed) error.value = String(reason);
  } finally {
    preparing.value = false;
  }
}
async function submit() {
  error.value = '';
  if (!ready.value) {
    error.value = tr('Zur Bestätigung bitte ZURÜCKSETZEN eingeben.');
    return;
  }
  if ([...password.value].length < 12) {
    error.value = tr('Das neue Passwort muss mindestens 12 Zeichen lang sein.');
    return;
  }
  if (password.value !== repeat.value) {
    error.value = tr('Die neuen Passwörter stimmen nicht überein.');
    return;
  }
  busy.value = true;
  try {
    await resetVault(preview.value!.id, confirmation.value, password.value);
    preview.value = null;
    notify(tr('Ein neuer, leerer Tresor wurde erstellt.'));
    emit('done');
  } catch (reason) {
    error.value = String(reason);
  } finally {
    password.value = '';
    repeat.value = '';
    busy.value = false;
  }
}
function close() {
  if (!busy.value) emit('close');
}
onMounted(prepare);
onBeforeUnmount(() => {
  disposed = true;
  password.value = '';
  repeat.value = '';
  if (preview.value && desktop)
    void invoke('vault_reset_cancel', { id: preview.value.id }).catch(() => {});
});
</script>
<template>
  <Modal
    :title="tr('Tresor zurücksetzen')"
    :subtitle="tr('Neuer Anfang auf diesem Gerät')"
    wide
    :busy="busy"
    @close="close"
  >
    <form class="form-body vault-reset-form" novalidate @submit.prevent="submit">
      <div class="vault-reset-warning">
        <Icon name="ShieldAlert" :size="23" />
        <div>
          <strong>{{ tr('Die lokalen Tresor-Daten werden ersetzt.') }}</strong>
          <p>
            {{
              tr(
                'Alle lokalen Verbindungen, Zugangsdaten, persönlichen Zuordnungen, Team-Konfigurationen und Team-Schlüssel gehen in dieser Installation verloren.',
              )
            }}
          </p>
        </div>
      </div>
      <p>
        {{
          tr(
            'Ohne das bisherige Passwort sind die verschlüsselten Daten nicht wiederherstellbar. Du erstellst einen',
          )
        }}
        <strong>{{ tr('neuen, leeren Tresor') }}</strong> {{ tr('mit einem neuen Passwort.') }}
      </p>
      <p class="muted">
        {{
          tr(
            'Team-Server, exportierte Backups und frühere Migrationssicherungen bleiben erhalten. Teams richtest du danach neu ein; Backups importierst du mit ihrem eigenen Passwort.',
          )
        }}
      </p>
      <details v-if="store.vaultPath" class="vault-reset-path">
        <summary>{{ tr('Betroffene lokale Datei') }}</summary>
        <code>{{ store.vaultPath }}</code>
      </details>
      <p v-if="preparing" role="status">
        <Icon name="LoaderCircle" :size="14" class="spin" />
        {{ tr('Lokaler Tresor wird geprüft …') }}
      </p>
      <div class="vault-reset-passwords">
        <label
          >{{ tr('Neues Tresor-Passwort')
          }}<input
            v-model="password"
            type="password"
            autocomplete="new-password"
            :disabled="busy || preparing"
            :placeholder="tr('Mindestens 12 Zeichen')"
        /></label>
        <label
          >{{ tr('Neues Passwort wiederholen')
          }}<input
            v-model="repeat"
            type="password"
            autocomplete="new-password"
            :disabled="busy || preparing"
        /></label>
      </div>
      <label
        >{{ tr('Zur Bestätigung ZURÜCKSETZEN eingeben')
        }}<input
          v-model="confirmation"
          autocomplete="off"
          spellcheck="false"
          :disabled="busy || preparing"
          :placeholder="tr('ZURÜCKSETZEN')"
      /></label>
      <p v-if="error" class="backup-error" role="alert">{{ errorText(error) }}</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" :disabled="busy" @click="close">
          {{ tr('Abbrechen') }}</button
        ><button
          v-if="!preview && !preparing"
          type="button"
          class="button secondary"
          @click="prepare"
        >
          {{ tr('Erneut prüfen') }}</button
        ><button type="submit" class="button danger" :disabled="!ready">
          <Icon :name="busy ? 'LoaderCircle' : 'Trash2'" :class="{ spin: busy }" :size="16" />{{
            busy ? tr('Wird zurückgesetzt …') : tr('Tresor ersetzen')
          }}
        </button>
      </div>
    </form>
  </Modal>
</template>
<style scoped>
.vault-reset-form {
  gap: 1rem;
}
.vault-reset-passwords {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1rem;
}
@media (max-width: 640px) {
  .vault-reset-passwords {
    grid-template-columns: 1fr;
  }
}
.vault-reset-form .modal-actions {
  flex-wrap: wrap;
}
.vault-reset-form > p {
  margin: 0;
  line-height: 1.65;
}
.vault-reset-warning {
  display: flex;
  align-items: flex-start;
  gap: 0.8rem;
  padding: 1rem;
  border: 1px solid var(--danger);
  border-radius: 0.6rem;
  background: var(--danger-bg);
}
.vault-reset-warning > svg {
  flex-shrink: 0;
  color: var(--danger);
}
.vault-reset-warning strong {
  color: var(--text-strong);
}
.vault-reset-warning p {
  margin: 0.5rem 0 0;
  line-height: 1.65;
  color: var(--text-secondary);
}
.vault-reset-path {
  color: var(--text-secondary);
}
.vault-reset-path summary {
  cursor: pointer;
}
.vault-reset-path code {
  display: block;
  margin-top: 0.65rem;
  overflow-wrap: anywhere;
  font-size: 0.928571rem;
}
</style>
