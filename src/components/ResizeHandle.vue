<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue';
const props = defineProps<{
  modelValue: number;
  min: number;
  max: number;
  reset: number;
  label: string;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: number] }>();
const dragging = ref(false);
let startX = 0,
  startValue = 0;
function change(value: number) {
  emit('update:modelValue', Math.max(props.min, Math.min(props.max, value)));
}
function down(event: PointerEvent) {
  if (event.button !== 0) return;
  event.preventDefault();
  (event.currentTarget as HTMLElement).focus();
  startX = event.clientX;
  startValue = props.modelValue;
  dragging.value = true;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  document.documentElement.classList.add('resizing-panels');
}
function end() {
  dragging.value = false;
  document.documentElement.classList.remove('resizing-panels');
}
function keys(e: KeyboardEvent) {
  if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) {
    e.preventDefault();
    change(
      e.key === 'Home'
        ? props.min
        : e.key === 'End'
          ? props.max
          : props.modelValue + (e.key === 'ArrowLeft' ? -1 : 1) * (e.shiftKey ? 40 : 10),
    );
  }
}
onBeforeUnmount(end);
</script>
<template>
  <div
    class="resize-handle"
    :class="{ dragging }"
    role="separator"
    tabindex="0"
    aria-orientation="vertical"
    :aria-label="label"
    :aria-valuemin="Math.round(min)"
    :aria-valuemax="Math.round(max)"
    :aria-valuenow="Math.round(modelValue)"
    :title="`${label} · ziehen oder Pfeiltasten · Doppelklick zum Zurücksetzen`"
    @pointerdown="down"
    @pointermove="dragging && change(startValue + $event.clientX - startX)"
    @pointerup="end"
    @pointercancel="end"
    @lostpointercapture="end"
    @keydown="keys"
    @dblclick="change(reset)"
  >
    <span></span>
  </div>
</template>
