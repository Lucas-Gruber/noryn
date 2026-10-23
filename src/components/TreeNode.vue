<script setup lang="ts">
import { computed, ref } from 'vue';
import type { NodeEntry } from '../types';
import { store, descendants } from '../composables/useHarbor';
import Icon from './Icon.vue';
const props = defineProps<{ node: NodeEntry; level?: number }>();
const emit = defineEmits<{ edit: [node: NodeEntry]; move: [nodeId: string, parentId: string] }>();
const expanded = ref(true);
const children = computed(() =>
  store.view.nodes
    .filter((n) => n.parentId === props.node.id)
    .sort(
      (a, b) =>
        (a.kind === 'folder' ? 0 : 1) - (b.kind === 'folder' ? 0 : 1) ||
        a.name.localeCompare(b.name),
    ),
);
const count = computed(
  () =>
    store.view.nodes.filter((n) => n.kind === 'connection' && descendants(props.node.id).has(n.id))
      .length,
);
function select() {
  store.page = 'connections';
  store.activeTab = 'connections';
  if (props.node.kind === 'folder') {
    store.folder = props.node.id;
    store.selected = props.node.id;
  } else store.selected = props.node.id;
}
function drop(event: DragEvent) {
  event.preventDefault();
  const id = event.dataTransfer?.getData('application/harbor-node');
  if (id && props.node.kind === 'folder') emit('move', id, props.node.id);
}
</script>
<template>
  <div
    class="tree-node"
    :class="{ active: store.folder === node.id || store.selected === node.id }"
    :style="{ paddingLeft: `${12 + (level || 0) * 16}px` }"
    draggable="true"
    @dragstart="(e: DragEvent) => e.dataTransfer?.setData('application/harbor-node', node.id)"
    @dragover="node.kind === 'folder' && $event.preventDefault()"
    @drop="drop"
    @click="select"
    @dblclick="emit('edit', node)"
  >
    <button
      v-if="node.kind === 'folder'"
      class="tree-toggle"
      :aria-label="expanded ? 'Einklappen' : 'Aufklappen'"
      @click.stop="expanded = !expanded"
    >
      <Icon :name="expanded ? 'ChevronDown' : 'ChevronRight'" :size="12" /></button
    ><span v-else class="tree-spacer"></span>
    <Icon
      :name="node.kind === 'folder' ? 'Folder' : node.protocol === 'sftp' ? 'Files' : 'Terminal'"
      :size="15"
    />
    <span class="ellipsis">{{ node.name }}</span
    ><span v-if="node.kind === 'folder'" class="tree-count">{{ count }}</span>
  </div>
  <template v-if="expanded && node.kind === 'folder'"
    ><TreeNode
      v-for="child in children"
      :key="child.id"
      :node="child"
      :level="(level || 0) + 1"
      @edit="emit('edit', $event)"
      @move="(id, parent) => emit('move', id, parent)"
  /></template>
</template>
