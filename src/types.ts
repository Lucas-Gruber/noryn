export interface NodeEntry {
  id: string;
  parentId: string | null;
  workspace: string;
  kind: 'folder' | 'connection';
  name: string;
  protocol: 'ssh' | 'sftp' | 'rdp' | 'vnc' | 'ftp';
  host: string;
  port: number;
  notes: string;
  tags: string[];
  favorite: boolean;
}
export interface CredentialInfo {
  id: string;
  name: string;
  username: string;
  kind: 'password' | 'key' | 'agent';
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
  port: number;
  connected: boolean;
  preview: boolean;
  buffer: Uint8Array[];
  sink?: (data: Uint8Array) => void;
}
export type TerminalEvent = { type: 'data'; data: string } | { type: 'closed'; message: string };
