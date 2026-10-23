<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import type { TeamInfo } from '../types';
import { ALL_WORKSPACES } from '../workspaces';
import Icon from './Icon.vue';

const props = defineProps<{ workspace: string; teams: TeamInfo[] }>();
const emit = defineEmits<{ select: [id: string]; manage: [] }>();
const root = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const list = ref<HTMLElement>();
const open = ref(false);
const options = computed(() => [
  {
    id: ALL_WORKSPACES,
    name: 'Alle Workspaces',
    description: 'Teams und private Verbindungen',
    icon: 'LayoutGrid',
  },
  ...props.teams.map((team) => ({
    id: team.id,
    name: team.name,
    description: 'Gemeinsame Verbindungen',
    icon: 'Users',
  })),
  {
    id: 'private',
    name: 'Privater Workspace',
    description: 'Nur für dich sichtbar',
    icon: 'LockKeyhole',
  },
]);
const current = computed(
  () => options.value.find((option) => option.id === props.workspace) || options.value.at(-1)!,
);

async function show() {
  open.value = true;
  await nextTick();
  const index = options.value.findIndex((option) => option.id === props.workspace);
  focusOption(Math.max(0, index));
}
function close(restoreFocus = false) {
  open.value = false;
  if (restoreFocus) trigger.value?.focus();
}
function focusOption(index: number) {
  list.value?.querySelectorAll<HTMLButtonElement>('[role="option"]')[index]?.focus();
}
function choose(id: string) {
  close(true);
  if (id !== props.workspace) emit('select', id);
}
function keys(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    close(true);
    return;
  }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const buttons = Array.from(list.value?.querySelectorAll('[role="option"]') || []);
  const index = buttons.indexOf(document.activeElement as HTMLElement);
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? buttons.length - 1
        : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
  focusOption(next);
}
function outside(event: PointerEvent) {
  if (!root.value?.contains(event.target as Node)) close();
}
function leave(event: FocusEvent) {
  if (!root.value?.contains(event.relatedTarget as Node | null)) close();
}
function manage() {
  close(true);
  emit('manage');
}
onMounted(() => document.addEventListener('pointerdown', outside));
onBeforeUnmount(() => document.removeEventListener('pointerdown', outside));
</script>

<template>
  <div ref="root" class="workspace-switch" @focusout="leave">
    <button
      ref="trigger"
      type="button"
      class="workspace-switch-trigger"
      :class="{ expanded: open }"
      aria-label="Arbeitsbereich wechseln"
      aria-describedby="current-workspace-name"
      aria-haspopup="listbox"
      aria-controls="workspace-options"
      :aria-expanded="open"
      @click="open ? close() : show()"
      @keydown.down.prevent="show"
      @keydown.up.prevent="show"
      @keydown.esc.prevent="close()"
    >
      <span class="workspace-switch-icon"><Icon :name="current.icon" :size="19" /></span>
      <span class="workspace-switch-copy">
        <span class="workspace-label">Workspace</span>
        <span id="current-workspace-name" class="workspace-switch-name ellipsis">{{
          current.name
        }}</span>
      </span>
      <Icon name="ChevronsUpDown" :size="15" class="workspace-switch-chevron" />
    </button>
    <div v-if="open" class="workspace-dropdown" @keydown.esc="keys">
      <div class="workspace-dropdown-heading">Arbeitsbereich wechseln</div>
      <div
        id="workspace-options"
        ref="list"
        class="workspace-options"
        role="listbox"
        aria-label="Arbeitsbereiche"
        @keydown="keys"
      >
        <button
          v-for="option in options"
          :key="option.id"
          type="button"
          role="option"
          tabindex="-1"
          :aria-selected="option.id === workspace"
          class="workspace-option"
          :class="{ selected: option.id === workspace }"
          @click="choose(option.id)"
        >
          <span class="workspace-option-icon"><Icon :name="option.icon" :size="18" /></span>
          <span class="workspace-option-copy">
            <span class="workspace-option-name ellipsis">{{ option.name }}</span>
            <span class="workspace-option-description">{{ option.description }}</span>
          </span>
          <Icon v-if="option.id === workspace" name="Check" :size="16" class="workspace-check" />
        </button>
      </div>
      <div class="workspace-dropdown-footer">
        <button type="button" @click="manage">
          <Icon name="Settings2" :size="15" />Teams verwalten<Icon name="ArrowRight" :size="14" />
        </button>
      </div>
    </div>
  </div>
</template>
