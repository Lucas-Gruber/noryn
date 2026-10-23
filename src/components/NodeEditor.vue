<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import type { NodeEntry } from '../types'
import { action, descendants, notify, saveNode, setBinding, store } from '../composables/useHarbor'
import Modal from './Modal.vue'
import Icon from './Icon.vue'
const props = defineProps<{ node?: NodeEntry; kind?: 'folder' | 'connection' }>()
const emit = defineEmits<{ close: [] }>()
const form = reactive<NodeEntry>(props.node ? { ...props.node, tags: [...props.node.tags] } : {
  id: crypto.randomUUID(), parentId: store.folder === 'all' ? null : store.folder, workspace: store.workspace,
  kind: props.kind || 'connection', name: '', protocol: 'ssh', host: '', port: 22, notes: '', tags: [], favorite: false,
})
const tags = ref(form.tags.join(', '))
const binding = ref(store.view.bindings[form.id] || '')
const busy = ref(false)
const folders = computed(() => store.view.nodes.filter(n => n.workspace === form.workspace && n.kind === 'folder' && !descendants(form.id).has(n.id)))
async function submit() {
  busy.value = true
  await action(async () => {
    form.tags = tags.value.split(',').map(t => t.trim()).filter(Boolean)
    await saveNode({ ...form, parentId: form.parentId || null })
    if ((store.view.bindings[form.id] || '') !== binding.value) await setBinding(form.id, binding.value || null)
    store.selected = form.id; notify(props.node ? 'Änderungen gespeichert' : form.kind === 'folder' ? 'Ordner angelegt' : 'Verbindung angelegt'); emit('close')
  })
  busy.value = false
}
</script>
<template>
  <Modal :title="`${node ? 'Bearbeiten' : kind === 'folder' ? 'Neuer Ordner' : 'Neue Verbindung'}`" :subtitle="form.workspace === 'private' ? 'Nur in deinem privaten Arbeitsbereich.' : 'Dieser Eintrag wird mit deinem Team geteilt.'" @close="emit('close')">
    <form @submit.prevent="submit" class="form-body">
      <label>Name<input v-model="form.name" placeholder="z. B. Production Web Server" required maxlength="512" autofocus /></label>
      <div class="form-row"><label>Übergeordneter Ordner<select v-model="form.parentId"><option :value="null">Kein Ordner · Hauptebene</option><option v-for="folder in folders" :key="folder.id" :value="folder.id">{{ folder.name }}</option></select></label>
      <label v-if="form.kind === 'connection'">Protokoll<select v-model="form.protocol" @change="form.port = ['ssh', 'sftp'].includes(form.protocol) ? 22 : form.protocol === 'rdp' ? 3389 : form.protocol === 'vnc' ? 5900 : 21"><option value="ssh">SSH + SFTP</option><option value="sftp">SFTP über SSH</option><option value="rdp">RDP · geplant</option><option value="vnc">VNC · geplant</option><option value="ftp">FTP · geplant</option></select></label></div>
      <div v-if="form.kind === 'connection'" class="form-row host-row"><label>Hostname / IP-Adresse<input v-model="form.host" placeholder="10.20.1.10" required maxlength="253" /></label><label>Port<input v-model.number="form.port" type="number" min="1" max="65535" required /></label></div>
      <label>Kategorien / Tags<input v-model="tags" placeholder="production, web, eu-central" /></label>
      <label>Notizen<textarea v-model="form.notes" placeholder="Informationen für dich und dein Team …" rows="3" /></label>
      <div class="local-field"><div class="field-heading"><Icon name="KeyRound" :size="16" /><span>Meine Zugangsdaten</span><span class="small-badge"><Icon name="LockKeyhole" :size="10" /> Nur lokal</span></div>
      <select v-model="binding"><option value="">Vom übergeordneten Ordner erben</option><option v-for="credential in store.view.credentials" :key="credential.id" :value="credential.id">{{ credential.name }} · {{ credential.username }}</option></select><p>Deine Auswahl gilt nur für dich. Jedes Teammitglied verbindet sich mit dem eigenen Konto.</p></div>
      <div class="modal-actions"><button type="button" class="button secondary" @click="emit('close')">Abbrechen</button><button class="button primary" :disabled="busy"><Icon :name="busy ? 'LoaderCircle' : 'Check'" :class="{ spin: busy }" :size="16" />Speichern</button></div>
    </form>
  </Modal>
</template>
