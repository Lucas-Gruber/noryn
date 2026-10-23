<script setup lang="ts">
import { onMounted, ref } from 'vue'
import Icon from './Icon.vue'
defineProps<{ title: string; subtitle?: string; wide?: boolean }>()
const emit = defineEmits<{ close: [] }>()
const dialog = ref<HTMLDialogElement>()
onMounted(() => dialog.value?.showModal())
</script>
<template>
  <dialog ref="dialog" class="modal" :class="{ wide }" @cancel.prevent="emit('close')" @click="e => e.target === dialog && emit('close')">
    <div class="modal-head"><div><h2>{{ title }}</h2><p v-if="subtitle">{{ subtitle }}</p></div><button class="icon-button" aria-label="Schließen" @click="emit('close')"><Icon name="X" /></button></div>
    <slot />
  </dialog>
</template>
