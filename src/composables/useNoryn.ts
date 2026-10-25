import { computed, reactive, ref } from 'vue';
import { invoke, isTauri, Channel } from '@tauri-apps/api/core';
import type {
  AuthChallenge,
  ClientSession,
  NodeEntry,
  TerminalEvent,
  VaultView,
  ConnectionLogin,
  CredentialDraft,
  LoginHealth,
} from '../types';
import { clearCredential } from '../connectionLogins';
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
  commands: [
    {
      id: '00000000-0000-4000-8000-000000000003',
      workspace: 'platform',
      name: 'Fehlgeschlagene Dienste',
      description: 'Gemeinsamer Diagnosebefehl',
      command: 'systemctl --failed --no-pager',
    },
  ],
  localRevision: 0,
  connectionCredentials: {},
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
    commands: [],
    localRevision: 0,
    connectionCredentials: {},
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
  page: 'connections' as 'connections' | 'credentials' | 'teams' | 'settings' | 'commands',
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
    retry?: () => Promise<void>;
    cancel?: () => void;
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
  const direct = store.view.connectionCredentials?.[nodeId];
  if (direct)
    return {
      credential: direct,
      inherited: false,
      source: 'Direkt an dieser Verbindung',
      direct: true,
    };

  const seen = new Set<string>();
  let id: string | null = nodeId;
  while (id && !seen.has(id)) {
    seen.add(id);
    const binding = store.view.bindings[id];
    if (binding)
      return {
        credential: store.view.credentials.find((c) => c.id === binding),
        inherited: id !== nodeId,
        direct: false,
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

export async function resetVault(id: string, confirmation: string, password: string) {
  if (!desktop) throw new Error('Der Tresor ist in der Desktop-App verfügbar.');
  store.loading = true;
  try {
    const view = await invoke<VaultView>('vault_reset_apply', { id, confirmation, password });
    // A new vault is a new lifecycle. The native revision is above all views
    // issued before locking, so late replies cannot restore the removed tree.
    vaultView.value = view;
    store.sessions = [];
    store.activeTab = 'connections';
    store.workspace = 'private';
    store.selected = '';
    store.folder = 'all';
    store.query = '';
    store.protocol = 'all';
    store.favorites = false;
    store.page = 'connections';
    store.error = '';
    store.challenge = null;
    store.authPrompt = null;
    store.exists = true;
    store.unlocked = true;
  } finally {
    store.loading = false;
  }
}

export async function saveNode(
  node: NodeEntry,
  base = store.view.nodes.find((n) => n.id === node.id) || null,
  login?: ConnectionLogin,
  expectedRevision?: number,
) {
  if (desktop)
    store.view = await invoke('node_save', {
      node,
      base,
      login: login || null,
      expectedRevision: expectedRevision ?? null,
    });
  else {
    if (login?.mode === 'direct' && (node.workspace !== 'private' || node.kind !== 'connection'))
      throw new Error('Direkte Zugangsdaten sind nur für private Verbindungen erlaubt.');
    store.view.connectionCredentials ||= {};
    if (login?.mode === 'direct') {
      const c = login.credential;
      store.view.connectionCredentials[node.id] = {
        id: node.id,
        name: node.name,
        username: c.username,
        kind: c.kind,
        hasPassword: !!c.password,
        teamId: null,
      };
      delete store.view.bindings[node.id];
    } else if (login?.mode === 'account') {
      delete store.view.connectionCredentials[node.id];
      if (login.credentialId) store.view.bindings[node.id] = login.credentialId;
      else delete store.view.bindings[node.id];
    }
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
    for (const id of ids) {
      delete store.view.bindings[id];
      if (store.view.connectionCredentials) delete store.view.connectionCredentials[id];
    }
  }
  if (store.selected === id) store.selected = '';
  if (store.folder === id) store.folder = 'all';
}

export async function setBinding(nodeId: string, credentialId: string | null) {
  if (desktop) store.view = await invoke('binding_set', { nodeId, credentialId });
  else {
    if (store.view.connectionCredentials) delete store.view.connectionCredentials[nodeId];
    if (credentialId) store.view.bindings[nodeId] = credentialId;
    else delete store.view.bindings[nodeId];
  }
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

interface QuickAttempt {
  login: { credentialId: string | null; credential: CredentialDraft | null };
  retry: (fingerprint: string) => Promise<void>;
  cancel: () => void;
}
export async function cancelHostKey() {
  const challenge = store.challenge;
  store.challenge = null;
  challenge?.cancel?.();
}
export async function connectQuick(
  node: NodeEntry,
  login: QuickAttempt['login'],
  done: () => void,
  cancel: () => void,
) {
  const clear = () => {
    if (login.credential) clearCredential(login.credential);
  };
  const attempt: QuickAttempt = {
    login,
    retry: run,
    cancel: () => {
      clear();
      cancel();
    },
  };
  async function run(fingerprint: string | null = null) {
    try {
      if (await connectSession(node, fingerprint, attempt)) {
        clear();
        done();
      }
    } catch (error) {
      clear();
      throw error;
    }
  }
  return run();
}
export async function connect(node: NodeEntry, trustedFingerprint: string | null = null) {
  await connectSession(node, trustedFingerprint);
}
interface SessionContinuation {
  retry: () => Promise<void>;
  cancel: () => void;
  signal: AbortSignal;
  authId?: string;
}
export function sessionForCommand(node: NodeEntry) {
  const username = credentialFor(node.id)?.credential?.username;
  return store.sessions.find(
    (s) =>
      s.connected &&
      s.protocol === 'ssh' &&
      s.nodeId === node.id &&
      s.host === node.host &&
      s.port === node.port &&
      s.username === username,
  );
}
export async function connectForCommand(
  node: NodeEntry,
  signal: AbortSignal,
): Promise<ClientSession> {
  if (signal.aborted || !store.unlocked) throw new Error('Ausführung abgebrochen.');
  const existing = sessionForCommand(node);
  if (existing) return existing;
  if (!credentialFor(node.id)?.credential)
    throw new Error(
      'Bitte deine eigenen Zugangsdaten an dieser Verbindung oder einem übergeordneten Ordner auswählen.',
    );
  if (store.loading || store.challenge || store.authPrompt)
    throw new Error('Es läuft bereits eine Anmeldung. Bitte diese zuerst abschließen.');
  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = (session?: ClientSession, error?: unknown) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener('abort', abort);
      if (session) resolve(session);
      else reject(error || new Error('Anmeldung abgebrochen.'));
    };
    const continuation: SessionContinuation = {
      retry: () => run(fingerprint),
      cancel: () => finish(),
      signal,
    };
    let fingerprint: string | null = null;
    const abort = () => {
      if (store.challenge?.cancel === continuation.cancel) store.challenge = null;
      if (continuation.authId && desktop)
        void invoke('auth_respond', { id: continuation.authId, answers: null }).catch(() => {});
      finish(undefined, new Error('Ausführung abgebrochen.'));
    };
    async function run(trusted: string | null = null) {
      try {
        if (signal.aborted) return abort();
        const session = await connectSession(node, trusted, undefined, continuation);
        if (session) finish(session);
        else if (store.challenge) fingerprint = store.challenge.fingerprint;
        else finish();
      } catch (error) {
        finish(undefined, error);
      }
    }
    signal.addEventListener('abort', abort, { once: true });
    void run();
  });
}
async function connectSession(
  node: NodeEntry,
  trustedFingerprint: string | null,
  quick?: QuickAttempt,
  continuation?: SessionContinuation,
) {
  if (!['ssh', 'sftp', 'ftp', 'ftps'].includes(node.protocol)) {
    fail('RDP und VNC sind für eine spätere Ausbaustufe vorgesehen.');
    return;
  }
  if (!quick && !credentialFor(node.id)?.credential) {
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
    username: quick
      ? quick.login.credential?.username ||
        store.view.credentials.find((c) => c.id === quick.login.credentialId)?.username ||
        ''
      : credentialFor(node.id)?.credential?.username || '',
    temporary: !!quick,
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
        if (continuation) {
          continuation.authId = challenge.id;
          if (continuation.signal.aborted) {
            void invoke('auth_respond', { id: challenge.id, answers: null }).catch(() => {});
            return;
          }
        }
        store.authPrompt = challenge;
      };
      output.onmessage = (event) => {
        // A closed session stays closed. Only an explicit user action may
        // establish a new connection, including after delayed channel events.
        if (!session.connected) return;
        if (event.type === 'data') {
          const bytes = Uint8Array.from(atob(event.data), (c) => c.charCodeAt(0));
          if (session.sink) session.sink(bytes);
          else session.buffer.push(bytes);
        } else {
          session.connected = false;
          session.endReason = event.message;
          const bytes = new TextEncoder().encode(`\r\n\x1b[90m${event.message}\x1b[0m\r\n`);
          if (session.sink) session.sink(bytes);
          else session.buffer.push(bytes);
        }
      };
      const result = await invoke<
        | { type: 'hostKey'; fingerprint: string; previous: string | null; host: string }
        | { type: 'connected'; id: string; home: string; username: string }
      >(
        quick ? 'quick_connect' : 'connect_node',
        quick
          ? { request: { node, ...quick.login }, trustedFingerprint, output, authOutput }
          : { nodeId: node.id, trustedFingerprint, output, authOutput },
      );
      if (result.type === 'hostKey') {
        store.challenge = {
          ...result,
          nodeId: node.id,
          ...(continuation
            ? { retry: continuation.retry, cancel: continuation.cancel }
            : quick
              ? { retry: () => quick.retry(result.fingerprint), cancel: quick.cancel }
              : {}),
        };
        if (continuation?.signal.aborted) {
          store.challenge = null;
          throw new Error('Ausführung abgebrochen.');
        }
        return false;
      }
      if (!store.unlocked || continuation?.signal.aborted) {
        await invoke('connection_close', { id: result.id }).catch(() => {});
        throw new Error('Der Tresor wurde während des Verbindungsaufbaus gesperrt.');
      }
      Object.assign(session, { id: result.id, home: result.home, username: result.username });
    } else {
      session.id = crypto.randomUUID();
      session.home = '/home/lucas';
    }
    store.sessions.push(session);
    if (!continuation) store.activeTab = session.id;
    store.challenge = null;
    if (session.protocol === 'ssh') void loadLoginHealth(session);
    return session;
  } finally {
    store.loading = false;
    store.authPrompt = null;
  }
}

