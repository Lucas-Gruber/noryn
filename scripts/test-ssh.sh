#!/usr/bin/env bash
set -euo pipefail
# Uses a temporary unprivileged sshd bound only to localhost, never the user's SSH config.
task_test_dir=$(mktemp -d /tmp/noryn-ssh-test.XXXXXX)
task_sshd_pid=''
task_sshd=$(command -v sshd || true)
if [[ -z "$task_sshd" && -x /usr/sbin/sshd ]]; then task_sshd=/usr/sbin/sshd; fi
if [[ -z "$task_sshd" ]]; then echo 'OpenSSH sshd executable not found'; exit 1; fi
cleanup() {
  if [[ -n "$task_sshd_pid" ]]; then kill "$task_sshd_pid" 2>/dev/null || true; wait "$task_sshd_pid" 2>/dev/null || true; fi
  # Keep test keys and logs private, then remove only this script's temporary directory.
  rm -rf -- "$task_test_dir"
}
trap cleanup EXIT
ssh-keygen -q -t ed25519 -N '' -f "$task_test_dir/host"
ssh-keygen -q -t ed25519 -N '' -f "$task_test_dir/client"
task_test_user=$(id -un)
task_test_port=${NORYN_TEST_SSH_PORT:-22229}
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
  if [ "${NORYN_TEST_ROOT:-}" = 1 ]; then printf '0\n'; else printf '12345\n'; fi
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
  password|password-rs)
    if [ "$interactive" != yes ]; then
      if [ "$mode" = password-rs ]; then
        printf '%s\n' 'sudo: interactive authentication is required' >&2
      else
        printf '%s\n' 'sudo test: a password is required' >&2
      fi
      exit 1
    fi
    printf '%s' "$prompt" >&2
    IFS= read -r password
    if [ "$password" != 'noryn-test-pass' ]; then
      printf '%s\n' 'Sorry, try again.' >&2
      printf '%s' "$prompt" >&2
      IFS= read -r password || true
      exit 1
    fi
    ;;
  nopasswd) ;;
  *) exit 1 ;;
esac
NORYN_TEST_ROOT=1
export NORYN_TEST_ROOT
exec "$@"
SCRIPT
# No real service or LVM tools are used by the management integration tests.
cat > "$task_test_dir/bin/systemctl" <<'SCRIPT'
#!/bin/sh
set -eu
verb=''
for arg in "$@"; do
 case "$arg" in show|status|cat|list-units|list-unit-files|start|stop|restart|reload|enable|disable) verb=$arg ;; esac
done
printf '%s\n' "$verb" >> "$(dirname "$0")/../service-calls"
case "$verb" in
 show) printf 'Version=257\nId=noryn-mock.service\nActiveState=active\nUnitFileState=enabled\nMainPID=1234\n' ;;
 status) printf 'noryn-mock.service - mock service\nActive: active (running)\n' ;;
 cat) printf '# /etc/systemd/system/noryn-mock.service\n[Service]\nExecStart=/bin/true\n' ;;
 list-units) printf 'noryn-mock.service loaded active running Mock service\n' ;;
 list-unit-files) printf 'noryn-mock.service enabled enabled\nnoryn-inactive.service disabled enabled\n' ;;
 start|stop|restart|reload|enable|disable)
  case "$*" in *noryn-mock.service*) ;; *) exit 1 ;; esac
  if [ -f "$(dirname "$0")/../fail-service" ]; then printf 'mock action failed\n' >&2; exit 1; fi
  ;;
 *) exit 1 ;;
esac
SCRIPT
for task_lvm_tool in pvs vgs lvs; do
 cat > "$task_test_dir/bin/$task_lvm_tool" <<'SCRIPT'
#!/bin/sh
case "$(basename "$0")" in
 pvs) printf '{"report":[{"pv":[{"pv_name":"/dev/mock","pv_size":"1048576","pv_free":"0","vg_name":"mock-vg"}]}]}' ;;
 vgs) printf '{"report":[{"vg":[{"vg_name":"mock-vg","vg_size":"1048576","vg_free":"0","pv_count":"1","lv_count":"1"}]}]}' ;;
 lvs) printf '{"report":[{"lv":[{"lv_name":"mock-lv","vg_name":"mock-vg","lv_path":"/dev/mock-vg/mock-lv","lv_size":"1048576","lv_attr":"-wi-ao----"}]}]}' ;;
esac
SCRIPT
 chmod 700 "$task_test_dir/bin/$task_lvm_tool"
done
chmod 700 "$task_test_dir/bin/systemctl"
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
"$task_sshd" -D -e -f "$task_test_dir/sshd_config" > "$task_test_dir/sshd.log" 2>&1 &
task_sshd_pid=$!
for task_attempt in $(seq 1 50); do
  if ssh-keyscan -p "$task_test_port" 127.0.0.1 > /dev/null 2>&1; then break; fi
  if ! kill -0 "$task_sshd_pid" 2>/dev/null; then cat "$task_test_dir/sshd.log"; exit 1; fi
  sleep 0.1
done
NORYN_TEST_SSH_PORT="$task_test_port" NORYN_TEST_SSH_USER="$task_test_user" NORYN_TEST_SSH_KEY="$task_test_dir/client" NORYN_TEST_SUDO_CONTROL="$task_test_dir/sudo-mode" cargo test -p noryn-core --test ssh_integration -- --ignored --nocapture --test-threads=1
