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
export interface TeamInfo {
  id: string;
  name: string;
  serverUrl: string;
  revision: string | null;
  dirty: boolean;
}
export interface VaultView {
  nodes: NodeEntry[];
  credentials: CredentialInfo[];
  bindings: Record<string, string>;
  teams: TeamInfo[];
}
export interface RemoteEntry {
  name: string;
  path: string;
  isDir: boolean;
  isLink: boolean;
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
  services: { name: string; load: string; active: string; state: string; description: string }[];
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
    disks: { name: string; total: number; used: number; available: number; mount: string }[];
  };
}
