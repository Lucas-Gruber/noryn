<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import type { ClientSession } from '../types';
import { showMenu } from '../composables/useContextMenu';
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
  const style = getComputedStyle(host.value!);
  const color = (name: string) => style.getPropertyValue(name).trim();
  term = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    fontFamily: "'JetBrains Mono', 'Cascadia Code', 'DejaVu Sans Mono', monospace",
    lineHeight: 1.45,
    scrollback: 5000,
    allowProposedApi: false,
    theme: {
      background: color('--terminal-background'),
      foreground: color('--terminal-foreground'),
      cursor: color('--accent'),
      cursorAccent: color('--terminal-background'),
      selectionBackground: color('--terminal-selection'),
      selectionInactiveBackground: color('--terminal-selection-inactive'),
      black: '#23262d',
      red: '#d98992',
      green: '#a3b89b',
      yellow: '#cfbc91',
      blue: '#a7b7d5',
      magenta: '#b9a6cc',
      cyan: '#96bfc5',
      white: color('--terminal-foreground'),
      brightBlack: color('--muted'),
      brightRed: '#e8a1a9',
      brightGreen: '#b9cdae',
      brightYellow: '#e0d0aa',
      brightBlue: '#c3cde2',
      brightMagenta: '#d0bbdf',
      brightCyan: '#b2d7dc',
      brightWhite: '#f4f5f7',
    },
  });
  fit = new FitAddon();
  term.loadAddon(fit);
  term.open(host.value!);
  resize();
  props.session.sink = (data) => term?.write(data);
  for (const bytes of props.session.buffer.splice(0)) term.write(bytes);
  if (props.session.preview) {
    term.writeln('\x1b[90mNoryn · Terminal-Vorschau · Keine echte SSH-Verbindung\x1b[0m\r\n');
    term.writeln(
      `\x1b[94m${props.session.username}@${props.session.host}\x1b[0m  \x1b[90m~\x1b[0m`,
    );
    term.write('\x1b[94m❯\x1b[0m ');
  }
  term.onData((data) => {
    if (!props.session.connected) return;
    if (props.session.preview) {
      if (data === '\r') {
        term?.write(
          '\r\n\x1b[90mEchte Befehle sind in der Tauri-Desktop-App verfügbar.\x1b[0m\r\n\x1b[94m❯\x1b[0m ',
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
function context(event: MouseEvent) {
  showMenu(event, [
    {
      label: 'Auswahl kopieren',
      icon: 'Copy',
      disabled: !term?.hasSelection(),
      action: () => navigator.clipboard.writeText(term?.getSelection() || ''),
    },
    {
      label: 'Einfügen',
      icon: 'Keyboard',
      disabled: !props.session.connected,
      action: async () => {
        const text = await navigator.clipboard.readText();
        term?.paste(text);
        term?.focus();
      },
    },
    {
      label: 'Terminal leeren',
      icon: 'Trash2',
      separator: true,
      action: () => {
        term?.clear();
        term?.focus();
      },
    },
  ]);
}
</script>
<template>
  <div ref="host" class="terminal-host" aria-label="SSH-Terminal" @contextmenu="context"></div>
</template>
