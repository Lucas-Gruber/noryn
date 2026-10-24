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
  object: 'folder' | 'connection' | 'credential' | 'workspace';
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
