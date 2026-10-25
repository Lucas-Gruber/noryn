export interface NodeEntry {
  id: string;
  parentId: string | null;
  workspace: string;
  kind: 'folder' | 'connection';
  name: string;
  protocol: 'ssh' | 'sftp' | 'rdp' | 'vnc' | 'ftp' | 'ftps';
  host: string;
  port: number;
  initialPath: string;
  notes: string;
  tags: string[];
  favorite: boolean;
}
export interface CredentialInfo {
  id: string;
  name: string;
  username: string;
  kind: 'password' | 'key' | 'agent';
  hasPassword: boolean;
  teamId: string | null;
}
export interface CredentialDraft {
  id: string;
  name: string;
  username: string;
  kind: 'password' | 'key' | 'agent';
  secret: string;
  passphrase: string;
  password: string;
  teamId: null;
}
export type ConnectionLogin =
  | { mode: 'keep' }
  | { mode: 'account'; credentialId: string | null }
  | { mode: 'direct'; credential: CredentialDraft };
export interface CredentialFolder {
  id: string;
  parentId: string | null;
  name: string;
}
export interface TeamInfo {
  id: string;
  name: string;
  memberName: string;
  serverUrl: string;
  revision: string | null;
  dirty: boolean;
  liveSync: boolean;
}
export interface VaultView {
  commands: SavedCommand[];
  connectionCredentials: Record<string, CredentialInfo>;
  localRevision: number;
  nodes: NodeEntry[];
  credentials: CredentialInfo[];
  credentialFolders: CredentialFolder[];
  credentialLocations: Record<string, string>;
  bindings: Record<string, string>;
  teams: TeamInfo[];
  teamActivity: Record<string, TeamChange[]>;
}
export type ChangedField =
  | 'command'
  | 'name'
  | 'parent'
  | 'kind'
  | 'host'
  | 'port'
  | 'protocol'
  | 'initialPath'
  | 'notes'
  | 'tags'
  | 'favorite'
  | 'username'
  | 'privateKey'
  | 'passphrase'
  | 'password'
  | 'children';
export interface TeamChange {
  id: string;
  actor: string;
  occurredAt: number;
  action: 'created' | 'updated' | 'deleted' | 'shared' | 'unshared' | 'imported';
  object: 'folder' | 'connection' | 'credential' | 'workspace' | 'command';
  targetId: string;
  targetName: string;
  fields: ChangedField[];
}
export interface RemoteEntry {
  name: string;
  path: string;
  isDir: boolean;
  isLink: boolean;
  isFile?: boolean;
  size: number;
  modified: number;
  permissions: string;
}
export interface ClientSession {
  health?: LoginHealth;
  healthLoading?: boolean;
  healthError?: string;
  endReason?: string;
  editedFiles?: number;
  temporary?: boolean;
  id: string;
  nodeId: string;
  name: string;
  host: string;
  username: string;
  home: string;
  initialPath: string;
  port: number;
  protocol: NodeEntry['protocol'];
  connected: boolean;
  preview: boolean;
  buffer: Uint8Array[];
  sink?: (data: Uint8Array) => void;
}
export type TerminalEvent = { type: 'data'; data: string } | { type: 'closed'; message: string };

export interface AuthChallenge {
  id: string;
  host: string;
  username: string;
  instructions: string;
  prompts: { text: string; echo: boolean }[];
}
export interface LocalDirectory {
  path: string;
  parent: string | null;
  separator: string;
  entries: RemoteEntry[];
}

export interface ForwardInfo {
  id: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
  activeConnections: number;
  uploaded: number;
  downloaded: number;
  lastError: string | null;
}
export type SystemSection = 'processes' | 'services' | 'hardware';
export interface SystemSnapshot {
  section: SystemSection;
  capturedAt: number;
  message: string | null;
  processes: {
    pid: number;
    parentPid: number;
    user: string;
    cpu: number;
    memoryPercent: number;
    memory: number;
    name: string;
  }[];
  services: {
    name: string;
    load: string;
    active: string;
    state: string;
    description: string;
    fileState: string;
  }[];
  hardware: null | {
    hostname: string;
    os: string;
    kernel: string;
    architecture: string;
    cpuModel: string;
    logicalCpus: number;
    memoryTotal: number;
    memoryAvailable: number;
    swapTotal: number;
    swapFree: number;
    uptime: number;
    load: number[];
    cpuTicks: { total: number; idle: number };
    io: { name: string; read: number; written: number }[];
    network: { name: string; received: number; sent: number }[];
    disks: { name: string; total: number; used: number; available: number; mount: string }[];
  };
}

export type HardwareInfo = NonNullable<SystemSnapshot['hardware']>;
export interface StorageInfo {
  mounts: { source: string; target: string; filesystem: string; options: string }[];
  pvs: Record<string, string>[];
  vgs: Record<string, string>[];
  lvs: Record<string, string>[];
  messages: string[];
}
export type SystemRequest =
  | {
      kind:
        | 'serviceStatus'
        | 'serviceFile'
        | 'serviceStart'
        | 'serviceStop'
        | 'serviceRestart'
        | 'serviceReload'
        | 'serviceEnable'
        | 'serviceDisable';
      name: string;
    }
  | { kind: 'processInfo'; pid: number }
  | { kind: 'processTerminate' | 'processKill'; pid: number; startTicks: number }
  | { kind: 'storage' };
export type SystemReply =
  | { type: 'completed'; output: string; startTicks: number | null; storage: StorageInfo | null }
  | { type: 'passwordRequired'; message: string };

export interface SavedCommand {
  id: string;
  workspace: string;
  name: string;
  description: string;
  command: string;
}

export interface ListeningPort {
  protocol: 'tcp' | 'udp';
  address: string;
  port: number;
  process: string;
  forwardHost: string;
}
export interface LoginHealth {
  capturedAt: number;
  previousLogin: number | null;
  uptime: number;
  cores: number;
  servicesAvailable: boolean;
  journalAvailable: boolean;
  portsAvailable: boolean;
  services: { name: string; state: string; description: string; becameInactive: boolean }[];
  events: { at: number; unit: string; message: string }[];
  updates: { manager: string; total: number; security: number | null } | null;
  peaks: { at: number; load: number }[];
  loadSamples: number;
  loadWindow: number;
  ports: ListeningPort[];
  notes: string[];
  activeServices: string[];
}
