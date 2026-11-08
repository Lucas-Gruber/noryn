<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { Channel, invoke } from '@tauri-apps/api/core';
import { decodeRdpFrame, scanCode } from '../rdpInput';
import { paintRdpFrame } from '../rdpDisplay';
import { errorText, t as tr } from '../i18n';
import { notify } from '../composables/useNoryn';
import type { ClientSession } from '../types';
import Icon from './Icon.vue';
const props = defineProps<{ session: ClientSession; visible: boolean }>();
const canvas = ref<HTMLCanvasElement>();
const viewport = ref<HTMLElement>();
const fit = ref(true),
  ready = ref(false),
  error = ref('');
let disposed = false,
  movement = 0,
  pending: { x: number; y: number } | undefined;
async function send(input: Record<string, unknown>) {
  if (disposed || !props.session.connected || props.session.preview) return;
  try {
    await invoke('rdp_input', { id: props.session.id, input });
  } catch (cause) {
    if (!disposed && props.session.connected) error.value = String(cause);
  }
}
function key(event: KeyboardEvent, down: boolean) {
  if (event.isComposing) return;
  event.preventDefault();
  event.stopPropagation();
  const code = scanCode(event.code);
  if (code) void send({ type: 'key', code, down });
  else if (down && event.key.length === 1) void send({ type: 'text', text: event.key });
}
function position(event: PointerEvent) {
  if (!canvas.value) return;
  const bounds = canvas.value.getBoundingClientRect();
  const x = Math.max(
    0,
    Math.min(
      canvas.value.width - 1,
      Math.floor(((event.clientX - bounds.left) * canvas.value.width) / bounds.width),
    ),
  );
  const y = Math.max(
    0,
    Math.min(
      canvas.value.height - 1,
      Math.floor(((event.clientY - bounds.top) * canvas.value.height) / bounds.height),
    ),
  );
  return { x, y };
}
function move(event: PointerEvent) {
  pending = position(event);
  if (!movement)
    movement = requestAnimationFrame(() => {
      movement = 0;
      if (pending) void send({ type: 'move', ...pending });
      pending = undefined;
    });
}
function button(event: PointerEvent, down: boolean) {
  cancelAnimationFrame(movement);
  movement = 0;
  pending = undefined;
  if (down) {
    canvas.value?.focus();
    canvas.value?.setPointerCapture(event.pointerId);
  }
  const pos = position(event);
  if (pos) void send({ type: 'button', button: event.button, down, ...pos });
}
function wheel(event: WheelEvent) {
  event.preventDefault();
  const horizontal = Math.abs(event.deltaX) > Math.abs(event.deltaY);
  const delta = horizontal ? event.deltaX : event.deltaY;
  if (!delta) return;
  void send({
    type: 'wheel',
    delta: delta > 0 ? -120 : 120,
    horizontal,
  });
}
async function fullscreen() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await viewport.value?.requestFullscreen();
  } catch {
    notify(tr('Vollbild ist in dieser Umgebung nicht verfügbar.'));
  }
}
function release() {
  cancelAnimationFrame(movement);
  movement = 0;
  pending = undefined;
  void send({ type: 'release' });
}
function visibility() {
  const visible = props.visible && !document.hidden;
  if (!visible) release();
  void send({ type: 'visible', visible });
}
watch(() => props.visible, visibility);
onMounted(async () => {
  window.addEventListener('blur', release);
  document.addEventListener('visibilitychange', visibility);
  if (props.session.preview) {
    ready.value = true;
    return;
  }
  const output = new Channel<ArrayBuffer>();
  let context: CanvasRenderingContext2D | null = null;
  output.onmessage = (buffer) => {
    if (disposed) return;
    try {
      const frame = decodeRdpFrame(buffer),
        target = canvas.value;
      if (target) {
        context ||= target.getContext('2d', { alpha: false });
        if (context) paintRdpFrame(target, context, frame);
        ready.value = true;
      }
      void send({ type: 'ack', sequence: frame.sequence });
    } catch (cause) {
      error.value = String(cause);
    }
  };
  try {
    await nextTick();
    await invoke('rdp_subscribe', { id: props.session.id, output });
    visibility();
  } catch (cause) {
    if (!disposed) error.value = String(cause);
  }
});
onBeforeUnmount(() => {
  window.removeEventListener('blur', release);
  document.removeEventListener('visibilitychange', visibility);
  release();
  disposed = true;
  cancelAnimationFrame(movement);
});
</script>
<template>
  <section class="rdp-pane" ref="viewport">
    <div class="rdp-toolbar">
      <span
        ><Icon name="Monitor" :size="16" /><strong>{{ session.username }}@{{ session.host }}</strong
        ><span class="small-badge">{{ tr('RDP · TLS/NLA') }}</span></span
      >
      <div>
        <button
          class="button secondary small"
          @click="send({ type: 'secureAttention' })"
          :disabled="!session.connected || session.preview"
        >
          {{ tr('Strg + Alt + Entf') }}
        </button>
        <button class="button secondary small" :aria-pressed="fit" @click="fit = !fit">
          <Icon name="Scan" :size="14" />{{ fit ? tr('Originalgröße') : tr('An Fenster anpassen') }}
        </button>
        <button class="icon-button" :aria-label="tr('Vollbild')" @click="fullscreen">
          <Icon name="Maximize2" :size="16" />
        </button>
      </div>
    </div>
    <p v-if="error" class="rdp-error" role="alert">{{ errorText(error) }}</p>
    <div class="rdp-screen" :class="{ fit: fit }" v-if="!session.preview">
      <canvas
        ref="canvas"
        width="1600"
        height="900"
        tabindex="0"
        :aria-label="tr('Remote-Desktop · zum Steuern anklicken')"
        @keydown="key($event, true)"
        @keyup="key($event, false)"
        @blur="release"
        @compositionend="send({ type: 'text', text: $event.data })"
        @pointermove="move"
        @pointerdown.prevent="button($event, true)"
        @pointerup.prevent="button($event, false)"
        @pointercancel="release"
        @wheel="wheel"
        @contextmenu.prevent
      />
      <span v-if="!ready" class="rdp-wait"
        ><Icon name="LoaderCircle" class="spin" :size="20" />{{
          tr('Warte auf Remote-Bildschirm …')
        }}</span
      >
    </div>
    <div v-else class="rdp-preview">
      <Icon name="Monitor" :size="48" />
      <h2>{{ tr('Remote-Desktop') }}</h2>
      <p>{{ tr('Die Browser-Vorschau verbindet sich nicht mit einem RDP-Server.') }}</p>
    </div>
    <p class="rdp-hint">
      {{
        tr(
          'Zum Steuern in den Bildschirm klicken. Beim Verlassen werden gedrückte Tasten freigegeben.',
        )
      }}
    </p>
  </section>
