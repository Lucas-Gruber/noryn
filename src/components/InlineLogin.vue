<script setup lang="ts">
import { onBeforeUnmount, reactive, ref } from 'vue';
import type { NodeEntry } from '../types';
import { clearCredential, emptyCredential } from '../connectionLogins';
import { desktop, notify, saveNode, store } from '../composables/useNoryn';
import { errorText, t as tr } from '../i18n';
import ConnectionLoginFields from './ConnectionLoginFields.vue';

const props = defineProps<{ node: NodeEntry }>();
const emit = defineEmits<{ done: []; close: [] }>();
const original = { ...props.node, tags: [...props.node.tags] };
const revision = store.view.localRevision;
const saved = store.view.connectionCredentials?.[original.id];
const form = reactive({
  ...emptyCredential(),
  username: saved?.username || '',
  kind: saved?.kind || 'password',
});
const busy = ref(false),
  error = ref('');
let disposed = false;
onBeforeUnmount(() => {
  disposed = true;
  clearCredential(form);
});
async function submit() {
  if (busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    if (original.workspace !== 'private')
      throw new Error(
        tr('Direkte Zugangsdaten sind nur für private Ordner und Verbindungen erlaubt.'),
      );
    if (!form.username.trim()) throw new Error(tr('Bitte einen Benutzernamen eingeben.'));
    await saveNode(
      original,
      original,
      {
        mode: 'direct',
        credential: {
          ...form,
          kind:
            original.kind === 'connection' && ['ftp', 'ftps', 'rdp'].includes(original.protocol)
              ? 'password'
              : form.kind,
        },
      },
      revision,
    );
    clearCredential(form);
    if (!disposed) {
      notify(tr('Zugangsdaten gespeichert'));
      emit('done');
    }
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  } finally {
    if (!disposed) busy.value = false;
  }
}
</script>
<template>
  <form
    class="inline-login"
    :aria-label="tr('Direkte Zugangsdaten')"
    :aria-busy="busy || undefined"
    @submit.prevent="submit"
  >
    <strong>{{ tr('Direkte Zugangsdaten') }}</strong>
    <p class="muted">
      {{
        node.kind === 'folder'
          ? tr(
              'Dieser Zugang wird verschlüsselt am privaten Ordner gespeichert und an Untereinträge vererbt.',
            )
          : tr('Dieser Zugang wird verschlüsselt nur an dieser privaten Verbindung gespeichert.')
      }}
    </p>
    <ConnectionLoginFields
      :form="form"
      :stored="!!saved"
      :ftp="node.kind === 'connection' && ['ftp', 'ftps', 'rdp'].includes(node.protocol)"
      :disabled="busy"
    />
    <p v-if="!desktop" class="muted">
      {{ tr('Browser-Vorschau: Es werden keine Passwörter oder Schlüssel gespeichert.') }}
    </p>
    <p v-if="error" class="inline-error" role="alert">{{ errorText(error) }}</p>
    <div class="inline-login-actions">
      <button type="button" class="button secondary" :disabled="busy" @click="emit('close')">
        {{ tr('Abbrechen') }}
      </button>
      <button class="button primary" :disabled="busy">
        {{ busy ? tr('Wird gespeichert …') : tr('Speichern') }}
      </button>
    </div>
  </form>
</template>
<style scoped>
.inline-login {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  margin-top: 0.75rem;
  padding: 1rem;
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  background: var(--panel);
}
.inline-login :deep(.form-row) {
  flex-direction: column;
  gap: 1rem;
}
.inline-login :deep(textarea) {
  max-height: 220px;
}
.inline-login p {
  margin: 0;
}
.inline-login-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 0.6rem;
}
.inline-error {
  color: var(--danger);
  overflow-wrap: anywhere;
}
</style>
