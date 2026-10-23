import { computed, reactive } from 'vue';
import { invoke, isTauri, Channel } from '@tauri-apps/api/core';
import type { ClientSession, NodeEntry, TerminalEvent, VaultView } from '../types';

export const desktop = isTauri();
const sample = (
  id: string,
  parentId: string | null,
  kind: 'folder' | 'connection',
  name: string,
  host = '',
  tags: string[] = [],
  protocol: NodeEntry['protocol'] = 'ssh',
): NodeEntry => ({
  id,
  parentId,
  kind,
  name,
  host,
  tags,
  protocol,
  port: 22,
  workspace: 'platform',
  favorite: ['web-01', 'db-01'].includes(id),
  notes: '',
});
const demo: VaultView = {
  nodes: [
    sample('prod', null, 'folder', 'Production'),
    sample('web-01', 'prod', 'connection', 'Web Server 01', '10.20.1.10', ['production', 'web']),
    sample('web-02', 'prod', 'connection', 'Web Server 02', '10.20.1.11', ['production', 'web']),
    sample('db-01', 'prod', 'connection', 'PostgreSQL Primary', '10.20.2.20', [
      'production',
      'database',
    ]),
    sample('storage', 'prod', 'connection', 'Storage & Backups', '10.20.3.30', ['backup'], 'sftp'),
    sample('staging', null, 'folder', 'Staging'),
    sample('stage-01', 'staging', 'connection', 'Staging Environment', '10.30.1.10', [
      'staging',
      'web',
    ]),
    sample('tools', null, 'folder', 'Infrastructure'),
    sample('monitor', 'tools', 'connection', 'Monitoring', '10.20.4.10', ['observability']),
    { ...sample('homelab', null, 'folder', 'Homelab'), workspace: 'private' },
    {
      ...sample('nas', 'homelab', 'connection', 'Home NAS', '192.168.1.50', ['home'], 'sftp'),
      workspace: 'private',
    },
  ],
  credentials: [
    { id: 'personal-key', name: 'Mein SSH-Schlüssel', username: 'lucas', kind: 'key' },
    { id: 'ops-account', name: 'Operations', username: 'deploy', kind: 'agent' },
  ],
  bindings: {
    prod: 'personal-key',
    staging: 'ops-account',
    tools: 'personal-key',
    homelab: 'personal-key',
  },
  teams: [
    {
      id: 'platform',
      name: 'Platform Team',
      serverUrl: 'https://sync.example.com',
      revision: null,
      dirty: false,
    },
  ],
};

export const store = reactive({
  view: (desktop ? { nodes: [], credentials: [], bindings: {}, teams: [] } : demo) as VaultView,
  unlocked: !desktop,
  exists: false,
  loading: false,
  workspace: desktop ? 'private' : 'platform',
  selected: '',
  folder: 'all',
  query: '',
  protocol: 'all',
  favorites: false,
  page: 'connections' as 'connections' | 'credentials' | 'teams',
  sessions: [] as ClientSession[],
  activeTab: 'connections',
  toast: '',
  error: '',
  challenge: null as null | {
    fingerprint: string;
    previous: string | null;
    host: string;
    nodeId: string;
  },
});
let toastTimer: ReturnType<typeof setTimeout>;
export function notify(message: string) {
  store.toast = message;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (store.toast = ''), 4500);
}
export function fail(error: unknown) {
  store.error = String(error);
}
export async function action<T>(fn: () => Promise<T>): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    fail(e);
    return undefined;
  }
}

export function descendants(id: string, nodes = store.view.nodes): Set<string> {
  const ids = new Set([id]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const node of nodes)
      if (node.parentId && ids.has(node.parentId) && !ids.has(node.id)) {
        ids.add(node.id);
        changed = true;
      }
  }
  return ids;
}

export function credentialFor(nodeId: string) {
  const seen = new Set<string>();
  let id: string | null = nodeId;
  while (id && !seen.has(id)) {
    seen.add(id);
    const binding = store.view.bindings[id];
    if (binding)
      return {
        credential: store.view.credentials.find((c) => c.id === binding),
        inherited: id !== nodeId,
        source: store.view.nodes.find((n) => n.id === id)?.name,
      };
    id = store.view.nodes.find((n) => n.id === id)?.parentId ?? null;
  }
  return undefined;
}

