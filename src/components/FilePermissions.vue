<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { errorText, t as tr } from '../i18n';
import { desktop, notify } from '../composables/useNoryn';
import { formatMode, parseMode, parseId, symbolicMode } from '../permissions';
import type { ClientSession, RemoteEntry } from '../types';
import Modal from './Modal.vue';

interface Snapshot {
  path: string;
  isDir: boolean;
  mode: number | null;
  uid: number | null;
  gid: number | null;
  canChmod: boolean;
  canChown: boolean;
  fingerprint: string;
}
const props = defineProps<{ session: ClientSession; side: 'local' | 'remote' }>();
const emit = defineEmits<{ done: [] }>();
const visible = ref(false),
  busy = ref(false),
  error = ref(''),
  path = ref('');
const snapshot = ref<Snapshot>();
interface TreePreview {
  fingerprint: string;
  files: number;
  directories: number;
  skippedLinks: number;
  skippedSpecial: number;
}
const recursive = ref(false),
  applyMode = ref(true),
  applyUid = ref(false),
  applyGid = ref(false);
const treePreview = ref<TreePreview>();
const failedApply = ref(false);
const modeText = ref(''),
  uidText = ref(''),
  gidText = ref('');
const mode = computed(() => parseMode(modeText.value));
const rows = computed(() => [tr('Eigentümer'), tr('Gruppe'), tr('Andere')]);
const columns = computed(() => [tr('Lesen'), tr('Schreiben'), tr('Ausführen')]);
const special = computed(() => [
  { bit: 0o4000, label: tr('Setuid') },
  { bit: 0o2000, label: tr('Setgid') },
  { bit: 0o1000, label: tr('Sticky Bit') },
]);
const change = computed(() => {
  const old = snapshot.value;
  if (!old) return;
  const withMode = old.canChmod && (!recursive.value || applyMode.value);
  const withUid = old.canChown && (!recursive.value || applyUid.value);
  const withGid = old.canChown && (!recursive.value || applyGid.value);
  if (withMode && (recursive.value || modeText.value !== '') && mode.value === undefined) return;
  if (
    (withUid &&
      (recursive.value || uidText.value !== '') &&
      parseId(uidText.value) === undefined) ||
    (withGid && (recursive.value || gidText.value !== '') && parseId(gidText.value) === undefined)
  )
    return;
  const result = {
    mode:
      withMode && mode.value !== undefined && (recursive.value || mode.value !== old.mode)
        ? mode.value
        : null,
    uid:
      withUid && uidText.value !== '' && (recursive.value || parseId(uidText.value) !== old.uid)
        ? parseId(uidText.value)!
        : null,
    gid:
      withGid && gidText.value !== '' && (recursive.value || parseId(gidText.value) !== old.gid)
        ? parseId(gidText.value)!
        : null,
  };
  if (result.mode !== null || result.uid !== null || result.gid !== null) return result;
});
watch(
  [recursive, modeText, uidText, gidText, applyMode, applyUid, applyGid],
  () => {
    treePreview.value = undefined;
  },
  { flush: 'sync' },
);
watch(
  recursive,
  (enabled) => {
    if (!enabled) return;
    applyMode.value = mode.value !== undefined && !!snapshot.value?.canChmod;
    applyUid.value =
      !!snapshot.value?.canChown &&
      uidText.value !== '' &&
      parseId(uidText.value) !== snapshot.value.uid;
    applyGid.value =
      !!snapshot.value?.canChown &&
      gidText.value !== '' &&
      parseId(gidText.value) !== snapshot.value.gid;
  },
  { flush: 'sync' },
);
let generation = 0;
let job: string | undefined;
function toggle(bit: number, checked: boolean) {
  if (mode.value === undefined || busy.value || !snapshot.value?.canChmod) return;
  modeText.value = formatMode(checked ? mode.value | bit : mode.value & ~bit);
  if (recursive.value) applyMode.value = true;
}
function accept(value: Snapshot) {
  if (!value.isDir) recursive.value = false;
  snapshot.value = value;
  modeText.value = value.mode === null ? '' : formatMode(value.mode);
  uidText.value = value.uid?.toString() ?? '';
  gidText.value = value.gid?.toString() ?? '';
}
async function open(entry: RemoteEntry) {
  if (busy.value) return;
  recursive.value = false;
  treePreview.value = undefined;
  path.value = entry.path;
  snapshot.value = undefined;
  error.value = '';
  visible.value = true;
  if (!desktop) {
    accept({
      path: entry.path,
      isDir: entry.isDir,
      mode: parseMode(entry.permissions) ?? null,
      uid: 1000,
      gid: 1000,
      canChmod: true,
      canChown: props.side === 'local' || !['ftp', 'ftps'].includes(props.session.protocol),
      fingerprint: 'demo',
    });
    return;
  }
  await load();
}
async function request(operation: 'load' | 'preview' | 'apply') {
  const applying = operation === 'apply';
  if (busy.value || (operation !== 'load' && (!snapshot.value || !change.value || !desktop)))
    return;
  const treeJob = recursive.value && operation !== 'load';
  if (treeJob && applying && !treePreview.value) return;
  const ticket = ++generation;
  busy.value = true;
  error.value = '';
  failedApply.value = false;
  job = crypto.randomUUID();
  const args = treeJob
    ? {
        id: props.session.id,
        jobId: job,
        request: {
          path: path.value,
          local: props.side === 'local',
          change: change.value,
          rootFingerprint: snapshot.value!.fingerprint,
        },
        expected: applying ? treePreview.value!.fingerprint : null,
      }
    : {
        id: props.session.id,
        jobId: job,
        path: path.value,
        local: props.side === 'local',
        change: applying ? change.value : null,
        expected: applying ? snapshot.value!.fingerprint : null,
      };
  if (operation === 'load') {
    snapshot.value = undefined;
    treePreview.value = undefined;
  }
  try {
    const result = await invoke<Snapshot | TreePreview>(
      treeJob ? 'file_permissions_tree' : 'file_permissions',
      args,
    );
    if (ticket !== generation) return;
    if (treeJob) treePreview.value = result as TreePreview;
    else accept(result as Snapshot);
    if (applying) {
      visible.value = false;
      notify(tr('Berechtigungen aktualisiert'));
      emit('done');
    }
  } catch (cause) {
    if (ticket !== generation) return;
    error.value = String(cause);
    failedApply.value = applying;
    snapshot.value = undefined;
    treePreview.value = undefined;
    if (applying) emit('done');
  } finally {
    if (ticket === generation) {
      busy.value = false;
      job = undefined;
    }
  }
}
async function load() {
  await request('load');
}
async function save() {
  await request(recursive.value && !treePreview.value ? 'preview' : 'apply');
}
function cancel() {
  if (job && desktop) void invoke('remote_transfer_cancel', { transferId: job }).catch(() => {});
}
function close() {
  if (!busy.value) {
    generation++;
    visible.value = false;
    snapshot.value = undefined;
  }
}
onBeforeUnmount(() => {
  generation++;
  cancel();
});
defineExpose({ open });
</script>

