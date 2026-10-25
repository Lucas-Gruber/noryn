<script setup lang="ts">
import { errorText, formatLocale, t as tr } from '../i18n';

import { computed, onBeforeUnmount, ref } from 'vue';
import { Channel, invoke } from '@tauri-apps/api/core';
import type { ClientSession, NodeEntry, SavedCommand } from '../types';
import {
  closeSession,
  connectForCommand,
  credentialFor,
  desktop,
  sessionForCommand,
  store,
} from '../composables/useNoryn';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
const props = defineProps<{
  command: SavedCommand;
  script?: { teamId: string; entryId: string };
}>();
const emit = defineEmits<{ close: [] }>();
interface Output {
  stdout: string;
  stderr: string;
  exitCode: number | null;
  error: string | null;
  truncated: boolean;
}
interface Reply {
  id: string;
  output: Output;
}
interface Target {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  context: string;
  connected: boolean;
  node?: NodeEntry;
  session?: ClientSession;
}
function context(node: NodeEntry) {
  const path: string[] = [];
  const seen = new Set<string>();
  let parent = node.parentId;
  while (parent && !seen.has(parent)) {
    seen.add(parent);
    const entry = store.view.nodes.find((n) => n.id === parent);
    if (!entry) break;
    path.unshift(entry.name);
    parent = entry.parentId;
  }
  const workspace =
    node.workspace === 'private'
      ? tr('Privat')
      : store.view.teams.find((t) => t.id === node.workspace)?.name || tr('Team');
  return [workspace, ...path].join(' / ');
}
const targets = computed<Target[]>(() => [
  ...store.view.nodes
    .filter((n) => n.kind === 'connection' && n.protocol === 'ssh')
    .map((node) => ({
      id: node.id,
      name: node.name,
      host: node.host,
      port: node.port,
      username: credentialFor(node.id)?.credential?.username || tr('Kein Konto zugeordnet'),
      context: context(node),
      connected: !!sessionForCommand(node),
      node,
    })),
  ...store.sessions
    .filter((s) => s.temporary && s.connected && s.protocol === 'ssh')
    .map((session) => ({
      id: `session:${session.id}`,
      name: session.name,
      host: session.host,
      port: session.port,
      username: session.username,
      get context() {
        return tr('Schnelle Verbindung');
      },
      connected: true,
      session,
    })),
]);
const query = ref('');
const visibleTargets = computed(() =>
  targets.value.filter((t) =>
    `${t.name} ${t.host} ${t.username} ${t.context}`
      .toLocaleLowerCase(formatLocale())
      .includes(query.value.toLocaleLowerCase(formatLocale())),
  ),
);
const selected = ref<string[]>([]);
const results = ref<Reply[]>([]);
const activeTarget = ref('');
const activeReply = computed(() => results.value.find((r) => r.id === activeTarget.value));
const labels = ref<Record<string, string>>({});
function selectResult(id: string) {
  if (selected.value.includes(id)) activeTarget.value = id;
}
function resultStatus(id: string) {
  const reply = results.value.find((r) => r.id === id);
  if (!reply) return running.value ? tr('Läuft / wartet') : tr('Ohne Ergebnis');
  if (reply.output.error) return tr('Fehler');
  return reply.output.exitCode === 0
    ? tr('Erfolgreich')
    : tr('Exit {p0}', { p0: reply.output.exitCode });
}
function resultKeys(event: KeyboardEvent, id: string) {
  const index = selected.value.indexOf(id);
  let next = index;
  if (event.key === 'ArrowRight') next = (index + 1) % selected.value.length;
  else if (event.key === 'ArrowLeft')
    next = (index + selected.value.length - 1) % selected.value.length;
  else if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = selected.value.length - 1;
  else return;
  event.preventDefault();
  selectResult(selected.value[next] || '');
  (event.currentTarget as HTMLElement).parentElement
    ?.querySelectorAll<HTMLButtonElement>('[role=tab]')
    [next]?.focus();
}
const phases = ref<Record<string, string>>({});
const running = ref(false),
  started = ref(false),
  error = ref(''),
  cancelling = ref(false),
  closeNew = ref(true);
const names = ref<Record<string, string>>({});
let runId = '',
  disposed = false,
  controller: AbortController | undefined;
