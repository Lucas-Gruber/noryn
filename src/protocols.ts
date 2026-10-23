export function isFileProtocol(protocol: string) {
  return ['sftp', 'ftp', 'ftps'].includes(protocol);
}
export function isFtp(protocol: string) {
  return protocol === 'ftp' || protocol === 'ftps';
}
export function transportLabel(protocol: string) {
  return protocol === 'ftp'
    ? 'FTP · unverschlüsselter Transport'
    : protocol === 'ftps'
      ? 'FTPS · TLS-verschlüsselt · Zertifikat geprüft'
      : 'SSH-Hostschlüssel geprüft · verschlüsselter Transport';
}