export async function loadLoginHealth(session: ClientSession) {
  if (!session.connected || session.healthLoading || session.protocol !== 'ssh') return;
  session.healthLoading = true;
  session.healthError = '';
  try {
    let report: LoginHealth;
    if (desktop) {
      const result = await invoke<{ report: LoginHealth; view: VaultView }>('ssh_login_health', {
        id: session.id,
      });
      if (!store.unlocked || !session.connected || !store.sessions.some((s) => s.id === session.id))
        return;
      store.view = result.view;
      report = result.report;
    } else {
      const now = Math.floor(Date.now() / 1000);
      report = {
        capturedAt: now,
        previousLogin: now - 86400,
        uptime: 432000,
        cores: 4,
        servicesAvailable: true,
        journalAvailable: true,
        portsAvailable: true,
        services: [
          {
            name: 'backup.service',
            state: 'failed',
            description: 'Nightly backup',
            becameInactive: true,
          },
        ],
        events: [
          {
            at: now - 7200,
            unit: 'backup.service',
            message: 'Backup process exited with status 1.',
          },
        ],
        updates: { manager: 'apt', total: 12, security: 3 },
        peaks: [{ at: now - 3600, load: 6.2 }],
        loadSamples: 144,
        loadWindow: now - 86400,
        ports: [
          {
            protocol: 'tcp',
            address: '0.0.0.0',
            port: 22,
            process: 'sshd',
            forwardHost: '127.0.0.1',
          },
          {
            protocol: 'tcp',
            address: '127.0.0.1',
            port: 5432,
            process: 'postgres',
            forwardHost: '127.0.0.1',
          },
          {
            protocol: 'udp',
            address: '0.0.0.0',
            port: 53,
            process: 'dns',
            forwardHost: '127.0.0.1',
          },
        ],
        notes: ['Browser-Vorschau mit Beispieldaten.'],
        activeServices: ['sshd.service', 'postgresql.service'],
      };
    }
    session.health = report;
  } catch (e) {
    if (session.connected && store.sessions.some((s) => s.id === session.id))
      session.healthError = String(e);
  } finally {
    session.healthLoading = false;
  }
}

