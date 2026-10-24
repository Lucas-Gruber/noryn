<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { desktop, notify, store } from '../composables/useNoryn';
import type { VaultView } from '../types';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import SelectMenu from './SelectMenu.vue';

type Scope = { kind: 'all' } | { kind: 'private' } | { kind: 'team'; teamId: string };
interface Preview {
  id: string;
  summary: {
    scope: Scope;
    label: string;
    nodes: number;
    credentials: number;
    teams: number;
    replacedNodes: number;
    replacedCredentials: number;
  };
}
const mode = ref<'export' | 'import' | null>(null);
const selected = ref('all');
const password = ref('');
const repeat = ref('');
const memberName = ref('');
const busy = ref(false);
const error = ref('');
const file = ref('');
const preview = ref<Preview | null>(null);
const confirmed = ref(false);
const options = computed(() => [
  { value: 'all', label: 'Alle Tresore · vollständige Sicherung' },
  { value: 'private', label: 'Nur privater Tresor' },
  ...store.view.teams.map((team) => ({ value: `team:${team.id}`, label: `Team · ${team.name}` })),
]);
const description = computed(() =>
  selected.value === 'all'
    ? 'Enthält private und Team-Einträge, alle Zugangsdaten, Team-Schlüssel und Server-Tokens sowie persönliche Zuordnungen, Ordner und Hostschlüssel.'
    : selected.value === 'private'
      ? 'Enthält private Einträge und Zugangsdaten mit persönlicher Ablage. Zuordnungen zu Konten anderer Tresore sind nicht enthalten.'
      : 'Enthält den Team-Baum, freigegebene Zugangsdaten, Verlauf sowie Team-Schlüssel und Server-Token. Persönliche Konten, Kontoauswahl, Namen und Ablage bleiben auf diesem Gerät.',
);
const sessionsOpen = computed(() => store.sessions.length > 0 || store.loading);
function start(value: 'export' | 'import') {
  mode.value = value;
  selected.value =
    store.workspace !== '*' && store.workspace !== 'private' ? `team:${store.workspace}` : 'all';
  error.value = '';
}
async function cancelPreview() {
  const id = preview.value?.id;
  preview.value = null;
  if (desktop && id) await invoke('vault_import_cancel', { id }).catch(() => {});
}
async function close() {
  if (busy.value) return;
  await cancelPreview();
  mode.value = null;
  password.value = repeat.value = memberName.value = file.value = error.value = '';
  confirmed.value = false;
}
async function exportVault() {
  error.value = '';
  if (password.value !== repeat.value) {
    error.value = 'Die Passwörter stimmen nicht überein.';
    return;
  }
  if ([...password.value].length < 12) {
    error.value = 'Bitte mindestens 12 Zeichen verwenden.';
    return;
  }
  busy.value = true;
  try {
    const path = await save({
      defaultPath: `Noryn-${new Intl.DateTimeFormat('sv-SE').format(new Date())}.noryn`,
      filters: [{ name: 'Noryn-Sicherung', extensions: ['noryn'] }],
    });
    if (!path) return;
    const scope: Scope =
      selected.value === 'all'
        ? { kind: 'all' }
        : selected.value === 'private'
          ? { kind: 'private' }
          : { kind: 'team', teamId: selected.value.slice(5) };
    await invoke('vault_export', { path, scope, password: password.value });
    busy.value = false;
    await close();
    notify('Verschlüsselte Sicherung gespeichert');
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function selectFile() {
  const path = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Noryn-Sicherung', extensions: ['noryn'] }],
  });
  if (typeof path === 'string') file.value = path;
}
async function inspect() {
  busy.value = true;
  error.value = '';
  try {
    preview.value = await invoke<Preview>('vault_import_preview', {
      path: file.value,
      password: password.value,
    });
    password.value = '';
    confirmed.value = false;
    const scope = preview.value.summary.scope;
    memberName.value =
      scope.kind === 'team'
        ? store.view.teams.find((t) => t.id === scope.teamId)?.memberName || ''
        : '';
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function applyImport() {
  if (!preview.value || !confirmed.value) return;
  busy.value = true;
  error.value = '';
  try {
    store.view = await invoke<VaultView>('vault_import_apply', {
      id: preview.value.id,
      memberName: memberName.value,
    });
    preview.value = null;
    store.selected = '';
    store.folder = 'all';
    if (
      store.workspace !== '*' &&
      store.workspace !== 'private' &&
      !store.view.teams.some((t) => t.id === store.workspace)
    )
      store.workspace = 'private';
    busy.value = false;
    await close();
    notify('Sicherung importiert. Team-Änderungen sind zunächst lokal.');
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
onBeforeUnmount(() => {
  password.value = repeat.value = '';
  void cancelPreview();
});
</script>

<template>
  <section class="settings-section" aria-labelledby="backup-heading">
    <div class="settings-section-head">
      <div>
        <h2 id="backup-heading">Tresore sichern & übertragen</h2>
        <p>Verschlüsselter Export und Import für deine privaten und Team-Tresore.</p>
      </div>
      <Icon name="ShieldCheck" :size="24" />
    </div>
    <p class="backup-description">
      Schütze jede Sicherung mit einem eigenen Passwort. Du kannst alle Tresore gemeinsam oder einen
      einzelnen Bereich exportieren.
    </p>
    <div class="backup-actions">
      <button class="button secondary" @click="start('export')">
        <Icon name="Download" :size="17" />Exportieren
      </button>
      <button class="button secondary" @click="start('import')">
        <Icon name="Upload" :size="17" />Importieren
      </button>
    </div>
    <p v-if="!desktop" class="muted">
      Export und Import sind in der Desktop-App verfügbar. Die Browser-Vorschau enthält nur
      Beispieldaten.
    </p>
  </section>
  <Modal
    v-if="mode"
    :title="mode === 'export' ? 'Tresor exportieren' : 'Sicherung importieren'"
    subtitle="Noryn-Sicherung · Argon2id + XChaCha20-Poly1305"
    @close="close"
  >
    <form
      class="form-body"
      @submit.prevent="mode === 'export' ? exportVault() : preview ? applyImport() : inspect()"
    >
      <template v-if="mode === 'export'">
        <label
          >Bereich<SelectMenu
            v-model="selected"
            :options="options"
            label="Exportbereich"
            :disabled="busy"
        /></label>
        <p class="backup-description">{{ description }}</p>
        <label
          >Backup-Passwort<input
            v-model="password"
            type="password"
            autocomplete="new-password"
            required
            minlength="12"
            :disabled="busy"
        /></label>
        <label
          >Passwort wiederholen<input
            v-model="repeat"
            type="password"
            autocomplete="new-password"
            required
            :disabled="busy"
        /></label>
        <p class="muted">
          Das Backup-Passwort wird nicht gespeichert. Bewahre es sicher auf. Vorhandene Dateien
          werden nicht überschrieben.
        </p>
      </template>
      <template v-else-if="!preview">
        <label
          >Sicherungsdatei<button
            type="button"
            class="button secondary backup-file"
            :disabled="busy || !desktop"
            @click="selectFile().catch((e) => (error = String(e)))"
          >
            <Icon name="FolderOpen" :size="17" />{{ file || 'Datei auswählen …' }}
          </button></label
        >
        <label
          >Backup-Passwort<input
            v-model="password"
            type="password"
            autocomplete="off"
            required
            :disabled="busy"
        /></label>
        <p class="muted">
          Zuerst wird die Sicherung entschlüsselt und geprüft. Dabei wird noch nichts geändert.
        </p>
      </template>
      <template v-else>
        <div class="backup-summary">
          <strong>{{ preview.summary.label }}</strong
          ><span
            >{{ preview.summary.nodes }} Einträge · {{ preview.summary.credentials }} Konten ·
            {{ preview.summary.teams }} Teams</span
          >
        </div>
        <p class="backup-description">
          Der gesicherte Bereich ersetzt dort {{ preview.summary.replacedNodes }} vorhandene
          Einträge und {{ preview.summary.replacedCredentials }} Konten.
          {{
            preview.summary.scope.kind === 'all'
              ? 'Alle Tresore werden wiederhergestellt.'
              : 'Andere Tresore bleiben erhalten.'
          }}
          Dein aktuelles Tresor-Passwort bleibt bestehen.
        </p>
        <label v-if="preview.summary.teams"
          >Dein Name für importierte Teams<input
            v-model="memberName"
            maxlength="80"
            required
            :disabled="busy"
          /><span class="muted"
            >Bestehende persönliche Team-Namen bleiben erhalten. Der Import wird im Team-Verlauf
            erfasst.</span
          ></label
        >
        <p class="muted">
          Zuordnungen zu entfernten Konten werden bereinigt. Die Vorschau gilt fünf Minuten.
          Team-Änderungen werden erst beim nächsten Abgleich übertragen. SSH-Agent-Konten verwenden
          den Agenten dieses Geräts.
        </p>
        <p v-if="sessionsOpen" class="backup-error">
          Bitte zuerst alle Verbindungen und Anmeldungen schließen.
        </p>
        <label class="checkbox-row"
          ><input v-model="confirmed" type="checkbox" :disabled="busy" />Ich möchte den angezeigten
          Bereich durch die Sicherung ersetzen.</label
        >
      </template>
      <p v-if="error" class="backup-error" role="alert">{{ error }}</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" :disabled="busy" @click="close">
          Abbrechen
        </button>
        <button
          class="button primary"
          :disabled="
            busy ||
            !desktop ||
            (mode === 'import' && (!file || (!!preview && (!confirmed || sessionsOpen))))
          "
        >
          <Icon v-if="busy" name="LoaderCircle" :size="16" />{{
            mode === 'export'
              ? 'Sicherung speichern'
              : preview
                ? 'Bereich wiederherstellen'
                : 'Vorschau öffnen'
          }}
        </button>
      </div>
    </form>
  </Modal>
</template>