</template>
<style scoped>
.rdp-pane {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  background: var(--bg);
}
.rdp-toolbar,
.rdp-toolbar > span,
.rdp-toolbar > div {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
}
.rdp-toolbar {
  justify-content: space-between;
  padding: 0.85rem 1rem;
  border-bottom: 1px solid var(--border);
  background: var(--panel);
}
.rdp-toolbar strong {
  font-size: 1rem;
  font-weight: 550;
  overflow-wrap: anywhere;
}
.rdp-screen {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: auto;
  background: #0c0c0c;
  display: flex;
  align-items: safe center;
  justify-content: safe center;
}
canvas {
  display: block;
  flex-shrink: 0;
  outline: none;
  touch-action: none;
  max-width: none;
  cursor: default;
}
.fit canvas {
  max-width: 100%;
  max-height: 100%;
  width: auto;
  height: auto;
  object-fit: contain;
}
canvas:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.rdp-wait {
  position: absolute;
  inset: 0;
  display: flex;
  gap: 0.75rem;
  align-items: center;
  justify-content: center;
  color: #eee;
  background: #111;
}
.rdp-hint {
  margin: 0;
  padding: 0.6rem 1rem;
  color: var(--muted);
  font-size: 0.9rem;
  background: var(--panel);
}
.rdp-error {
  padding: 0.75rem 1rem;
  color: var(--danger);
  overflow-wrap: anywhere;
}
.rdp-preview {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1rem;
  color: var(--muted);
}
.rdp-pane:fullscreen {
  height: 100vh;
  width: 100vw;
}
</style>
