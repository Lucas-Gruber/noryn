<script setup lang="ts">
import { ref } from 'vue'
import type { ClientSession } from '../types'
import TerminalPane from './TerminalPane.vue'
import FilePane from './FilePane.vue'
import Icon from './Icon.vue'
defineProps<{ session: ClientSession; visible: boolean }>()
const files = ref(true)
</script>
<template><div class="session-pane"><div class="session-toolbar"><span class="session-target"><Icon name="Terminal" :size="16" /><strong>{{ session.username }}<span>@</span>{{ session.host }}</strong><span class="status-pill" :class="{ amber: !session.connected }"><span class="status-dot"></span>{{ session.preview ? 'Vorschau' : session.connected ? 'Verbunden' : 'Beendet' }}</span></span><button class="button secondary small" :class="{ selected: files }" @click="files = !files"><Icon name="Files" :size="14" />Dateibrowser</button></div><div class="session-split"><div class="terminal-wrap"><div class="terminal-caption"><span>TERMINAL</span><span>SSH <Icon name="LockKeyhole" :size="11" /></span></div><TerminalPane :session="session" :visible="visible" /></div><FilePane v-if="files" :session="session" /></div><div class="session-bottom"><span><Icon name="ShieldCheck" :size="12" />{{ session.preview ? 'Browser-Vorschau mit Beispieldaten' : 'SSH-Hostschlüssel geprüft · verschlüsselter Transport' }}</span><span>{{ session.name }} · Port 22 / konfigurierter SSH-Port</span></div></div></template>
