import { computed, reactive, ref } from 'vue';
import { invoke, isTauri, Channel } from '@tauri-apps/api/core';
import type { AuthChallenge, ClientSession, NodeEntry, TerminalEvent, VaultView } from '../types';
import { includesWorkspace } from '../workspaces';

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
  initialPath: '',
  workspace: 'platform',
  favorite: ['web-01', 'db-01'].includes(id),
  notes: '',
});
const demo: VaultView = {
  localRevision: 0,
  teamActivity: {
    platform: [
      {
        id: '00000000-0000-4000-8000-000000000001',
        actor: 'Mara',
        occurredAt: Date.now() - 3_600_000,
        action: 'created',
        object: 'folder',
        targetId: 'prod',
        targetName: 'Production',
        fields: [],
      },
      {
        id: '00000000-0000-4000-8000-000000000002',
        actor: 'Lucas',
        occurredAt: Date.now() - 600_000,
        action: 'updated',
        object: 'connection',
        targetId: 'web-01',
        targetName: 'Web Server 01',
        fields: ['host', 'port'],
      },
    ],
  },
  credentialFolders: [],
  credentialLocations: {},
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
    {
      id: 'personal-key',
      name: 'Mein Firmenkonto',
      username: 'lucas',
      kind: 'key',
      hasPassword: true,
      teamId: null,
    },
    {
      id: 'ops-account',
      name: 'Operations',
      username: 'deploy',
      kind: 'agent',
      hasPassword: false,
      teamId: null,
    },
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
      memberName: 'Lucas',
      serverUrl: 'https://sync.example.com',
      revision: null,
      dirty: false,
      liveSync: false,
    },
  ],
};

function emptyView(): VaultView {
  return {
    localRevision: 0,
    nodes: [],
    credentials: [],
    credentialFolders: [],
    credentialLocations: {},
    bindings: {},
    teams: [],
    teamActivity: {},
  };
}
const vaultView = ref<VaultView>(desktop ? emptyView() : demo);
export const store = reactive({
  // Concurrent IPC replies may arrive out of order. Never replace newer local data.
  view: computed({
    get: () => vaultView.value,
    set: (next: VaultView) => {
      if (next.localRevision >= vaultView.value.localRevision) vaultView.value = next;
    },
  }),
  unlocked: !desktop,
  exists: false,
  vaultPath: '',
  loading: false,
  workspace: desktop ? 'private' : 'platform',
  selected: '',
  folder: 'all',
  query: '',
  protocol: 'all',
  favorites: false,
  page: 'connections' as 'connections' | 'credentials' | 'teams' | 'settings',
  sessions: [] as ClientSession[],
  activeTab: 'connections',
  toast: '',
  error: '',
  authPrompt: null as AuthChallenge | null,
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
      includesWorkspace(store.workspace, n.workspace) &&
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

export async function saveNode(
  node: NodeEntry,
  base = store.view.nodes.find((n) => n.id === node.id) || null,
) {
  if (desktop) store.view = await invoke('node_save', { node, base });
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

export async function removeTeam(id: string, expectedRevision: number) {
  if (desktop) store.view = await invoke('team_remove', { teamId: id, expectedRevision });
  else {
    store.view.teams = store.view.teams.filter((t) => t.id !== id);
    store.view.nodes = store.view.nodes.filter((n) => n.workspace !== id);
    store.view.credentials = store.view.credentials.filter((c) => c.teamId !== id);
    delete store.view.teamActivity[id];
    for (const [node, credential] of Object.entries(store.view.bindings))
      if (
        !store.view.nodes.some((n) => n.id === node) ||
        !store.view.credentials.some((c) => c.id === credential)
      )
        delete store.view.bindings[node];
    for (const credential of Object.keys(store.view.credentialLocations))
      if (!store.view.credentials.some((c) => c.id === credential))
        delete store.view.credentialLocations[credential];
  }
  if (store.workspace === id) store.workspace = 'private';
  if (!store.view.nodes.some((n) => n.id === store.selected)) store.selected = '';
  store.folder = 'all';
  notify('Team von diesem Gerät entfernt. Der Server bleibt unverändert.');
}

export async function copyNodes(
  sourceIds: string[],
  workspace: string,
  parentId: string | null,
  expectedRevision: number,
) {
  let roots: string[];
  if (desktop) {
    const result = await invoke<{ view: VaultView; roots: string[] }>('node_copy', {
      sourceIds,
      workspace,
      parentId,
      expectedRevision,
    });
    store.view = result.view;
    roots = result.roots;
  } else {
    const included = new Set(sourceIds.flatMap((id) => [...descendants(id)]));
    const source = store.view.nodes.filter((n) => included.has(n.id));
    const ids = new Map(source.map((n) => [n.id, crypto.randomUUID()]));
    const copies = source.map((n) => ({
      ...n,
      id: ids.get(n.id)!,
      workspace,
      tags: [...n.tags],
      parentId: (n.parentId && ids.get(n.parentId)) || parentId,
    }));
    roots = copies.filter((n) => n.parentId === parentId).map((n) => n.id);
    store.view.nodes.push(...copies);
    const team = store.view.teams.find((t) => t.id === workspace);
    if (team) team.dirty = true;
  }
  store.workspace = workspace;
  store.folder = 'all';
  store.query = '';
  store.favorites = false;
  store.protocol = 'all';
  store.selected = roots[0] || '';
  notify('Einträge kopiert. Deine Zugangsdaten kannst du am Ziel auswählen.');
}

export async function connect(node: NodeEntry, trustedFingerprint: string | null = null) {
  if (!['ssh', 'sftp', 'ftp', 'ftps'].includes(node.protocol)) {
    fail('RDP und VNC sind für eine spätere Ausbaustufe vorgesehen.');
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
    initialPath: ['sftp', 'ftp', 'ftps'].includes(node.protocol) ? node.initialPath || '' : '',
    port: node.port,
    protocol: node.protocol,
    connected: true,
    preview: !desktop,
    buffer: [],
  });
  try {
    if (desktop) {
      const output = new Channel<TerminalEvent>();
      const authOutput = new Channel<AuthChallenge>();
      authOutput.onmessage = (challenge) => {
        store.authPrompt = challenge;
      };
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
      >('connect_node', { nodeId: node.id, trustedFingerprint, output, authOutput });
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
    store.authPrompt = null;
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
  store.authPrompt = null;
  if (desktop) {
    vaultView.value = emptyView();
    store.unlocked = false;
    store.exists = true;
  } else
    notify(
      'Die Browser-Vorschau speichert keine Zugangsdaten. Der Tresor ist in der Desktop-App verfügbar.',
    );
}
