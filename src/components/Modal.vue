<script setup lang="ts">
import { t as tr } from '../i18n';

import { onMounted, ref } from 'vue';
import Icon from './Icon.vue';
const props = defineProps<{ title: string; subtitle?: string; wide?: boolean; busy?: boolean }>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement>();
function close() {
  if (!props.busy) emit('close');
}
onMounted(() => dialog.value?.showModal());
</script>
<template>
  <dialog
    ref="dialog"
    class="modal"
    :class="{ wide }"
    :aria-busy="busy || undefined"
    @cancel.prevent="close"
    @click="(e) => e.target === dialog && close()"
  >
    <div class="modal-head">
      <div>
        <h2>{{ title }}</h2>
        <p v-if="subtitle">{{ subtitle }}</p>
      </div>
      <button class="icon-button" :aria-label="tr('Schließen')" :disabled="busy" @click="close">
        <Icon name="X" />
      </button>
    </div>
    <slot />
  </dialog>
</template>
