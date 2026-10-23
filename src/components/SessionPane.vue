<script setup lang="ts">
import { ref } from 'vue';
import type { ClientSession } from '../types';
import { transportLabel } from '../protocols';
import TerminalPane from './TerminalPane.vue';
import FileWorkspace from './FileWorkspace.vue';
import Icon from './Icon.vue';
defineProps<{ session: ClientSession; visible: boolean }>();
const files = ref(true);
const filesFullscreen = ref(false);
function toggleFiles() {
  files.value = !files.value;
  if (!files.value) filesFullscreen.value = false;
}
</script>
<template>
  <div class="session-pane">
    <div v-if="session.protocol === 'ssh'" class="session-toolbar">
      <span class="session-target"
        ><Icon name="Terminal" :size="16" /><strong
          >{{ session.username }}<span>@</span>{{ session.host }}</strong
        ><span class="status-pill" :class="{ amber: !session.connected }"
          ><span class="status-dot"></span
          >{{ session.preview ? 'Vorschau' : session.connected ? 'Verbunden' : 'Beendet' }}</span
        ></span
      ><button class="button secondary small" :class="{ selected: files }" @click="toggleFiles">
        <Icon name="Files" :size="14" />Dateibrowser
      </button>
    </div>
    <div
      class="session-split"
      :class="{ 'files-fullscreen': filesFullscreen || session.protocol !== 'ssh' }"
    >
      <div v-if="session.protocol === 'ssh'" v-show="!filesFullscreen" class="terminal-wrap">
        <div class="terminal-caption">
          <span>TERMINAL</span><span>SSH <Icon name="LockKeyhole" :size="11" /></span>
        </div>
        <TerminalPane :session="session" :visible="visible && !filesFullscreen" />
      </div>
      <FileWorkspace
        v-show="files || session.protocol !== 'ssh'"
        :session="session"
        :visible="visible && (files || session.protocol !== 'ssh')"
        :fullscreen="filesFullscreen || session.protocol !== 'ssh'"
        @fullscreen="filesFullscreen = !filesFullscreen"
      />
    </div>
    <div class="session-bottom">
      <span
        ><Icon :name="session.protocol === 'ftp' ? 'ShieldAlert' : 'ShieldCheck'" :size="12" />{{
          session.preview ? 'Browser-Vorschau mit Beispieldaten' : transportLabel(session.protocol)
        }}</span
      ><span>{{ session.name }} · Port {{ session.port }}</span>
    </div>
  </div>
</template>
