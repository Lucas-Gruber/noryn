<script setup lang="ts">
import { t as tr } from '../i18n';

import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, SavedCommand } from '../types';
import { transportLabel } from '../protocols';
import TerminalPane from './TerminalPane.vue';
import FileWorkspace from './FileWorkspace.vue';
import Icon from './Icon.vue';
import SshTools from './SshTools.vue';
import ResizeHandle from './ResizeHandle.vue';
import { layout } from '../composables/useLayout';
import { action, reconnectSession, store } from '../composables/useNoryn';
import CommandsPage from './CommandsPage.vue';
import Modal from './Modal.vue';
import LoginHealth from './LoginHealth.vue';
const terminal = ref<InstanceType<typeof TerminalPane>>();
const commandToRun = ref<SavedCommand>();
const commandBusy = ref(false),
  commandError = ref('');
const props = defineProps<{ session: ClientSession; visible: boolean }>();
async function insert(command: SavedCommand) {
  view.value = 'terminal';
  filesFullscreen.value = false;
  await nextTick();
  terminal.value?.insert(command.command);
}
async function runCommand() {
  if (!commandToRun.value || !props.session.connected || commandBusy.value) return;
  commandBusy.value = true;
  commandError.value = '';
  try {
    if (!props.session.preview)
      await invoke('terminal_input', {
        id: props.session.id,
        data: `${commandToRun.value.command}\n`,
      });
    else
      props.session.sink?.(
        new TextEncoder().encode(tr('\r\nBrowser-Vorschau: Befehl wird nicht ausgeführt.\r\n')),
      );
    commandToRun.value = undefined;
    view.value = 'terminal';
    filesFullscreen.value = false;
  } catch (e) {
    commandError.value = String(e);
  } finally {
    commandBusy.value = false;
  }
}
const split = ref<HTMLElement>();
const width = ref(900);
let observer: ResizeObserver;
const filesLocal = ref(false);
const splitMax = computed(() => Math.max(240, width.value - (filesLocal.value ? 468 : 308)));
const terminalWidth = computed(() =>
  Math.max(240, Math.min(splitMax.value, (width.value - 8) * layout.terminalRatio)),
);
onMounted(() => {
  observer = new ResizeObserver((entries) => {
    const w = entries[0]?.contentRect.width;
    if (w) width.value = w;
  });
  if (split.value) observer.observe(split.value);
});
onBeforeUnmount(() => observer?.disconnect());
const view = ref<'terminal' | 'forwards' | 'system' | 'commands' | 'health'>('terminal');
defineEmits<{ save: [] }>();
const files = ref(true);
const filesFullscreen = ref(false);
function toggleFiles() {
  files.value = !files.value;
  if (!files.value) filesFullscreen.value = false;
}
</script>
<template>
  <div class="session-pane">
    <div v-if="!session.connected" class="session-ended" role="status">
      <div>
        <strong>{{ tr('Sitzung beendet') }}</strong
        ><span>{{ session.endReason || tr('Die Verbindung ist geschlossen.') }}</span
        ><small>{{ tr('Keine automatische Wiederverbindung.') }}</small>
      </div>
      <button
        v-if="!session.temporary"
        class="button secondary small"
        :disabled="store.loading"
        @click="action(() => reconnectSession(session))"
      >
        <Icon name="RefreshCw" :size="14" />{{ tr('Neu verbinden') }}
      </button>
    </div>
    <div v-if="session.temporary" class="temporary-session">
      <span><Icon name="Clock" :size="14" />{{ tr('Temporäre Verbindung') }}</span>
      <button class="button secondary small" @click="$emit('save')">
        <Icon name="Plus" :size="14" />{{ tr('Im privaten Tresor speichern') }}
      </button>
    </div>
    <div v-if="session.protocol === 'ssh'" class="session-toolbar">
      <span class="session-target"
        ><Icon name="Terminal" :size="16" /><strong
          >{{ session.username }}<span>@</span>{{ session.host }}</strong
        ><span class="status-pill" :class="{ amber: !session.connected }"
          ><span class="status-dot"></span
          >{{
            session.preview ? tr('Vorschau') : session.connected ? tr('Verbunden') : tr('Beendet')
          }}</span
        ></span
      >
      <div class="session-tools-tabs">
        <button
          class="button secondary small"
          :class="{ selected: view === 'health' }"
          @click="view = 'health'"
        >
          <Icon
            :name="session.healthLoading ? 'LoaderCircle' : 'Activity'"
            :size="14"
            :class="{ spin: session.healthLoading }"
          />{{ tr('Login-Check') }}
        </button>
        <button
          class="button secondary small"
          :class="{ selected: view === 'commands' }"
          @click="view = 'commands'"
        >
          <Icon name="Keyboard" :size="14" />{{ tr('Befehle') }}
        </button>
        <button
          class="button secondary small"
          :class="{ selected: view === 'terminal' }"
          @click="view = 'terminal'"
        >
          <Icon name="Terminal" :size="14" />{{ tr('Terminal') }}</button
        ><button
          class="button secondary small"
          :class="{ selected: view === 'forwards' }"
          @click="view = 'forwards'"
        >
          <Icon name="Link" :size="14" />{{ tr('Weiterleitungen') }}</button
        ><button
          class="button secondary small"
          :class="{ selected: view === 'system' }"
          @click="view = 'system'"
        >
          <Icon name="Activity" :size="14" />{{ tr('System') }}</button
        ><button
          v-if="view === 'terminal'"
          class="button secondary small"
          :class="{ selected: files }"
          @click="toggleFiles"
        >
          <Icon name="Files" :size="14" />{{ tr('Dateibrowser') }}
        </button>
      </div>
    </div>
    <div
      v-show="view === 'terminal'"
      ref="split"
      class="session-split"
      :style="{ '--terminal-width': terminalWidth + 'px' }"
      :class="{
        'has-files': files,
        'files-fullscreen': filesFullscreen || session.protocol !== 'ssh',
      }"
    >
      <div v-if="session.protocol === 'ssh'" v-show="!filesFullscreen" class="terminal-wrap">
        <div class="terminal-caption">
          <span>{{ tr('TERMINAL') }}</span
          ><span>{{ tr('SSH') }} <Icon name="LockKeyhole" :size="11" /></span>
        </div>
        <TerminalPane
          ref="terminal"
          :session="session"
          :visible="visible && view === 'terminal' && !filesFullscreen"
        />
      </div>
      <ResizeHandle
        v-if="session.protocol === 'ssh' && files && !filesFullscreen"
        :model-value="terminalWidth"
        :min="240"
        :max="splitMax"
        :reset="(width - 8) / 2"
        :label="tr('Aufteilung von Terminal und Dateien')"
        @update:model-value="layout.terminalRatio = $event / (width - 8)"
      />
      <FileWorkspace
        v-show="files || session.protocol !== 'ssh'"
        :session="session"
        :visible="visible && view === 'terminal' && (files || session.protocol !== 'ssh')"
        :fullscreen="filesFullscreen || session.protocol !== 'ssh'"
        @local-view="filesLocal = $event"
        @fullscreen="filesFullscreen = !filesFullscreen"
      />
    </div>
    <LoginHealth v-if="session.protocol === 'ssh'" v-show="view === 'health'" :session="session" />
    <SshTools
      v-if="session.protocol === 'ssh'"
      v-show="view === 'forwards' || view === 'system'"
      :session="session"
      :visible="visible && (view === 'forwards' || view === 'system')"
      :mode="view === 'forwards' ? 'forwards' : 'system'"
    />
    <CommandsPage
      v-if="session.protocol === 'ssh'"
      v-show="view === 'commands'"
      :session="session"
      @insert="insert"
      @run="
        commandToRun = $event;
        commandError = '';
      "
    />
    <Modal
      v-if="commandToRun"
      :title="tr('Befehl ausführen?')"
      :subtitle="`${session.username}@${session.host}:${session.port}`"
      wide
      @close="commandToRun = undefined"
      ><div class="form-body">
        <p v-if="commandError" class="error-message" role="alert">{{ commandError }}</p>
        <p>
          „{{ commandToRun.name
          }}{{
            tr(
              '“ wird in dieser SSH-Sitzung mit ihren aktuellen Rechten und ihrem aktuellen Arbeitsverzeichnis ausgeführt.',
            )
          }}
        </p>
        <pre class="command-preview">{{ commandToRun.command }}</pre>
        <div class="modal-actions">
          <button class="button secondary" @click="commandToRun = undefined">
            {{ tr('Abbrechen') }}</button
          ><button
            class="button primary"
            :disabled="commandBusy || !session.connected"
            @click="runCommand"
          >
            {{ tr('Auf diesem Server ausführen') }}
          </button>
        </div>
      </div></Modal
    >
    <div class="session-bottom">
      <span
        ><Icon :name="session.protocol === 'ftp' ? 'ShieldAlert' : 'ShieldCheck'" :size="12" />{{
          session.preview
            ? tr('Browser-Vorschau mit Beispieldaten')
            : transportLabel(session.protocol)
        }}</span
      ><span>{{ session.name }} {{ tr('· Port') }} {{ session.port }}</span>
    </div>
  </div>
</template>
