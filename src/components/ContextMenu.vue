<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { choose, closeMenu, menu } from '../composables/useContextMenu';
import Icon from './Icon.vue';
const element = ref<HTMLElement>();
const left = ref(0),
  top = ref(0);
watch(
  () => [menu.open, menu.x, menu.y],
  async () => {
    if (!menu.open) return;
    left.value = menu.x;
    top.value = menu.y;
    await nextTick();
    const box = element.value?.getBoundingClientRect();
    if (box) {
      left.value = Math.max(8, Math.min(menu.x, window.innerWidth - box.width - 8));
      top.value = Math.max(8, Math.min(menu.y, window.innerHeight - box.height - 8));
    }
    element.value?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  },
);
function outside(event: PointerEvent) {
  if (!element.value?.contains(event.target as Node)) closeMenu();
}
function keys(event: KeyboardEvent) {
  if (!menu.open) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    closeMenu();
  }
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const buttons = Array.from(
      element.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') || [],
    );
    const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[
      (current + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
    ]?.focus();
  }
}
window.addEventListener('pointerdown', outside, true);
window.addEventListener('keydown', keys);
onBeforeUnmount(() => {
  window.removeEventListener('pointerdown', outside, true);
  window.removeEventListener('keydown', keys);
});
</script>
<template>
  <Teleport to="body"
    ><div
      v-if="menu.open"
      ref="element"
      class="context-menu"
      role="menu"
      :style="{ left: left + 'px', top: top + 'px' }"
      @contextmenu.prevent
    >
      <template v-for="(item, i) in menu.items" :key="i"
        ><div v-if="item.separator" class="menu-separator"></div>
        <button
          role="menuitem"
          :disabled="item.disabled"
          :class="{ danger: item.danger }"
          @click="choose(item)"
        >
          <Icon :name="item.icon || 'ArrowRight'" :size="15" /><span>{{ item.label }}</span
          ><kbd v-if="item.shortcut">{{ item.shortcut }}</kbd>
        </button></template
      >
    </div></Teleport
  >
</template>