const command = { ...props.command };
async function run() {
  if (running.value || started.value || !selected.value.length) return;
  const chosen = selected.value.map((id) => targets.value.find((t) => t.id === id));
  if (chosen.some((t) => !t)) {
    error.value = tr('Verbindungen wurden geändert. Bitte erneut auswählen.');
    return;
  }
  names.value = Object.fromEntries(
    chosen.map((t) => [t!.id, `${t!.context} · ${t!.name} · ${t!.username}@${t!.host}:${t!.port}`]),
  );
  labels.value = Object.fromEntries(chosen.map((t) => [t!.id, t!.name]));
  activeTarget.value = selected.value[0] || '';
  running.value = true;
  started.value = true;
  error.value = '';
  runId = crypto.randomUUID();
  controller = new AbortController();
  const signal = controller.signal;
  const owned: string[] = [],
    mapping = new Map<string, string>();
  const receive = (reply: Reply) => {
    if (!disposed) results.value = [...results.value.filter((r) => r.id !== reply.id), reply];
  };
  const failure = (id: string, reason: unknown) =>
    receive({
      id,
      output: { stdout: '', stderr: '', exitCode: null, error: String(reason), truncated: false },
    });
  try {
    // Authentication dialogs share one surface. Resolve host keys/OTP in order;
    // execution is parallel after all chosen logins have completed.
    for (const target of chosen as Target[]) {
      if (signal.aborted) {
        failure(target.id, tr('Ausführung abgebrochen.'));
        continue;
      }
      phases.value[target.id] = tr('Verbindung wird geöffnet …');
      try {
        const reused = target.session || (target.node && sessionForCommand(target.node));
        const session = target.session || (await connectForCommand(target.node!, signal));
        if (!reused) owned.push(session.id);
        mapping.set(session.id, target.id);
        phases.value[target.id] = tr('Verbunden · wartet auf Ausführung …');
      } catch (reason) {
        failure(target.id, reason);
      }
    }
    if (signal.aborted || !mapping.size) return;
    if (desktop) {
      const output = new Channel<Reply>();
      const received = (reply: Reply) => {
        const id = mapping.get(reply.id);
        if (id) receive({ ...reply, id });
      };
      output.onmessage = received;
      const replies = await invoke<Reply[]>('command_batch_run', {
        script: props.script || null,
        runId,
        ids: [...mapping.keys()],
        commandId: command.id,
        expectedCommand: command.command,
        output,
      });
      for (const reply of replies) received(reply);
    } else {
      for (const id of mapping.values())
        receive({
          id,
          output: {
            get stdout() {
              return tr('Browser-Vorschau: Es wurde kein Befehl ausgeführt.\n');
            },
            stderr: '',
            exitCode: 0,
            error: null,
            truncated: false,
          },
        });
    }
  } catch (reason) {
    if (!disposed) error.value = String(reason);
  } finally {
    for (const id of selected.value)
      if (!results.value.some((r) => r.id === id))
        failure(
          id,
          signal.aborted
            ? tr('Ausführung abgebrochen.')
            : tr(
                'Keine Ausführung bestätigt. Bei einem Übertragungsfehler können entfernte Prozesse weiterlaufen.',
              ),
        );
    if (closeNew.value) {
      const closed = await Promise.allSettled(owned.map((id) => closeSession(id)));
      if (!disposed && closed.some((r) => r.status === 'rejected'))
        error.value = tr(
          'Einige automatisch geöffneten Sitzungen konnten nicht geschlossen werden. Bitte die Sitzungstabs prüfen.',
        );
    }
    running.value = false;
  }
}
async function cancel() {
  cancelling.value = true;
  controller?.abort();
  if (desktop && runId) {
    try {
      await invoke('command_batch_cancel', { runId });
    } catch (reason) {
      if (!disposed) error.value = String(reason);
    }
  }
}
onBeforeUnmount(() => {
  disposed = true;
  if (running.value) void cancel();
});
</script>
<template>
  <Modal
    :title="started ? tr('Ergebnisse pro Verbindung') : tr('Auf mehreren Verbindungen ausführen')"
    :subtitle="command.name"
    wide
    :busy="running"
    @close="emit('close')"
  >
    <div class="form-body command-batch">
      <p v-if="!started" class="muted">
        {{
          tr(
            'Wähle deine Ziele. Noryn öffnet fehlende SSH-Sitzungen mit deinen persönlichen Zugangsdaten; Hostschlüssel und OTP werden bei Bedarf abgefragt.',
          )
        }}
      </p>
      <details v-if="!started" class="muted batch-details">
        <summary>{{ tr('Wie wird der Befehl ausgeführt?') }}</summary>
        <p>
          {{
            tr(
              'Bis zu vier Ziele parallel, 120 Sekunden je Ziel. Eine separate Shell mit dem angemeldeten Benutzer, ohne Terminal-Eingabe oder sudo-Abfrage. Verzeichnis und Rechte des Terminals werden nicht übernommen.',
            )
          }}
        </p>
      </details>
      <p v-if="script && !started" class="tools-notice">
        {{ tr('Bash-Ausführung · Der angezeigte Inhalt wird über die bestehende SSH-Sitzung an') }}
        <code>bash -s --</code> {{ tr('gestreamt. Auf dem Ziel wird keine Skriptdatei angelegt.') }}
      </p>
      <pre v-if="!started" class="batch-command">{{ command.command }}</pre>
      <template v-if="!started">
        <p v-if="!targets.length" class="tools-notice">
          {{ tr('Lege zuerst SSH-Verbindungen im Verbindungsbaum an.') }}
        </p>
        <input
          v-model="query"
          :aria-label="tr('Verbindungen suchen')"
          :placeholder="tr('Verbindungen suchen …')"
        />
        <div class="batch-target-list">
          <label v-for="target in visibleTargets" :key="target.id" class="batch-target">
            <input
              v-model="selected"
              type="checkbox"
              :value="target.id"
              :disabled="selected.length >= 16 && !selected.includes(target.id)"
            />
            <span
              ><strong>{{ target.name }}</strong
              ><small>{{ target.username }}@{{ target.host }}:{{ target.port }}</small
              ><small
                >{{ target.context }} ·
                {{
                  target.connected ? tr('Bereits verbunden') : tr('Wird automatisch geöffnet')
                }}</small
              ></span
            >
          </label>
        </div>
        <label class="batch-target"
          ><input v-model="closeNew" type="checkbox" />{{
            tr('Neu geöffnete Sitzungen danach schließen')
          }}</label
        >
        <p class="muted">
          {{ selected.length }}
          {{
            tr(
              'von maximal 16 Zielen ausgewählt. Der oben sichtbare Befehl wird auf jedem ausgewählten Ziel ausgeführt.',
            )
          }}
        </p>
      </template>
      <template v-else>
        <p aria-live="polite">
          {{ results.length }} / {{ selected.length }} {{ tr('abgeschlossen')
          }}{{ running ? tr(' · Ausführung läuft …') : '' }}
        </p>
        <div
          class="batch-result-tabs"
          role="tablist"
          :aria-label="tr('Ergebnisse nach Verbindung')"
        >
          <button
            v-for="id in selected"
            :id="`batch-tab-${id}`"
            :key="id"
            role="tab"
            :aria-selected="activeTarget === id"
            :aria-controls="`batch-panel-${id}`"
            :tabindex="activeTarget === id ? 0 : -1"
            :title="names[id]"
            @click="selectResult(id)"
            @keydown="resultKeys($event, id)"
          >
            <span>{{ labels[id] }}</span
            ><small
              :class="{
                'inline-error':
                  resultStatus(id) === 'Fehler' || resultStatus(id).startsWith('Exit'),
              }"
              >{{ resultStatus(id) }}</small
            >
          </button>
        </div>
        <article
          v-if="activeTarget"
          :id="`batch-panel-${activeTarget}`"
          class="batch-result"
          role="tabpanel"
          :aria-labelledby="`batch-tab-${activeTarget}`"
          tabindex="0"
        >
          <h3>{{ names[activeTarget] }}</h3>
          <template v-if="activeReply">
            <p
              :class="
                activeReply.output.error || activeReply.output.exitCode !== 0
                  ? 'inline-error'
                  : 'muted'
              "
            >
              {{
                activeReply.output.error ||
                tr('Exit-Code {p0}', { p0: activeReply.output.exitCode })
              }}
            </p>
            <template v-if="activeReply.output.stdout"
              ><span class="output-label">{{ tr('Standardausgabe') }}</span>
              <pre>{{ activeReply.output.stdout }}</pre>
            </template>
            <template v-if="activeReply.output.stderr"
              ><span class="output-label">{{ tr('Fehlerausgabe') }}</span>
              <pre class="batch-stderr">{{ activeReply.output.stderr }}</pre>
            </template>
            <p v-if="activeReply.output.truncated" class="muted">
              {{ tr('Ausgabe auf 256 KiB je Stream gekürzt.') }}
            </p>
            <p v-if="!activeReply.output.stdout && !activeReply.output.stderr" class="muted">
              {{ tr('Keine Ausgabe.') }}
            </p>
          </template>
          <p v-else class="muted">
            {{
              running
                ? phases[activeTarget] || tr('Wartet auf Anmeldung …')
                : tr('Kein Ergebnis verfügbar.')
            }}
          </p>
        </article>
        <details class="batch-details muted">
          <summary>
            {{ script ? tr('Ausgeführtes Skript anzeigen') : tr('Ausgeführten Befehl anzeigen') }}
          </summary>
          <pre class="batch-command">{{ command.command }}</pre>
        </details>
        <p class="muted">
          {{
            tr(
              'Abbruch beendet den SSH-Kanal. Entfernte Prozesse können weiterlaufen. Ergebnisse bleiben nur bis zum Schließen dieser Ansicht erhalten.',
            )
          }}
        </p>
      </template>
      <p v-if="error" class="inline-error" role="alert">{{ errorText(error) }}</p>
      <div class="modal-actions">
        <button v-if="running" class="button danger" :disabled="cancelling" @click="cancel">
          <Icon name="Square" :size="14" />{{
            cancelling ? tr('Wird abgebrochen …') : tr('Abbrechen')
          }}
        </button>
        <button v-else class="button secondary" @click="emit('close')">
          {{ tr('Schließen') }}
        </button>
        <button v-if="!started" class="button primary" :disabled="!selected.length" @click="run">
          <Icon name="Play" :size="14" />{{ tr('Auf') }} {{ selected.length }}
          {{ tr('Zielen ausführen') }}
        </button>
      </div>
    </div>
  </Modal>