export const connections = computed(() => {
  const ids = store.folder === 'all' ? null : descendants(store.folder);
  const query = store.query.toLowerCase().trim();
  return store.view.nodes.filter(
    (n) =>
      n.workspace === store.workspace &&
      n.kind === 'connection' &&
      (!ids || ids.has(n.id)) &&
      (!store.favorites || n.favorite) &&
      (store.protocol === 'all' || n.protocol === store.protocol) &&
      (!query || [n.name, n.host, ...n.tags].join(' ').toLowerCase().includes(query)),
  );
});

export async function unlock(password: string) {
  store.loading = true;
  try {
    store.view = await invoke<VaultView>('vault_unlock', { password });
    store.unlocked = true;
  } finally {
    store.loading = false;
  }
}

export async function saveNode(node: NodeEntry) {
  if (desktop) store.view = await invoke('node_save', { node });
  else {
    store.view.nodes = [...store.view.nodes.filter((n) => n.id !== node.id), node];
    const team = store.view.teams.find((t) => t.id === node.workspace);
    if (team) team.dirty = true;
  }
}

export async function deleteNode(id: string) {
  if (desktop) store.view = await invoke('node_delete', { id });
  else {
    const ids = descendants(id);
    store.view.nodes = store.view.nodes.filter((n) => !ids.has(n.id));
    for (const id of ids) delete store.view.bindings[id];
  }
  if (store.selected === id) store.selected = '';
  if (store.folder === id) store.folder = 'all';
}

export async function setBinding(nodeId: string, credentialId: string | null) {
  if (desktop) store.view = await invoke('binding_set', { nodeId, credentialId });
  else if (credentialId) store.view.bindings[nodeId] = credentialId;
  else delete store.view.bindings[nodeId];
  notify('Persönliche Zuordnung gespeichert');
}

export async function connect(node: NodeEntry, trustedFingerprint: string | null = null) {
  if (!['ssh', 'sftp'].includes(node.protocol)) {
    fail(
      'RDP, VNC und FTP sind für eine spätere Ausbaustufe vorgesehen. SSH und SFTP sind bereits verfügbar.',
    );
    return;
  }
  if (!credentialFor(node.id)?.credential) {
    store.selected = node.id;
    fail('Bitte zuerst deine Zugangsdaten am Eintrag oder einem übergeordneten Ordner auswählen.');
    return;
  }
  store.loading = true;
  const session = reactive<ClientSession>({
    id: '',
    nodeId: node.id,
    name: node.name,
    host: node.host,
    username: credentialFor(node.id)?.credential?.username || '',
    home: '/',
    port: node.port,
    connected: true,
    preview: !desktop,
    buffer: [],
  });
  try {
    if (desktop) {
      const output = new Channel<TerminalEvent>();
      output.onmessage = (event) => {
        if (event.type === 'data') {
          const bytes = Uint8Array.from(atob(event.data), (c) => c.charCodeAt(0));
          if (session.sink) session.sink(bytes);
          else session.buffer.push(bytes);
        } else {
          session.connected = false;
          const bytes = new TextEncoder().encode(`\r\n\x1b[90m${event.message}\x1b[0m\r\n`);
          if (session.sink) session.sink(bytes);
          else session.buffer.push(bytes);
        }
      };
      const result = await invoke<
        | { type: 'hostKey'; fingerprint: string; previous: string | null; host: string }
        | { type: 'connected'; id: string; home: string; username: string }
      >('connect_node', { nodeId: node.id, trustedFingerprint, output });
      if (result.type === 'hostKey') {
        store.challenge = { ...result, nodeId: node.id };
        return;
      }
      Object.assign(session, { id: result.id, home: result.home, username: result.username });
    } else {
      session.id = crypto.randomUUID();
      session.home = '/home/lucas';
    }
    store.sessions.push(session);
    store.activeTab = session.id;
    store.challenge = null;
  } finally {
    store.loading = false;
  }
}

export async function closeSession(id: string) {
  if (desktop) await invoke('connection_close', { id });
  store.sessions = store.sessions.filter((s) => s.id !== id);
  if (store.activeTab === id) store.activeTab = 'connections';
}

export async function lock() {
  if (desktop) await invoke('vault_lock');
  store.sessions = [];
  store.activeTab = 'connections';
  store.selected = '';
  store.query = '';
  store.error = '';
  store.challenge = null;
  if (desktop) {
    store.view = { nodes: [], credentials: [], bindings: {}, teams: [] };
    store.unlocked = false;
    store.exists = true;
  } else
    notify(
      'Die Browser-Vorschau speichert keine Zugangsdaten. Der Tresor ist in der Desktop-App verfügbar.',
    );
}
