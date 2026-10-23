import { generateKeyPairSync } from 'node:crypto';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import ssh2 from 'ssh2';

// A real localhost SSH server with deterministic second-factor challenges.
// No PAM files, user accounts, sudoers entries or remote hosts are changed.
const directory = await mkdtemp(join(tmpdir(), 'noryn-otp-'));
const hostKey = generateKeyPairSync('rsa', { modulusLength: 2048 }).privateKey.export({
  type: 'pkcs1',
  format: 'pem',
});
const clientKey = generateKeyPairSync('rsa', { modulusLength: 2048 });
const privateKey = clientKey.privateKey.export({ type: 'pkcs1', format: 'pem' });
const allowed = ssh2.utils.parseKey(privateKey);
await writeFile(join(directory, 'client.key'), privateKey, { mode: 0o600 });
const clients = new Set();
const server = new ssh2.Server({ hostKeys: [hostKey] }, (client) => {
  clients.add(client);
  let firstFactor = false;
  client.on('close', () => clients.delete(client));
  client.on('error', () => {});
  client.on('authentication', (context) => {
    const mode = context.username;
    const methods =
      firstFactor || ['multi-user', 'cancel-user', 'bad-user'].includes(mode)
        ? ['keyboard-interactive']
        : mode === 'key-user'
          ? ['publickey']
          : ['password'];
    if (mode === 'key-user' && context.method === 'publickey') {
      if (
        !context.key.data.equals(allowed.getPublicSSH()) ||
        (context.signature &&
          allowed.verify(context.blob, context.signature, context.hashAlgo) !== true)
      )
        return context.reject();
      if (!context.signature) return context.accept();
      firstFactor = true;
      return context.reject(['keyboard-interactive'], true);
    }
    if (mode === 'password-user' && context.method === 'password') {
      if (context.password !== 'stored-login-password') return context.reject(methods);
      firstFactor = true;
      return context.reject(['keyboard-interactive'], true);
    }
    if (context.method === 'keyboard-interactive' && methods.includes('keyboard-interactive')) {
      const prompts =
        mode === 'multi-user'
          ? [
              { prompt: 'Password: ', echo: false },
              { prompt: 'Verification code: ', echo: false },
            ]
          : [{ prompt: 'Verification code: ', echo: false }];
      return context.prompt(
        prompts,
        'Two-factor authentication',
        'Enter the current authenticator code.',
        (answers) => {
          const valid =
            mode === 'multi-user'
              ? answers[0] === 'interactive-password' && answers[1] === '123456'
              : answers[0] === '123456';
          if (valid) context.accept();
          else context.reject(['keyboard-interactive']);
        },
      );
    }
    context.reject(methods, firstFactor);
  });
});

try {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const child = spawn(
    'cargo',
    ['test', '-p', 'harbor-core', '--test', 'otp_integration', '--', '--ignored', '--nocapture'],
    {
      stdio: 'inherit',
      env: {
        ...process.env,
        NORYN_TEST_OTP_PORT: String(server.address().port),
        NORYN_TEST_OTP_KEY: join(directory, 'client.key'),
      },
    },
  );
  const status = await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code) => resolve(code ?? 1));
  });
  process.exitCode = status;
} finally {
  for (const client of clients) client.end();
  await new Promise((resolve) => server.close(resolve));
  await rm(directory, { recursive: true, force: true });
}