</template>
<style scoped>
.command-batch {
  max-height: calc(100vh - 170px);
  gap: 0.8rem;
  overflow: auto;
}
.command-batch > * {
  flex-shrink: 0;
}
.command-batch > .batch-target-list {
  flex-shrink: 1;
}
.batch-target-list {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  min-height: 7rem;
  max-height: 30vh;
  overflow: auto;
}
.batch-details {
  font-size: 0.9rem;
}
.batch-details summary {
  cursor: pointer;
}
.batch-details p {
  margin-top: 0.6rem;
  line-height: 1.6;
}
.batch-command,
.batch-result pre {
  padding: 0.9rem;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 0.5rem;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font: 0.95rem/1.6 var(--font-mono);
  max-height: 24rem;
  overflow: auto;
  user-select: text;
}
.batch-target {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 0.8rem;
  padding: 0.8rem;
  border: 1px solid var(--border);
  border-radius: 0.5rem;
  cursor: pointer;
}
.batch-target input {
  width: auto;
}
.batch-target span {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.batch-target small {
  color: var(--muted);
}
.batch-result-tabs {
  display: flex;
  gap: 0.4rem;
  overflow-x: auto;
  border-bottom: 1px solid var(--border);
  padding-bottom: 0.6rem;
}
.batch-result-tabs button {
  display: flex;
  flex-direction: column;
  flex: 0 0 auto;
  gap: 0.3rem;
  padding: 0.65rem 0.9rem;
  border: 1px solid var(--border);
  border-radius: 0.5rem;
  background: var(--bg);
  color: var(--muted);
  cursor: pointer;
  text-align: left;
}
.batch-result-tabs button[aria-selected='true'] {
  color: var(--text);
  background: var(--accent-soft);
  border-color: var(--accent);
}
.batch-result-tabs small {
  font-size: 0.8rem;
}
.output-label {
  display: block;
  margin: 0.9rem 0 0.4rem;
  font-size: 0.9rem;
  color: var(--muted);
}
.batch-result {
  border-top: 1px solid var(--border);
  padding-top: 0.8rem;
}
.batch-result h3 {
  font-size: 1rem;
  overflow-wrap: anywhere;
}
.batch-stderr {
  color: var(--danger);
}
</style>
