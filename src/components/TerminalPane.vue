<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { clipboardKey, copyText, pasteText } from '../clipboard';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import type { ClientSession } from '../types';
import { showMenu } from '../composables/useContextMenu';
import { fail } from '../composables/useNoryn';
import { appearance, baseTextPixels } from '../composables/useAppearance';
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
let disposed = false;
function insert(command: string) {
  if (!props.session.connected || /[\r\n\x00-\x08\x0b-\x1f\x7f]/.test(command)) return;
  term?.paste(command);
  term?.focus();
}
defineExpose({ insert });
function terminalAppearance() {
  const style = getComputedStyle(host.value!);
  const color = (name: string) => style.getPropertyValue(name).trim();
  return {
    fontSize: baseTextPixels(),
    fontFamily: color('--font-mono'),
    theme: {
      background: color('--terminal-background'),
      foreground: color('--terminal-foreground'),
      cursor: color('--accent'),
      cursorAccent: color('--terminal-background'),
      selectionBackground: color('--terminal-selection'),
      selectionInactiveBackground: color('--terminal-selection-inactive'),
      black: color('--terminal-ansi-black'),
      red: color('--terminal-ansi-red'),
      green: color('--terminal-ansi-green'),
      yellow: color('--terminal-ansi-yellow'),
      blue: color('--terminal-ansi-blue'),
      magenta: color('--terminal-ansi-magenta'),
      cyan: color('--terminal-ansi-cyan'),
      white: color('--terminal-ansi-white'),
      brightBlack: color('--terminal-ansi-bright-black'),
      brightRed: color('--terminal-ansi-bright-red'),
      brightGreen: color('--terminal-ansi-bright-green'),
      brightYellow: color('--terminal-ansi-bright-yellow'),
      brightBlue: color('--terminal-ansi-bright-blue'),
      brightMagenta: color('--terminal-ansi-bright-magenta'),
      brightCyan: color('--terminal-ansi-bright-cyan'),
      brightWhite: color('--terminal-ansi-bright-white'),
    },
  };
}
onMounted(async () => {
  try {
    await document.fonts.load(`${baseTextPixels()}px "JetBrains Mono Variable"`);
  } catch {
    // The bundled fallback stack keeps the terminal usable if a font cannot load.
  }
  if (disposed || !host.value) return;
  term = new Terminal({
    ...terminalAppearance(),
    cursorBlink: true,
    lineHeight: 1.45,
    scrollback: 5000,
    allowProposedApi: false,
  });
  fit = new FitAddon();
  term.loadAddon(fit);
  term.open(host.value!);
  term.attachCustomKeyEventHandler((event) => clipboardKey(event, copy, paste, fail));
  resize();
  props.session.sink = (data) => term?.write(data);
  for (const bytes of props.session.buffer.splice(0)) term.write(bytes);
  if (props.session.preview) {
    term.writeln('\x1b[90mNoryn · Terminal-Vorschau · Keine echte SSH-Verbindung\x1b[0m\r\n');
    term.writeln(
      `\x1b[92m${props.session.username}\x1b[90m@\x1b[94m${props.session.host}\x1b[0m  \x1b[96m~\x1b[0m`,
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
watch([() => appearance.theme, () => appearance.textSize], async () => {
  await nextTick();
  if (!term || disposed || !host.value) return;
  const options = terminalAppearance();
  term.options.fontFamily = options.fontFamily;
  term.options.fontSize = options.fontSize;
  term.options.theme = options.theme;
  resize();
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
  disposed = true;
  delete props.session.sink;
  observer?.disconnect();
  cancelAnimationFrame(frame);
  term?.dispose();
});
async function copy() {
  const text = term?.getSelection();
  if (text) await copyText(text);
}
async function paste() {
  if (disposed || !props.session.connected) return;
  const text = await pasteText();
  if (disposed || !props.session.connected) return;
  term?.paste(text);
  term?.focus();
}
function context(event: MouseEvent) {
  showMenu(event, [
    {
      label: 'Auswahl kopieren',
      icon: 'Copy',
      disabled: !term?.hasSelection(),
      action: copy,
      shortcut: 'Ctrl Shift C',
    },
    {
      label: 'Einfügen',
      icon: 'Keyboard',
      disabled: !props.session.connected,
      action: paste,
      shortcut: 'Ctrl Shift V',
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
