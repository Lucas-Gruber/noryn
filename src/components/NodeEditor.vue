<script setup lang="ts">
import SelectMenu from './SelectMenu.vue';
import { computed, reactive, ref, onBeforeUnmount, watch } from 'vue';
import type { NodeEntry } from '../types';
import { isFileProtocol } from '../protocols';
import { ALL_WORKSPACES } from '../workspaces';
import { descendants, notify, saveNode, store } from '../composables/useNoryn';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
import ConnectionLoginFields from './ConnectionLoginFields.vue';
import { emptyCredential, clearCredential } from '../connectionLogins';
const props = defineProps<{ node?: NodeEntry; kind?: 'folder' | 'connection'; direct?: boolean }>();
const emit = defineEmits<{ close: [] }>();
const original = props.node ? { ...props.node, tags: [...props.node.tags] } : null;
const error = ref('');
const revision = store.view.localRevision;
const savedDirect = props.node && store.view.connectionCredentials?.[props.node.id];
const access = ref(savedDirect || props.direct ? 'direct' : 'account');
const direct = reactive({
  ...emptyCredential(),
  username: savedDirect?.username || '',
  kind: savedDirect?.kind || 'password',
});
onBeforeUnmount(() => clearCredential(direct));
const parentWorkspace = store.view.nodes.find((node) => node.id === store.folder)?.workspace;
const defaultWorkspace =
  parentWorkspace ||
  (store.workspace === ALL_WORKSPACES
    ? store.view.nodes.find((node) => node.id === store.selected)?.workspace || 'private'
    : store.workspace);