export async function closeSession(id: string) {
  if (desktop) await invoke('connection_close', { id });
  store.sessions = store.sessions.filter((s) => s.id !== id);
  if (store.activeTab === id) store.activeTab = 'connections';
}

export async function reconnectSession(session: ClientSession) {
  if (store.loading || session.connected || session.temporary) return;
  const node = store.view.nodes.find((node) => node.id === session.nodeId);
  if (!node) throw new Error('Der gespeicherte Verbindungseintrag wurde entfernt.');
  await connect(node);
}

export async function lock() {
  if (desktop) await invoke('vault_lock');
  store.sessions = [];
  store.activeTab = 'connections';
  store.selected = '';
  store.query = '';
  store.error = '';
  await cancelHostKey();
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

export async function saveQuickSession(
  session: ClientSession,
  name: string,
  parentId: string | null,
) {
  if (!session.temporary) throw new Error('Diese Verbindung ist bereits gespeichert.');
  let node: NodeEntry;
  if (desktop) {
    const result = await invoke<{ view: VaultView; node: NodeEntry }>('quick_save', {
      sessionId: session.id,
      name,
      parentId,
    });
    store.view = result.view;
    node = result.node;
  } else {
    node = {
      id: crypto.randomUUID(),
      parentId,
      workspace: 'private',
      kind: 'connection',
      name,
      host: session.host,
      port: session.port,
      protocol: session.protocol,
      initialPath: session.initialPath,
      notes: '',
      tags: [],
      favorite: false,
    };
    await saveNode(node, null);
    // Browser mode keeps metadata only; the user must choose real credentials in the desktop app.
  }
  session.nodeId = node.id;
  session.name = node.name;
  session.temporary = false;
  notify('Verbindung im privaten Tresor gespeichert.');
}
