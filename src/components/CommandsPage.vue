<script setup lang="ts">
import { errorText, formatLocale, t as tr } from '../i18n';

import { computed, reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, SavedCommand, VaultView } from '../types';
import { desktop, notify, store } from '../composables/useNoryn';
import SelectMenu from './SelectMenu.vue';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
import CommandBatch from './CommandBatch.vue';
import ScriptLibrary from './ScriptLibrary.vue';
const section = ref('commands');
const batch = ref<SavedCommand>();
const props = defineProps<{ session?: ClientSession }>();
const emit = defineEmits<{ run: [command: SavedCommand]; insert: [command: SavedCommand] }>();
const workspace = ref(
  store.view.nodes.find((n) => n.id === props.session?.nodeId)?.workspace ||
    (store.workspace === '*' ? 'private' : store.workspace),
);
const options = computed(() => [
  {
    value: 'private',
    get label() {
      return tr('Privater Tresor');
    },
  },
  ...store.view.teams.map((t) => ({ value: t.id, label: tr('Team · {p0}', { p0: t.name }) })),
]);
const query = ref(''),
  editing = ref(false),
  error = ref(''),
  busy = ref(false),
  revision = ref(0),
  existing = ref(false);
const deleting = ref<{ command: SavedCommand; revision: number }>();
const draft = reactive<SavedCommand>({
  id: '',
  workspace: 'private',
  name: '',
  description: '',
  command: '',
});
const entries = computed(() =>
  (store.view.commands || []).filter(
    (c) =>
      c.workspace === workspace.value &&
      `${c.name} ${c.description} ${c.command}`
        .toLocaleLowerCase(formatLocale())
        .includes(query.value.toLocaleLowerCase(formatLocale())),
  ),
);
function edit(command?: SavedCommand, copy = false) {
  Object.assign(
    draft,
    command || { workspace: workspace.value, name: '', description: '', command: '' },
    { id: !command || copy ? crypto.randomUUID() : command.id },
  );
  existing.value = !!command && !copy;
  revision.value = store.view.localRevision;
  editing.value = true;
  error.value = '';
}
async function save() {
  if (busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    if (desktop)
      store.view = await invoke<VaultView>('command_save', {
        command: { ...draft },
        expectedRevision: revision.value,
      });
    else {
      store.view.commands = [
        ...(store.view.commands || []).filter((c) => c.id !== draft.id),
        { ...draft },
      ];
      notify(tr('Befehl bleibt nur in dieser Browser-Vorschau.'));
    }
    workspace.value = draft.workspace;
    editing.value = false;
    draft.command = '';
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function remove() {
  if (!deleting.value || busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    if (desktop)
      store.view = await invoke<VaultView>('command_delete', {
        id: deleting.value.command.id,
        expectedRevision: deleting.value.revision,
      });
    else
      store.view.commands = store.view.commands.filter((c) => c.id !== deleting.value!.command.id);
    deleting.value = undefined;
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function copy(command: string) {
  try {
    await navigator.clipboard.writeText(command);
    notify(tr('Befehl kopiert.'));
  } catch {
    error.value = tr('Zwischenablage nicht verfügbar.');
  }
}
</script>
<template>
  <section class="commands-page">
    <div class="library-switch" :aria-label="tr('Sammlung')">
      <button :class="{ active: section === 'commands' }" @click="section = 'commands'">
        {{ tr('Befehle') }}</button
      ><button :class="{ active: section === 'scripts' }" @click="section = 'scripts'">
        {{ tr('Skript-Sammlung') }}
      </button>
    </div>
    <ScriptLibrary v-if="section === 'scripts'" />
    <template v-else>
      <div class="tools-heading">
        <div>
          <span class="eyebrow">{{ tr('BEFEHLS-STORE') }}</span>
          <h2>{{ tr('Weniger tippen. Mehr erledigen.') }}</h2>
          <p>
            {{
              tr('Wiederkehrende Befehle privat ablegen oder verschlüsselt mit deinem Team teilen.')
            }}
          </p>
        </div>
        <button class="button primary" @click="edit()">
          <Icon name="Plus" :size="16" />{{ tr('Neuer Befehl') }}
        </button>
      </div>
      <div class="commands-filters">
        <SelectMenu v-model="workspace" :options="options" :label="tr('Befehls-Tresor')" /><input
          v-model="query"
          :aria-label="tr('Befehle suchen')"
          :placeholder="tr('Befehle suchen …')"
        />
      </div>
      <p v-if="!desktop" class="tools-notice">
        {{ tr('Browser-Vorschau · Änderungen bleiben nur im Arbeitsspeicher.') }}
      </p>
      <p v-if="error && !editing && !deleting" class="error-message" role="alert">
        {{ errorText(error) }}
      </p>
      <div v-if="!entries.length" class="tools-empty">
        <Icon name="Terminal" :size="30" />
        <h3>{{ tr('Deine häufigsten Handgriffe, griffbereit.') }}</h3>
        <p>{{ tr('Lege einen ersten Befehl für diesen Tresor an.') }}</p>
      </div>
      <article v-for="command in entries" :key="command.id" class="command-card">
        <div class="tools-heading">
          <div>
            <h3>{{ command.name }}</h3>
            <p v-if="command.description">{{ command.description }}</p>
          </div>
          <span class="small-badge">{{
            command.workspace === 'private' ? tr('Privat') : tr('Team-Sync')
          }}</span>
        </div>
        <pre>{{ command.command }}</pre>
        <div class="command-actions">
          <button class="button secondary small" @click="copy(command.command)">
            <Icon name="Copy" :size="14" />{{ tr('Kopieren') }}</button
          ><button
            v-if="session && !command.command.includes('\n')"
            class="button secondary small"
            :disabled="!session.connected"
            @click="emit('insert', command)"
          >
            {{ tr('Ins Terminal einfügen') }}</button
          ><button
            v-if="session"
            class="button primary small"
            :disabled="!session.connected"
            @click="emit('run', command)"
          >
            <Icon name="Play" :size="14" />{{ tr('Ausführen …') }}</button
          ><button class="button secondary small" @click="batch = { ...command }">
            <Icon name="Files" :size="14" />{{ tr('Mehrere Verbindungen …') }}</button
          ><button class="icon-button" :title="tr('Bearbeiten')" @click="edit(command)">
            <Icon name="Pencil" :size="15" /></button
          ><button
            class="icon-button"
            :title="tr('Kopie in einem anderen Tresor anlegen')"
            @click="edit(command, true)"
          >
            <Icon name="Files" :size="15" /></button
          ><button
            class="icon-button"
            :title="tr('Befehl löschen')"
            @click="
              deleting = { command, revision: store.view.localRevision };
              error = '';
            "
          >
            <Icon name="Trash2" :size="15" />
          </button>
        </div>
      </article>
      <Modal
        v-if="editing"
        :title="existing ? tr('Befehl bearbeiten') : tr('Neuer Befehl')"
        @close="
          editing = false;
          draft.command = '';
        "
        ><form class="form-body" @submit.prevent="save">
          <p v-if="error" class="error-message" role="alert">{{ errorText(error) }}</p>
          <label
            >{{ tr('Tresor')
            }}<SelectMenu
              v-model="draft.workspace"
              :options="options"
              :disabled="existing"
              :label="tr('Befehl speichern in')"
          /></label>
          <p class="muted">
            {{
              draft.workspace === 'private'
                ? tr('Dieser Befehl bleibt in deinem verschlüsselten privaten Tresor.')
                : tr(
                    'Der vollständige Befehl wird mit genau diesem Team verschlüsselt synchronisiert.',
                  )
            }}
          </p>
          <label
            >{{ tr('Name')
            }}<input v-model.trim="draft.name" required maxlength="120" autofocus /></label
          ><label
            >{{ tr('Beschreibung') }}<input v-model="draft.description" maxlength="4096" /></label
          ><label
            >{{ tr('Befehl')
            }}<textarea
              v-model="draft.command"
              class="mono"
              required
              rows="7"
              maxlength="65536"
              spellcheck="false"
            />
          </label>
          <div class="modal-actions">
            <button
              type="button"
              class="button secondary"
              @click="
                editing = false;
                draft.command = '';
              "
            >
              {{ tr('Abbrechen') }}</button
            ><button class="button primary" :disabled="busy">{{ tr('Speichern') }}</button>
          </div>
        </form></Modal
      >
      <Modal
        v-if="deleting"
        :title="tr('Befehl löschen?')"
        :subtitle="deleting.command.name"
        @close="deleting = undefined"
        ><div class="form-body">
          <p v-if="error" class="error-message" role="alert">{{ errorText(error) }}</p>
          <p>
            {{
              deleting.command.workspace === 'private'
                ? tr('Der gespeicherte Befehl wird entfernt.')
                : tr('Die Löschung wird mit diesem Team synchronisiert.')
            }}
          </p>
          <div class="modal-actions">
            <button class="button secondary" @click="deleting = undefined">
              {{ tr('Abbrechen') }}</button
            ><button class="button primary" :disabled="busy" @click="remove">
              {{ tr('Löschen') }}
            </button>
          </div>
        </div></Modal
      >
      <CommandBatch v-if="batch" :command="batch" @close="batch = undefined" />
    </template>
  </section>
</template>

<style>
.library-switch {
  display: inline-flex;
  padding: 4px;
  gap: 4px;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  margin-bottom: 26px;
}
.library-switch button {
  border: 0;
  border-radius: 7px;
  background: none;
  color: var(--muted);
  padding: 10px 18px;
  cursor: pointer;
}
.library-switch button.active {
  color: var(--text);
  background: var(--surface-raised);
  box-shadow: 0 1px 4px #0002;
}
</style>
