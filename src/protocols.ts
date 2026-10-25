import { t as tr } from './i18n';
export function isFileProtocol(protocol: string) {
  return ['sftp', 'ftp', 'ftps'].includes(protocol);
}
export function isFtp(protocol: string) {
  return protocol === 'ftp' || protocol === 'ftps';
}
export function transportLabel(protocol: string) {
  return protocol === 'ftp'
    ? tr('FTP · unverschlüsselter Transport')
    : protocol === 'ftps'
      ? tr('FTPS · TLS-verschlüsselt · Zertifikat geprüft')
      : tr('SSH-Hostschlüssel geprüft · verschlüsselter Transport');
}
