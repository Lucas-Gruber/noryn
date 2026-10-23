#!/usr/bin/env bash
set -euo pipefail
# Uses a temporary unprivileged sshd bound only to localhost, never the user's SSH config.
task_test_dir=$(mktemp -d /tmp/harbor-ssh-test.XXXXXX)
task_sshd_pid=''
cleanup() {
  if [[ -n "$task_sshd_pid" ]]; then kill "$task_sshd_pid" 2>/dev/null || true; wait "$task_sshd_pid" 2>/dev/null || true; fi
  # Keep test keys and logs private, then remove only this script's temporary directory.
  rm -rf -- "$task_test_dir"
}
trap cleanup EXIT
ssh-keygen -q -t ed25519 -N '' -f "$task_test_dir/host"
ssh-keygen -q -t ed25519 -N '' -f "$task_test_dir/client"
task_test_user=$(id -un)
task_test_port=${HARBOR_TEST_SSH_PORT:-22229}
task_sftp_server=''
for task_candidate in /usr/lib/ssh/sftp-server /usr/lib/openssh/sftp-server /usr/libexec/openssh/sftp-server; do
  if [[ -x "$task_candidate" ]]; then task_sftp_server=$task_candidate; break; fi
done
if [[ -z "$task_sftp_server" ]]; then echo 'OpenSSH sftp-server executable not found'; exit 1; fi
# Exercise the sudo wire protocol without changing sudoers or elevating this
# test server. These shims exist only in the isolated server's private PATH.
mkdir "$task_test_dir/bin"
printf 'nopasswd\n' > "$task_test_dir/sudo-mode"
cat > "$task_test_dir/bin/id" <<'SCRIPT'
#!/bin/sh
if [ "$#" -eq 1 ] && [ "$1" = '-u' ]; then
  printf '0\n'
else
  exec /usr/bin/id "$@"
fi
SCRIPT
cat > "$task_test_dir/bin/sudo" <<'SCRIPT'
#!/bin/sh
set -eu
interactive=no
prompt=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    -n) shift ;;
    -S) interactive=yes; shift ;;
    -p) prompt=$2; shift 2 ;;
    --) shift; break ;;
    *) printf '%s\n' 'unexpected sudo test argument' >&2; exit 1 ;;
  esac
done
mode=$(cat "$(dirname "$0")/../sudo-mode")
case "$mode" in
  deny) printf '%s\n' 'sudo test: access denied' >&2; exit 1 ;;
  password)
    if [ "$interactive" != yes ]; then
      printf '%s\n' 'sudo test: a password is required' >&2; exit 1
    fi
    printf '%s' "$prompt" >&2
    IFS= read -r password
    if [ "$password" != 'harbor-test-pass' ]; then
      printf '%s\n' 'Sorry, try again.' >&2
      printf '%s' "$prompt" >&2
      IFS= read -r password || true
      exit 1
    fi
    ;;
  nopasswd) ;;
  *) exit 1 ;;
esac
exec "$@"
SCRIPT
chmod 700 "$task_test_dir/bin/id" "$task_test_dir/bin/sudo"
cat > "$task_test_dir/session.sh" <<'SCRIPT'
PATH="$(dirname "$0")/bin:/usr/bin:/bin"
export PATH
if [ -z "${SSH_ORIGINAL_COMMAND:-}" ]; then
  exec /bin/sh -i
else
  exec /bin/sh -c "$SSH_ORIGINAL_COMMAND"
fi
SCRIPT
cat > "$task_test_dir/sshd_config" <<EOF
ListenAddress 127.0.0.1
Port $task_test_port
HostKey $task_test_dir/host
PidFile $task_test_dir/sshd.pid
AuthorizedKeysFile $task_test_dir/client.pub
StrictModes no
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
AllowUsers $task_test_user
Subsystem sftp $task_sftp_server
ForceCommand /bin/sh $task_test_dir/session.sh
LogLevel ERROR
EOF
/usr/bin/sshd -D -e -f "$task_test_dir/sshd_config" > "$task_test_dir/sshd.log" 2>&1 &
task_sshd_pid=$!
for task_attempt in $(seq 1 50); do
  if ssh-keyscan -p "$task_test_port" 127.0.0.1 > /dev/null 2>&1; then break; fi
  if ! kill -0 "$task_sshd_pid" 2>/dev/null; then cat "$task_test_dir/sshd.log"; exit 1; fi
  sleep 0.1
done
HARBOR_TEST_SSH_PORT="$task_test_port" HARBOR_TEST_SSH_USER="$task_test_user" HARBOR_TEST_SSH_KEY="$task_test_dir/client" HARBOR_TEST_SUDO_CONTROL="$task_test_dir/sudo-mode" cargo test -p harbor-core --test ssh_integration -- --ignored --nocapture --test-threads=1
