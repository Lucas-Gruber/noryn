<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import type { ClientSession } from '../types';
import { fail } from '../composables/useHarbor';
const props = defineProps<{ session: ClientSession; visible: boolean }>();
const host = ref<HTMLElement>();
let term: Terminal | undefined;
let fit: FitAddon | undefined;
let observer: ResizeObserver | undefined;
let frame = 0;
function resize() {
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(() => {
    if (host.value?.clientWidth && host.value?.clientHeight) fit?.fit();
  });
}
onMounted(() => {
  term = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    fontFamily: "'JetBrains Mono', 'Cascadia Code', 'DejaVu Sans Mono', monospace",
    lineHeight: 1.45,
    scrollback: 5000,
    allowProposedApi: false,
    theme: {
      background: '#111719',
      foreground: '#d3ded9',
      cursor: '#89d7b3',
      selectionBackground: '#35594d',
      black: '#1c2629',
      red: '#e89595',
      green: '#94d7af',
      yellow: '#e0c58c',
      blue: '#94b9e8',
      magenta: '#b6a3dd',
      cyan: '#8cccd4',
      white: '#d3ded9',
      brightBlack: '#6d8081',
    },
  });
  fit = new FitAddon();
  term.loadAddon(fit);
  term.open(host.value!);
  resize();
  props.session.sink = (data) => term?.write(data);
  for (const bytes of props.session.buffer.splice(0)) term.write(bytes);
  if (props.session.preview) {
    term.writeln('\x1b[90mHarbor · Terminal-Vorschau · Keine echte SSH-Verbindung\x1b[0m\r\n');
    term.writeln(
      `\x1b[32m${props.session.username}@${props.session.host}\x1b[0m  \x1b[90m~\x1b[0m`,
    );
    term.write('\x1b[32m❯\x1b[0m ');
  }
  term.onData((data) => {
    if (!props.session.connected) return;
    if (props.session.preview) {
      if (data === '\r') {
        term?.write(
          '\r\n\x1b[90mEchte Befehle sind in der Tauri-Desktop-App verfügbar.\x1b[0m\r\n\x1b[32m❯\x1b[0m ',
        );
        return;
      }
      if (data === '\x7f') {
        term?.write('\b \b');
        return;
      }
      term?.write(data);
      return;
    }
    void invoke('terminal_input', { id: props.session.id, data }).catch(fail);
  });
  term.onResize(({ cols, rows }) => {
    if (!props.session.preview && props.session.connected)
      void invoke('terminal_resize', { id: props.session.id, cols, rows }).catch(fail);
  });
  observer = new ResizeObserver(resize);
  observer.observe(host.value!);
  term.focus();
});
watch(
  () => props.visible,
  async (visible) => {
    if (visible) {
      await nextTick();
      resize();
    }
  },
);
onBeforeUnmount(() => {
  delete props.session.sink;
  observer?.disconnect();
  cancelAnimationFrame(frame);
  term?.dispose();
});
</script>
<template><div ref="host" class="terminal-host" aria-label="SSH-Terminal"></div></template>
