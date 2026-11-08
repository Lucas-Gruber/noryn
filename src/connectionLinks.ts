import { t as tr } from './i18n';

export interface ConnectionLink {
  protocol: 'ssh' | 'sftp' | 'ftp' | 'ftps' | 'rdp';
  host: string;
  port: number;
  username: string;
  initialPath: string;
}
export const linkSchemes = ['ssh', 'sftp', 'ftp', 'ftps', 'rdp'] as const;
export function defaultPort(protocol: string) {
  return protocol === 'rdp' ? 3389 : ['ftp', 'ftps'].includes(protocol) ? 21 : 22;
}
function decoded(value: string) {
  try {
    const text = decodeURIComponent(value);
    if (/[\u0000-\u001f\u007f]/u.test(text)) throw Error();
    return text;
  } catch {
    throw Error(tr('Ungültiger Verbindungslink.'));
  }
}
export function isConnectionLink(text: string) {
  return /^(ssh|sftp|ftp|ftps|rdp):\/\//i.test(text.trim());
}
export function parseConnectionLink(text: string): ConnectionLink {
  if (
    text.length > 8192 ||
    !isConnectionLink(text) ||
    /[\u0000-\u001f\u007f]/u.test(text) ||
    /\s/u.test(text.trim())
  )
    throw Error(tr('Ungültiger Verbindungslink.'));
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    throw Error(tr('Ungültiger Verbindungslink.'));
  }
  if (url.password) throw Error(tr('Passwörter bitte im Anmeldedialog eingeben, nicht im Link.'));
  const protocol = url.protocol.slice(0, -1).toLowerCase() as ConnectionLink['protocol'];
  const host = decoded(url.hostname)
    .replace(/^\[|\]$/g, '')
    .toLowerCase();
  const port = url.port ? Number(url.port) : defaultPort(protocol);
  const path = decoded(url.pathname);
  const username = decoded(url.username);
  if (
    !host ||
    /[\s/\\@?#%]/u.test(host) ||
    (host.includes(':') && !url.hostname.startsWith('[')) ||
    host.length > 253 ||
    !Number.isInteger(port) ||
    port < 1 ||
    port > 65535 ||
    url.search ||
    url.hash ||
    username.length > 512 ||
    path.length > 4096 ||
    (path && !path.startsWith('/')) ||
    (['ssh', 'rdp'].includes(protocol) && path && path !== '/')
  )
    throw Error(tr('Ungültiger Verbindungslink.'));
  return {
    protocol,
    host,
    port,
    username,
    initialPath: ['ssh', 'rdp'].includes(protocol) ? '' : path,
  };
}

/** URI lists from Dolphin/Nautilus and native local paths. No remote URL is a local filename. */
export function parseFileDrop(values: string[]): { local: string[]; remote: ConnectionLink[] } {
  const lines = values.filter((line) => line && !line.startsWith('#'));
  if (!lines.length || lines.length > 256 || lines.some((line) => line.length > 8192))
    throw Error(tr('Bitte höchstens 256 Dateien gleichzeitig ablegen.'));
  const local: string[] = [],
    remote: ConnectionLink[] = [];
  for (const line of lines) {
    if (/^file:\/\//i.test(line)) {
      let url: URL;
      try {
        url = new URL(line);
      } catch {
        throw Error(tr('Ungültiger Datei-Link.'));
      }
      if (
        (url.hostname && url.hostname !== 'localhost') ||
        url.search ||
        url.hash ||
        url.username ||
        url.password
      )
        throw Error(tr('Ungültiger Datei-Link.'));
      let path = decoded(url.pathname);
      if (/^\/[a-z]:\//i.test(path)) path = path.slice(1);
      local.push(path);
    } else if (isConnectionLink(line)) {
      const link = parseConnectionLink(line);
      if (
        !['sftp', 'ftp', 'ftps'].includes(link.protocol) ||
        !link.initialPath ||
        link.initialPath === '/'
      )
        throw Error(tr('Bitte Datei- oder Ordnerlinks für SFTP, FTP oder FTPS verwenden.'));
      remote.push(link);
    } else if (/^[a-z][\w+.-]*:\/\//i.test(line) || line.includes('\0')) {
      throw Error(tr('Ungültiger Datei-Link.'));
    } else if (/^(\/|[a-z]:[\\/]|\\\\)/i.test(line)) local.push(line);
    else throw Error(tr('Ungültiger Datei-Link.'));
  }
  return { local: [...new Set(local)], remote };
}
