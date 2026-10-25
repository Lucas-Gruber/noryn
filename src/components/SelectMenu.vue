<script setup lang="ts" generic="T extends string | null">
import { formatLocale, t as tr } from '../i18n';

import { computed, nextTick, onBeforeUnmount, ref, useId } from 'vue';
import Icon from './Icon.vue';
const props = defineProps<{
  modelValue: T;
  options: readonly { value: T; label: string; disabled?: boolean }[];
  label?: string;
  placeholder?: string;
  disabled?: boolean;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: T] }>();
const id = useId();
const trigger = ref<HTMLButtonElement>();
const menu = ref<HTMLElement>();
const target = ref<HTMLElement>();
const open = ref(false);
const active = ref(0);
const position = ref({ left: '0px', top: '0px', width: '0px', maxHeight: '280px' });
const selected = computed(() => props.options.find((o) => o.value === props.modelValue));
let typed = '';
let typedAt = 0;
function close(focus = false) {
  open.value = false;
  document.removeEventListener('pointerdown', outside, true);
  window.removeEventListener('resize', dismiss);
  window.removeEventListener('scroll', scrolled, true);
  if (focus) trigger.value?.focus();
}
function dismiss() {
  close();
}
function scrolled(e: Event) {
  if (!menu.value?.contains(e.target as Node)) close();
}
function outside(e: PointerEvent) {
  if (!menu.value?.contains(e.target as Node) && !trigger.value?.contains(e.target as Node))
    close();
}
async function show() {
  if (props.disabled || !trigger.value) return;
  target.value = trigger.value.closest('dialog') || document.body;
  const rect = trigger.value.getBoundingClientRect();
  const below = window.innerHeight - rect.bottom - 12;
  const above = rect.top - 12;
  const height = Math.min(280, Math.max(80, below >= 180 || below >= above ? below : above));
  const width = Math.min(Math.max(rect.width, 210), window.innerWidth - 24);
  position.value = {
    left: `${Math.max(12, Math.min(rect.left, window.innerWidth - width - 12))}px`,
    top: `${below >= 180 || below >= above ? rect.bottom + 6 : Math.max(12, rect.top - height - 6)}px`,
    width: `${width}px`,
    maxHeight: `${height}px`,
  };
  active.value = Math.max(
    0,
    props.options.findIndex((o) => o.value === props.modelValue),
  );
  open.value = true;
  await nextTick();
  menu.value?.focus();
  document.addEventListener('pointerdown', outside, true);
  window.addEventListener('resize', dismiss);
  window.addEventListener('scroll', scrolled, true);
  scrollActive();
}
function choose(index: number) {
  const option = props.options[index];
  if (!option || option.disabled) return;
  emit('update:modelValue', option.value);
  close(true);
}
function scrollActive() {
  nextTick(() =>
    menu.value
      ?.querySelector(`[data-index="${active.value}"]`)
      ?.scrollIntoView({ block: 'nearest' }),
  );
}
function keydown(e: KeyboardEvent) {
  if (e.key === 'Tab') {
    close();
    return;
  }
  if (e.key === 'Escape') {
    e.preventDefault();
    close(true);
    return;
  }
  if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault();
    choose(active.value);
    return;
  }
  if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
    e.preventDefault();
    const direction = e.key === 'ArrowUp' || e.key === 'End' ? -1 : 1;
    let index = e.key === 'Home' ? -1 : e.key === 'End' ? props.options.length : active.value;
    for (let n = 0; n < props.options.length; n++) {
      index = (index + direction + props.options.length) % props.options.length;
      if (!props.options[index]?.disabled) {
        active.value = index;
        break;
      }
    }
    scrollActive();
  } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
    const now = Date.now();
    typed = now - typedAt < 700 ? typed + e.key : e.key;
    typedAt = now;
    const index = props.options.findIndex(
      (o) =>
        !o.disabled &&
        o.label
          .toLocaleLowerCase(formatLocale())
          .startsWith(typed.toLocaleLowerCase(formatLocale())),
    );
    if (index >= 0) {
      active.value = index;
      scrollActive();
    }
  }
}
onBeforeUnmount(() => close());
</script>
<template>
  <button
    ref="trigger"
    type="button"
    class="select-menu"
    :disabled="disabled"
    aria-haspopup="listbox"
    :aria-label="label"
    :aria-expanded="open"
    :aria-controls="open ? id : undefined"
    @click="open ? close(true) : show()"
    @keydown.down.prevent="show()"
    @keydown.up.prevent="show()"
  >
    <span>{{ selected?.label || placeholder || tr('Auswählen …') }}</span
    ><Icon name="ChevronDown" :size="14" />
  </button>
  <Teleport v-if="open && target" :to="target">
    <div
      :id="id"
      ref="menu"
      class="select-popup"
      role="listbox"
      tabindex="0"
      :aria-label="label"
      :aria-activedescendant="`${id}-${active}`"
      :style="position"
      @keydown="keydown"
      @focusout="
        (e) => {
          if (!menu?.contains(e.relatedTarget as Node)) close();
        }
      "
    >
      <div
        v-for="(option, index) in options"
        :id="`${id}-${index}`"
        :key="String(option.value)"
        :data-index="index"
        role="option"
        :aria-selected="option.value === modelValue"
        :aria-disabled="option.disabled"
        class="select-option"
        :class="{ active: index === active, disabled: option.disabled }"
        @pointermove="active = index"
        @pointerdown.prevent
        @click="choose(index)"
      >
        <span>{{ option.label }}</span
        ><Icon v-if="option.value === modelValue" name="Check" :size="15" />
      </div>
    </div>
  </Teleport>
</template>
