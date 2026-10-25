import type { CredentialDraft } from './types';
export function emptyCredential(): CredentialDraft {
  return {
    id: '',
    name: '',
    username: '',
    kind: 'password',
    secret: '',
    password: '',
    passphrase: '',
    teamId: null,
  };
}
export function clearCredential(value: CredentialDraft) {
  value.secret = '';
  value.password = '';
  value.passphrase = '';
}
