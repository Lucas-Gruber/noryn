<script setup lang="ts">
import { messageText, errorText, t as tr, formatLocale } from '../i18n';

import { computed, ref, watch, defineAsyncComponent } from 'vue';
import type { SavedCommand, ScriptEntry, ScriptInfo } from '../types';
import { desktop, store } from '../composables/useNoryn';
import {
  deleteScript,
  readScript,
  saveScript,
  scriptStates,
  syncScripts,
} from '../composables/useScripts';
const BashEditor = defineAsyncComponent(() => import('./BashEditor.vue'));
import ScriptEditors from './ScriptEditors.vue';
import SelectMenu from './SelectMenu.vue';
import CommandBatch from './CommandBatch.vue';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
const teamId = ref(
  store.workspace !== '*' && store.workspace !== 'private'
    ? store.workspace
    : store.view.teams[0]?.id || '',
);
const folder = ref<string | null>(null),
  query = ref(''),
  error = ref(''),
  busy = ref(false);
const editing = ref<ScriptEntry>(),
  expected = ref<string | null>(null),
  deleting = ref<ScriptInfo>();
const batch = ref<{ command: SavedCommand; script: { teamId: string; entryId: string } }>();
const resolution = ref<'adoptOrigin' | 'republishMissing' | 'keepBoth'>();
const team = computed(() => store.view.teams.find((t) => t.id === teamId.value));
const summary = computed(() => store.view.scriptLibraries?.[teamId.value]);
const status = computed(() => scriptStates[teamId.value]);
const entries = computed(() => summary.value?.entries || []);
const visible = computed(() =>
  entries.value
    .filter((e) =>
      query.value
        ? `${e.name} ${e.description}`
            .toLocaleLowerCase(formatLocale())
            .includes(query.value.toLocaleLowerCase(formatLocale()))
        : e.parentId === folder.value,
    )
    .sort(
      (a, b) =>
        Number(b.kind === 'folder') - Number(a.kind === 'folder') ||
        a.name.localeCompare(b.name, formatLocale()),
    ),
);
function path(id: string | null): string {
  const names: string[] = [],
    seen = new Set<string>();
  while (id && !seen.has(id)) {
    seen.add(id);
    const e = entries.value.find((e) => e.id === id);
    if (!e) break;
    names.unshift(e.name);
    id = e.parentId;
  }
  return names.join(' / ');
}
const crumbs = computed(() => {
  const result: ScriptInfo[] = [];
  let id = folder.value;
  const seen = new Set<string>();
  while (id && !seen.has(id)) {
    seen.add(id);
    const e = entries.value.find((e) => e.id === id);
    if (!e) break;
    result.unshift(e);
    id = e.parentId;
  }
  return result;
});
const parentOptions = computed(() => {
  const excluded = new Set(editing.value ? [editing.value.id] : []);
  for (let i = 0; i <= entries.value.length; i++) {
    const n = excluded.size;
    for (const e of entries.value) if (e.parentId && excluded.has(e.parentId)) excluded.add(e.id);
    if (n === excluded.size) break;
  }
  return [
    {
      value: null,
      get label() {
        return tr('Sammlung /');
      },
    },
    ...entries.value
      .filter((e) => e.kind === 'folder' && !excluded.has(e.id))
      .map((e) => ({ value: e.id, label: path(e.id) })),
  ];
});
watch(teamId, () => {
  folder.value = null;
  query.value = '';
  error.value = '';
});
watch(entries, () => {
  if (folder.value && !entries.value.some((e) => e.id === folder.value)) folder.value = null;
});
watch(
  () => store.view.teams.map((t) => t.id).join(','),
  () => {
    if (!team.value) teamId.value = store.view.teams[0]?.id || '';
  },
);
async function edit(info?: ScriptInfo, kind: ScriptEntry['kind'] = 'script') {
  error.value = '';
  try {
    const target = teamId.value;
    const entry = info
      ? await readScript(target, info)
      : {
          id: crypto.randomUUID(),
          parentId: folder.value,
          kind,
          name: '',
          description: '',
          body: kind === 'script' ? '#!/usr/bin/env bash\nset -euo pipefail\n\n' : '',
          updatedAt: 0,
          updatedBy: '',
        };
    if (target !== teamId.value || !store.unlocked) return;
    expected.value = info?.fingerprint || null;
    editing.value = entry;
  } catch (e) {
    error.value = String(e);
  }
}
async function save(copy = false) {
  if (!editing.value || busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    await saveScript(
      teamId.value,
      {
        ...editing.value,
        ...(copy
          ? { id: crypto.randomUUID(), name: tr('{p0} (Kopie)', { p0: editing.value.name }) }
          : {}),
      },
      copy ? null : expected.value,
    );
    editing.value = undefined;
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
    await deleteScript(teamId.value, deleting.value);
    deleting.value = undefined;
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function execute(info: ScriptInfo) {
  try {
    const entry = await readScript(teamId.value, info);
    batch.value = {
      command: {
        id: entry.id,
        workspace: teamId.value,
        name: entry.name,
        description: entry.description,
        command: entry.body,
      },
      script: { teamId: teamId.value, entryId: entry.id },
    };
  } catch (e) {
    error.value = String(e);
  }
}
async function synchronize() {
  await syncScripts(teamId.value, {
    manual: true,
    ...(resolution.value ? { [resolution.value]: true } : {}),
  });
  resolution.value = undefined;
}
const editorPanel = ref<InstanceType<typeof ScriptEditors>>();
const actions: Record<string, string> = {
  get created() {
    return tr('Erstellt');
  },
  get updated() {
    return tr('Bearbeitet');
  },
  get deleted() {
    return tr('Entfernt');
  },
  get imported() {
    return tr('Importiert');
  },
};
const fields: Record<string, string> = {
  get body() {
    return tr('Inhalt');
  },
  get name() {
    return tr('Name');
  },
  get description() {
    return tr('Beschreibung');
  },
  get parent() {
    return tr('Ordner');
  },
  get kind() {
    return tr('Typ');
  },
};
function changedFields(values: string[]) {
  return values.map((f) => fields[f] || f).join(', ');
}
</script>
<template>
  <section class="script-library">
    <div class="tools-heading">
      <div>
        <span class="eyebrow">{{ tr('TEAM-BIBLIOTHEK') }}</span>
        <h2>{{ tr('Skripte, die Arbeit abnehmen.') }}</h2>
        <p>
          {{
            tr(
              'Bash-Skripte mit deinem Team organisieren und Ende-zu-Ende verschlüsselt synchronisieren.',
            )
          }}
        </p>
      </div>
      <button class="button secondary" :disabled="!team || status?.busy" @click="synchronize">
        <Icon name="RefreshCw" :size="16" />{{
          status?.busy ? tr('Synchronisiert …') : tr('Synchronisieren')
        }}
      </button>
    </div>
    <div v-if="!team" class="tools-empty">
      <Icon name="Users" :size="30" />
      <h3>{{ tr('Eine Sammlung für dein Team.') }}</h3>
      <p>
        {{
          tr(
            'Richte zuerst ein Team unter Team-Sync ein. Der private Tresor hat keine Skript-Sammlung.',
          )
        }}
      </p>
      <button class="button secondary" @click="store.page = 'teams'">
        {{ tr('Team-Sync öffnen') }}
      </button>
    </div>
    <template v-else>
      <div class="script-toolbar">
        <SelectMenu
          v-model="teamId"
          :options="store.view.teams.map((t) => ({ value: t.id, label: t.name }))"
          :label="tr('Skript-Team')"
        /><input
          v-model="query"
          :placeholder="tr('Skripte und Ordner suchen …')"
          :aria-label="tr('Skripte suchen')"
        /><button
          class="button secondary"
          :disabled="!team.memberName.trim()"
          @click="edit(undefined, 'folder')"
        >
          <Icon name="FolderPlus" :size="16" />{{ tr('Ordner') }}</button
        ><button class="button primary" :disabled="!team.memberName.trim()" @click="edit()">
          <Icon name="Plus" :size="16" />{{ tr('Neues Skript') }}
        </button>
      </div>
      <p v-if="!team.memberName.trim()" class="tools-notice">
        {{ tr('Trage unter Team-Sync deinen Namen ein, bevor du die Sammlung bearbeitest.') }}
      </p>
      <div class="script-sync-line">
        <span class="small-badge">{{
          summary?.dirty ? tr('Lokale Änderungen') : tr('Synchronisiert')
        }}</span
        ><span class="muted">{{
          messageText(status?.message || '') ||
          tr('Abgleich beim Entsperren und nach dem Speichern. Kein permanentes Polling.')
        }}</span>
      </div>
      <p v-if="summary?.suspended" class="tools-notice">
        {{
          tr(
            'Importierte Sammlung: Änderungen bleiben lokal, bis du ausdrücklich auf „Synchronisieren“ klickst.',
          )
        }}
      </p>
      <div v-if="summary?.originChanged" class="tools-notice">
        {{ tr('Das Sync-Ziel oder der Team-Schlüssel wurde geändert.') }}
        <button class="button secondary small" @click="resolution = 'adoptOrigin'">
          {{ tr('Neues Ziel bestätigen …') }}
        </button>
      </div>
      <div v-if="status?.error" class="error-message" role="alert">
        {{ errorText(status.error) }}
        <div class="command-actions">
          <button
            v-if="status.error.includes('Konflikt')"
            class="button secondary small"
            @click="resolution = 'keepBoth'"
          >
            {{ tr('Beide Stände behalten …') }}</button
          ><button
            v-if="status.error.includes('erneute Veröffentlichung')"
            class="button secondary small"
            @click="resolution = 'republishMissing'"
          >
            {{ tr('Sammlung erneut veröffentlichen …') }}
          </button>
        </div>
      </div>
      <p v-if="error && !editing && !deleting" class="error-message" role="alert">
        {{ errorText(error) }}
      </p>
      <ScriptEditors ref="editorPanel" />
      <nav class="script-crumbs" :aria-label="tr('Skriptordner')">
        <button
          @click="
            folder = null;
            query = '';
          "
        >
          <Icon name="Library" :size="15" />{{ tr('Sammlung') }}</button
        ><template v-for="crumb in crumbs" :key="crumb.id"
          ><Icon name="ChevronRight" :size="13" /><button
            @click="
              folder = crumb.id;
              query = '';
            "
          >
            {{ crumb.name }}
          </button></template
        >
      </nav>
      <div v-if="!visible.length" class="tools-empty">
        <Icon name="FileCode2" :size="30" />
        <h3>{{ query ? tr('Keine Treffer.') : tr('Platz für deinen nächsten Handgriff.') }}</h3>
        <p>{{ tr('Erstelle ein Bash-Skript oder einen Ordner.') }}</p>
      </div>
      <div v-else class="script-list">
        <article v-for="entry in visible" :key="entry.id" class="script-row">
          <button
            class="script-open"
            @click="entry.kind === 'folder' ? ((folder = entry.id), (query = '')) : edit(entry)"
          >
            <span class="script-symbol"
              ><Icon :name="entry.kind === 'folder' ? 'Folder' : 'FileCode2'" :size="22" /></span
            ><span
              ><strong>{{ entry.name }}</strong
              ><span v-if="entry.description" class="script-description">{{
                entry.description
              }}</span
              ><small
                >{{ query ? path(entry.parentId) + ' · ' : ''
                }}{{
                  entry.kind === 'folder'
                    ? tr('Ordner')
                    : `${Math.ceil(entry.bytes / 1024)} KiB · Bash`
                }}
                · {{ entry.updatedBy }} ·
                {{ new Date(entry.updatedAt).toLocaleDateString(formatLocale()) }}</small
              ></span
            >
          </button>
          <div class="command-actions">
            <template v-if="entry.kind === 'script'"
              ><button
                class="button secondary small"
                :disabled="!desktop"
                :title="tr('Mit dem Standardprogramm für Textdateien bearbeiten')"
                @click="editorPanel?.open(teamId, entry)"
              >
                <Icon name="ExternalLink" :size="14" />{{ tr('Extern') }}</button
              ><button class="button primary small" @click="execute(entry)">
                <Icon name="Play" :size="14" />{{ tr('Ausführen …') }}
              </button></template
            ><button
              class="icon-button"
              :title="tr('Bearbeiten / verschieben')"
              @click="edit(entry)"
            >
              <Icon name="Pencil" :size="15" /></button
            ><button
              class="icon-button"
              :title="tr('Löschen')"
              @click="
                deleting = entry;
                error = '';
              "
            >
              <Icon name="Trash2" :size="15" />
            </button>
          </div>
        </article>
      </div>
      <details v-if="summary?.history.length" class="script-history">
        <summary>{{ tr('Letzte Änderungen') }}</summary>
        <p v-for="change in summary.history.slice(-20).reverse()" :key="change.id">
          <time>{{ new Date(change.occurredAt).toLocaleString(formatLocale()) }}</time> ·
          {{ change.actor }} · {{ actions[change.action] }}: {{ change.target
          }}<small v-if="change.fields.length"> ({{ changedFields(change.fields) }})</small>
        </p>
      </details>
    </template>
    <Modal
      v-if="editing"
      :wide="editing.kind === 'script'"
      :title="
        editing.kind === 'folder' ? tr('Skriptordner bearbeiten') : tr('Bash-Skript bearbeiten')
      "
      :subtitle="team?.name"
      @close="editing = undefined"
      ><form
        class="form-body script-form"
        :class="{ 'script-code-form': editing.kind === 'script' }"
        @submit.prevent="save()"
      >
        <p v-if="error" class="error-message" role="alert">{{ errorText(error) }}</p>
        <div class="script-fields">
          <label
            >{{ tr('Name')
            }}<input v-model.trim="editing.name" required maxlength="120" autofocus /></label
          ><label
            >{{ tr('Ordner')
            }}<SelectMenu
              v-model="editing.parentId"
              :options="parentOptions"
              :label="tr('Skriptordner')"
          /></label>
        </div>
        <label
          >{{ tr('Beschreibung')
          }}<textarea v-model="editing.description" maxlength="4096" rows="2" /></label
        ><BashEditor v-if="editing.kind === 'script'" v-model="editing.body" />
        <p class="muted">
          {{
            tr(
              'Verschlüsselt auf diesem Gerät und auf dem Sync-Server. Bei der Ausführung wird der Inhalt über SSH an Bash gestreamt; es wird keine Skriptdatei auf dem Ziel angelegt.',
            )
          }}
        </p>
        <div class="modal-actions">
          <button type="button" class="button secondary" @click="editing = undefined">
            {{ tr('Abbrechen') }}</button
          ><button
            v-if="expected"
            type="button"
            class="button secondary"
            :disabled="busy"
            @click="save(true)"
          >
            {{ tr('Als Kopie speichern') }}</button
          ><button class="button primary" :disabled="busy">{{ tr('Speichern') }}</button>
        </div>
      </form></Modal
    >
    <Modal
      v-if="deleting"
      :title="tr('Eintrag löschen?')"
      :subtitle="deleting.name"
      @close="deleting = undefined"
      ><div class="form-body">
        <p v-if="error" class="error-message" role="alert">{{ errorText(error) }}</p>
        <p>
          {{
            deleting.kind === 'folder'
              ? tr(
                  'Der Ordner wird entfernt. Seine Skripte und Unterordner bleiben eine Ebene höher erhalten.',
                )
              : tr('Das Skript wird aus dieser Team-Sammlung entfernt.')
          }}
          {{ tr('Die Änderung wird synchronisiert.') }}
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
    <Modal v-if="resolution" :title="tr('Abgleich bestätigen')" @close="resolution = undefined"
      ><div class="form-body">
        <p>
          {{
            resolution === 'keepBoth'
              ? tr(
                  'Der aktuelle Serverstand bleibt erhalten. Deine gesamte lokale Sammlung wird zusätzlich unter „Lokaler Entwurf (Konfliktkopie)“ mit neuen IDs abgelegt. Anschließend kannst du die Varianten vergleichen und bereinigen.',
                )
              : resolution === 'adoptOrigin'
                ? tr(
                    'Die lokale Sammlung wird mit dem neu konfigurierten Server und Team-Schlüssel abgeglichen. Prüfe zuvor unter Team-Sync, ob das Ziel korrekt ist.',
                  )
                : tr(
                    'Auf dem Server fehlt eine zuvor bekannte Sammlung. Deine lokale Sammlung wird ausdrücklich erneut verschlüsselt hochgeladen.',
                  )
          }}
        </p>
        <div class="modal-actions">
          <button class="button secondary" @click="resolution = undefined">
            {{ tr('Abbrechen') }}</button
          ><button class="button primary" @click="synchronize">
            {{ tr('Bestätigen und synchronisieren') }}
          </button>
        </div>
      </div></Modal
    >
    <CommandBatch
      v-if="batch"
      :command="batch.command"
      :script="batch.script"
      @close="batch = undefined"
    />
  </section>
</template>
<style>
.modal[open]:has(.script-code-form) {
  width: 1100px;
  height: min(840px, calc(100vh - 50px));
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.modal:has(.script-code-form) .modal-head {
  flex-shrink: 0;
}
.script-code-form {
  flex: 1;
  min-height: 0;
  overflow: auto;
  gap: 12px;
  padding-top: 18px;
}
.script-code-form > * {
  flex-shrink: 0;
}
.script-code-form .bash-editor {
  flex: 1;
  min-height: 10rem;
}
.script-code-form .cm-editor {
  height: 100%;
}
.script-code-form .cm-scroller {
  height: 100%;
  min-height: 10rem;
}
.script-code-form .modal-actions {
  padding-top: 12px;
  padding-bottom: 12px;
}
.script-toolbar,
.script-sync-line,
.script-crumbs,
.script-fields {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.script-toolbar {
  margin: 24px 0 16px;
}
.script-toolbar input {
  flex: 1;
  min-width: 12rem;
}
.script-toolbar .select-menu {
  width: 14rem;
}
.script-sync-line {
  font-size: 0.9rem;
  margin: 12px 0;
}
.script-crumbs {
  gap: 6px;
  padding: 18px 0;
  border-bottom: 1px solid var(--border);
  color: var(--muted);
}
.script-crumbs button {
  display: flex;
  align-items: center;
  gap: 6px;
  background: none;
  color: var(--text);
  border: 0;
  cursor: pointer;
}
.script-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px 0;
  border-bottom: 1px solid var(--border);
  flex-wrap: wrap;
}
.script-open {
  display: flex;
  align-items: center;
  gap: 14px;
  flex: 1;
  min-width: 12rem;
  text-align: left;
  border: 0;
  background: none;
  color: var(--text);
  cursor: pointer;
}
.script-symbol {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border-radius: 10px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  flex-shrink: 0;
}
.script-open strong {
  font-size: 1.05rem;
}
.script-description,
.script-open small {
  display: block;
  margin-top: 5px;
  color: var(--muted);
}
.script-open small {
  font-size: 0.86rem;
}
.script-fields {
  align-items: start;
}
.script-fields label {
  flex: 1;
  min-width: 14rem;
}
.script-history {
  margin-top: 24px;
  color: var(--muted);
  font-size: 0.9rem;
}
.script-history summary {
  cursor: pointer;
  margin-bottom: 16px;
}
.script-history p {
  margin: 8px 0;
}
</style>
