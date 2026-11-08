import { t as tr } from './i18n';
export function isFileProtocol(protocol: string) {
  return ['sftp', 'ftp', 'ftps'].includes(protocol);
}
export function isFtp(protocol: string) {
  return protocol === 'ftp' || protocol === 'ftps';
}
export function protocolIcon(protocol: string) {
  return isFileProtocol(protocol)
    ? 'Files'
    : ['rdp', 'vnc'].includes(protocol)
      ? 'Monitor'
      : 'Terminal';
}
export function transportLabel(protocol: string) {
  if (protocol === 'rdp') return tr('RDP · TLS-verschlüsselt · NLA-Anmeldung');
  return protocol === 'ftp'
    ? tr('FTP · unverschlüsselter Transport')
    : protocol === 'ftps'
      ? tr('FTPS · TLS-verschlüsselt · Zertifikat geprüft')
      : tr('SSH-Hostschlüssel geprüft · verschlüsselter Transport');
}