const form = reactive<NodeEntry>(
  props.node
    ? { ...props.node, initialPath: props.node.initialPath || '', tags: [...props.node.tags] }
    : {
        id: crypto.randomUUID(),
        parentId: store.folder === 'all' ? null : store.folder,
        workspace: defaultWorkspace,
        kind: props.kind || 'connection',
        name: '',
        protocol: 'ssh',
        host: '',
        port: 22,
        initialPath: '',
        notes: '',
        tags: [],
        favorite: false,
      },
);
watch(
  () => form.workspace,
  () => {
    access.value = 'account';
    clearCredential(direct);
  },
);
const tags = ref(form.tags.join(', '));
const binding = ref(store.view.bindings[form.id] || '');
const busy = ref(false);
const folders = computed(() =>
  store.view.nodes.filter(
    (n) => n.workspace === form.workspace && n.kind === 'folder' && !descendants(form.id).has(n.id),
  ),
);
async function submit() {
  busy.value = true;
  error.value = '';
  try {
    form.initialPath =
      isFileProtocol(form.protocol) && form.kind === 'connection' ? form.initialPath : '';
    form.tags = tags.value
      .split(',')
      .map((t) => t.trim())
      .filter(Boolean);
    if (access.value === 'direct' && !direct.username.trim())
      throw new Error('Bitte einen Benutzernamen eingeben.');
    await saveNode(
      { ...form, parentId: form.parentId || null },
      original,
      access.value === 'direct'
        ? {
            mode: 'direct',
            credential: {
              ...direct,
              kind: ['ftp', 'ftps'].includes(form.protocol) ? 'password' : direct.kind,
            },
          }
        : { mode: 'account', credentialId: binding.value || null },
      revision,
    );
    clearCredential(direct);
    store.selected = form.id;
    notify(
      props.node
        ? 'Änderungen gespeichert'
        : form.kind === 'folder'
          ? 'Ordner angelegt'
          : 'Verbindung angelegt',
    );
    emit('close');
  } catch (e) {
    error.value = String(e);
  }
  busy.value = false;
}
</script>
<template>
  <Modal
    :title="`${node ? 'Bearbeiten' : kind === 'folder' ? 'Neuer Ordner' : 'Neue Verbindung'}`"
    :subtitle="
      form.workspace === 'private'
        ? 'Nur in deinem privaten Arbeitsbereich.'
        : 'Dieser Eintrag wird mit deinem Team geteilt.'
    "
    :busy="busy"
    @close="emit('close')"
  >
    <form @submit.prevent="submit" class="form-body">
      <label v-if="!node && store.workspace === ALL_WORKSPACES">
        Arbeitsbereich<SelectMenu
          v-model="form.workspace"
          label="Arbeitsbereich"
          :options="[
            ...store.view.teams.map((t) => ({ value: t.id, label: t.name })),
            { value: 'private', label: 'Privater Workspace · nur für dich' },
          ]"
          @update:model-value="form.parentId = null"
        />
      </label>
      <label
        >Name<input
          v-model="form.name"
          placeholder="z. B. Production Web Server"
          required
          maxlength="512"
          autofocus
      /></label>
      <div class="form-row">
        <label
          >Übergeordneter Ordner<SelectMenu
            v-model="form.parentId"
            label="Übergeordneter Ordner"
            :options="[
              { value: null, label: 'Kein Ordner · Hauptebene' },
              ...folders.map((f) => ({ value: f.id, label: f.name })),
            ]"
        /></label>
        <label v-if="form.kind === 'connection'"
          >Protokoll<SelectMenu
            v-model="form.protocol"
            label="Protokoll"
            :options="
              [
                { value: 'ssh', label: 'SSH + SFTP' },
                { value: 'sftp', label: 'SFTP · nur Dateien' },
                { value: 'ftps', label: 'FTPS · TLS-verschlüsselt' },
                { value: 'ftp', label: 'FTP · unverschlüsselt' },
                { value: 'rdp', label: 'RDP · geplant' },
                { value: 'vnc', label: 'VNC · geplant' },
              ] as const
            "
            @update:model-value="
              form.port = ['ssh', 'sftp'].includes(form.protocol)
                ? 22
                : form.protocol === 'rdp'
                  ? 3389
                  : form.protocol === 'vnc'
                    ? 5900
                    : 21
            "
        /></label>
      </div>
      <p v-if="form.kind === 'connection' && ['ftp', 'ftps'].includes(form.protocol)" class="muted">
        {{
          form.protocol === 'ftps'
            ? 'Explizites FTPS (AUTH TLS), üblicherweise Port 21. Das Serverzertifikat wird geprüft.'
            : 'FTP überträgt Benutzername, Passwort und Dateien unverschlüsselt. Verwende FTPS, wenn dein Server TLS unterstützt.'
        }}
        Wähle einen Zugang mit Benutzerpasswort.
      </p>
      <div v-if="form.kind === 'connection'" class="form-row host-row">
        <label
          >Hostname / IP-Adresse<input
            v-model="form.host"
            placeholder="10.20.1.10"
            required
            maxlength="253" /></label
        ><label
          >Port<input v-model.number="form.port" type="number" min="1" max="65535" required
        /></label>
      </div>
      <label v-if="form.kind === 'connection' && isFileProtocol(form.protocol)">
        Startordner (optional)<input
          v-model="form.initialPath"
          placeholder="/foldery/folderx"
          pattern="/.*"
          maxlength="4096"
        />
        <span class="muted">Leer lassen, um im persönlichen Serverordner zu starten.</span>
      </label>
      <label
        >Kategorien / Tags<input v-model="tags" placeholder="production, web, eu-central"
      /></label>
      <label
        >Notizen<textarea
          v-model="form.notes"
          placeholder="Informationen für dich und dein Team …"
          rows="3"
        />
      </label>
      <div class="local-field">
        <div class="field-heading">
          <Icon name="KeyRound" :size="16" /><span>Meine Zugangsdaten</span
          ><span class="small-badge"><Icon name="LockKeyhole" :size="10" /> Nur lokal</span>
        </div>
        <label v-if="form.workspace === 'private' && form.kind === 'connection'"
          >Zugang verwenden<SelectMenu
            v-model="access"
            label="Zugangsart"
            :options="[
              { value: 'account', label: 'Gespeichertes Konto / Vererbung' },
              { value: 'direct', label: 'Direkt an dieser privaten Verbindung' },
            ]"
        /></label>
        <ConnectionLoginFields
          v-if="access === 'direct' && form.workspace === 'private' && form.kind === 'connection'"
          :form="direct"
          :stored="!!savedDirect"
          :ftp="['ftp', 'ftps'].includes(form.protocol)"
          :disabled="busy"
        />
        <SelectMenu
          v-else
          v-model="binding"
          label="Meine Zugangsdaten"
          :options="[
            { value: '', label: 'Vom übergeordneten Ordner erben' },
            ...store.view.credentials.map((c) => ({
              value: c.id,
              label: c.name + ' · ' + c.username,
            })),
          ]"
        />
        <p>
          {{
            access === 'direct'
              ? 'Dieser Zugang wird verschlüsselt nur an dieser privaten Verbindung gespeichert.'
              : 'Deine Auswahl gilt nur für dich. Jedes Teammitglied verbindet sich mit dem eigenen Konto.'
          }}
        </p>
      </div>
      <p v-if="error" class="backup-error" role="alert">{{ error }}</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" @click="emit('close')">Abbrechen</button
        ><button class="button primary" :disabled="busy">
          <Icon
            :name="busy ? 'LoaderCircle' : 'Check'"
            :class="{ spin: busy }"
            :size="16"
          />Speichern
        </button>
      </div>
    </form>
  </Modal>
</template>