<template>
  <Modal
    v-if="visible"
    class="permissions-dialog"
    :title="tr('Berechtigungen')"
    :subtitle="side === 'local' ? tr('Lokales Dateisystem') : tr('Entferntes Dateisystem')"
    :busy="busy"
    @close="close"
  >
    <form @submit.prevent="save">
      <div class="form-body permissions-body">
        <code class="permissions-path">{{ path }}</code>
        <p v-if="!recursive" class="muted">
          {{
            tr('Gilt nur für diesen Eintrag. Unterordner und Dateien darin werden nicht verändert.')
          }}
        </p>
        <template v-if="snapshot">
          <label
            v-if="snapshot.isDir && (snapshot.canChmod || snapshot.canChown)"
            class="checkbox-row"
          >
            <input v-model="recursive" type="checkbox" :disabled="busy" />{{
              tr('Rekursiv auf Unterordner und Dateien anwenden')
            }}
          </label>
          <div v-if="recursive" class="local-field">
            <p class="muted">
              {{
                tr(
                  'Welche Werte sollen für alle Einträge übernommen werden? Verknüpfungen und Spezialdateien werden übersprungen.',
                )
              }}
            </p>
            <label class="checkbox-row"
              ><input
                v-model="applyMode"
                type="checkbox"
                :disabled="busy || !snapshot.canChmod"
              />{{ tr('Dateirechte (chmod)') }}</label
            >
            <label class="checkbox-row"
              ><input v-model="applyUid" type="checkbox" :disabled="busy || !snapshot.canChown" />{{
                tr('Eigentümer (UID)')
              }}</label
            >
            <label class="checkbox-row"
              ><input v-model="applyGid" type="checkbox" :disabled="busy || !snapshot.canChown" />{{
                tr('Gruppe (GID)')
              }}</label
            >
          </div>
          <fieldset :disabled="busy || !snapshot.canChmod">
            <legend>{{ tr('Dateirechte (chmod)') }}</legend>
            <div class="mode-input">
              <label
                >{{ tr('Oktal (z. B. 755 oder 4755)')
                }}<input
                  v-model="modeText"
                  @input="recursive && (applyMode = true)"
                  inputmode="numeric"
                  maxlength="4"
                  placeholder="755"
                  spellcheck="false"
                  :aria-invalid="modeText !== '' && mode === undefined"
              /></label>
              <code>{{ mode === undefined ? '—' : symbolicMode(mode) }}</code>
            </div>
            <p v-if="snapshot.mode === null" class="muted">
              {{
                tr(
                  'Der Server liefert keine Unix-Rechte. Bei Bedarf einen vollständigen oktalen Wert eingeben.',
                )
              }}
            </p>
            <p v-if="modeText !== '' && mode === undefined" class="inline-error">
              {{ tr('Bitte 3 oder 4 Ziffern von 0 bis 7 eingeben.') }}
            </p>
            <table class="permission-grid">
              <thead>
                <tr>
                  <th scope="col">{{ tr('Zugriff') }}</th>
                  <th v-for="column in columns" :key="column" scope="col">{{ column }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(row, rowIndex) in rows" :key="row">
                  <th scope="row">{{ row }}</th>
                  <td v-for="(column, columnIndex) in columns" :key="column">
                    <input
                      type="checkbox"
                      :aria-label="`${row}: ${column}`"
                      :disabled="mode === undefined"
                      :checked="
                        mode !== undefined && !!(mode & (1 << (8 - rowIndex * 3 - columnIndex)))
                      "
                      @change="
                        toggle(
                          1 << (8 - rowIndex * 3 - columnIndex),
                          ($event.target as HTMLInputElement).checked,
                        )
                      "
                    />
                  </td>
                </tr>
              </tbody>
            </table>
            <div class="special-bits">
              <label v-for="item in special" :key="item.bit" class="checkbox-row"
                ><input
                  type="checkbox"
                  :disabled="mode === undefined"
                  :checked="mode !== undefined && !!(mode & item.bit)"
                  @change="toggle(item.bit, ($event.target as HTMLInputElement).checked)"
                />{{ item.label }}</label
              >
            </div>
          </fieldset>
          <fieldset :disabled="busy || !snapshot.canChown">
            <legend>{{ tr('Eigentümer und Gruppe (chown)') }}</legend>
            <div class="form-row">
              <label
                >{{ tr('Eigentümer (UID)')
                }}<input
                  v-model="uidText"
                  @input="recursive && (applyUid = true)"
                  inputmode="numeric"
                  :placeholder="tr('Unverändert')"
                  :aria-invalid="uidText !== '' && parseId(uidText) === undefined" /></label
              ><label
                >{{ tr('Gruppe (GID)')
                }}<input
                  v-model="gidText"
                  @input="recursive && (applyGid = true)"
                  inputmode="numeric"
                  :placeholder="tr('Unverändert')"
                  :aria-invalid="gidText !== '' && parseId(gidText) === undefined"
              /></label>
            </div>
            <p
              v-if="
                snapshot.canChown &&
                ((uidText !== '' && parseId(uidText) === undefined) ||
                  (gidText !== '' && parseId(gidText) === undefined))
              "
              class="inline-error"
            >
              {{ tr('Bitte gültige UID/GID von 0 bis 4294967294 eingeben.') }}
            </p>
            <p class="muted">
              {{
                tr(
                  'Numerische IDs verwenden. Leere Felder bleiben unverändert. chown kann Setuid/Setgid entfernen.',
                )
              }}
            </p>
          </fieldset>
          <p v-if="!snapshot.canChown && side === 'remote'" class="muted">
            {{
              tr(
                'FTP/FTPS unterstützt hier nur chmod über SITE CHMOD, sofern der Server es erlaubt. Ein standardisiertes chown gibt es nicht.',
              )
            }}
          </p>
          <p v-if="!snapshot.canChmod && !snapshot.canChown" class="muted">
            {{
              tr(
                'Dieses Dateisystem stellt keine bearbeitbaren Unix-Rechte bereit. Windows-ACLs werden hier nicht verändert.',
              )
            }}
          </p>
          <p v-if="side === 'remote' && snapshot.canChown" class="muted">
            {{
              tr(
                'Die Rechte des aktuellen Kontos gelten. Bei Bedarf zuerst den sudo-Modus im Dateibrowser aktivieren.',
              )
            }}
          </p>
          <p v-if="!desktop" class="muted">
            {{
              tr('Browser-Vorschau: Berechtigungen können nur in der Desktop-App geändert werden.')
            }}
          </p>
        </template>
        <p v-if="busy && !snapshot" class="muted" role="status">
          {{ tr('Metadaten werden geladen …') }}
        </p>
        <div v-if="treePreview" class="local-field" role="status">
          <p>
            {{
              tr(
                '{files} Dateien und {directories} Ordner, einschließlich des ausgewählten Ordners.',
                { files: treePreview.files, directories: treePreview.directories },
              )
            }}
          </p>
          <p class="muted">
            {{
              tr('{links} Verknüpfungen und {special} Spezialdateien werden übersprungen.', {
                links: treePreview.skippedLinks,
                special: treePreview.skippedSpecial,
              })
            }}
          </p>
          <p class="muted">
            {{
              tr(
                'Die ausgewählten Werte werden überall gleich gesetzt. Bei Abbruch bleiben bereits ausgeführte Änderungen erhalten.',
              )
            }}
          </p>
        </div>
        <p v-if="error" class="inline-error" role="alert">
          {{ errorText(error)
          }}<template v-if="failedApply"
            ><br />{{
              tr(
                'Teiländerungen können bereits wirksam sein. Vor einem weiteren Versuch neu laden und prüfen.',
              )
            }}</template
          >
        </p>
      </div>
      <div class="modal-actions">
        <button v-if="busy" type="button" class="button secondary" @click="cancel">
          {{ tr('Vorgang abbrechen') }}
        </button>
        <button v-else type="button" class="button secondary" @click="close">
          {{ tr('Schließen') }}
        </button>
        <button type="button" class="button secondary" :disabled="busy || !desktop" @click="load">
          {{ tr('Neu laden') }}
        </button>
        <button class="button primary" :disabled="busy || !change || !desktop">
          {{
            busy
              ? tr('Wird verarbeitet …')
              : recursive
                ? treePreview
                  ? tr('Rekursiv anwenden')
                  : tr('Vorschau prüfen')
                : tr('Übernehmen')
          }}
        </button>
      </div>
    </form>
  </Modal>
</template>

<style scoped>
.permissions-dialog {
  width: 620px;
}
.modal-actions {
  position: sticky;
  bottom: 0;
  z-index: 3;
  flex-wrap: wrap;
}
.permissions-body {
  gap: 1.1rem;
}
.permissions-path {
  overflow-wrap: anywhere;
  padding: 0.7rem 0.9rem;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
}
fieldset {
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  padding: 1rem;
  margin: 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.9rem;
}
legend {
  color: var(--text);
  padding: 0 0.4rem;
  font-weight: 600;
}
fieldset:disabled {
  opacity: 0.6;
}
.mode-input {
  display: flex;
  align-items: end;
  gap: 1.5rem;
}
.mode-input label {
  flex: 1;
}
.mode-input code {
  padding-bottom: 0.8rem;
  font-size: 1rem;
  white-space: nowrap;
}
.permission-grid {
  width: 100%;
  border-collapse: collapse;
}
.permission-grid th {
  font-weight: 500;
  color: var(--text-secondary);
}
.permission-grid :is(td, th) {
  padding: 0.65rem 0.25rem;
  text-align: center;
  border-bottom: 1px solid var(--border);
}
.permission-grid th:first-child {
  text-align: left;
}
.permission-grid input {
  width: 1.1rem;
  height: 1.1rem;
  cursor: pointer;
  accent-color: var(--accent);
}
.special-bits {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
}
.muted {
  font-size: 0.928571rem;
  line-height: 1.6;
}
.inline-error {
  white-space: pre-wrap;
}
</style>
