export LC_ALL=C SYSTEMD_COLORS=0 SYSTEMD_PAGER=cat
[ "$(uname -s)" = Linux ] || { echo 'Der Login-Bericht unterstützt derzeit Linux.' >&2; exit 1; }
now=$(date +%s)
uptime=$(cut -d ' ' -f 1 /proc/uptime)
cores=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)
printf 'NORYN_HEALTH\t%s\t%s\t%s\n' "$now" "$uptime" "$cores"
since=$1
boot=$((now - ${uptime%%.*}))
if [ "$since" -eq 0 ] || [ "$since" -gt "$now" ]; then since=$boot; fi
printf 'NORYN_SINCE\t%s\n' "$since"
if ! command -v timeout >/dev/null 2>&1; then
 printf 'NORYN_NOTE\tFür zeitlich begrenzte Zusatzabfragen wird GNU timeout benötigt.\n'
 exit 0
fi
printf 'NORYN_SECTION\tservices\n'
if units=$(timeout 2 systemctl --no-pager --plain --no-legend list-units --type=service --all 2>/dev/null); then
 printf '%s\n' "$units" | head -n 400
 printf 'NORYN_SERVICES_OK\n'
else printf 'NORYN_NOTE\tDienststatus nicht verfügbar (systemd fehlt oder Zugriff verweigert).\n'; fi
printf 'NORYN_SECTION\tjournal\n'
# systemd UNIT_FAILED, UNIT_PROCESS_EXIT and UNIT_OUT_OF_MEMORY message IDs.
if journal=$(timeout 2 journalctl --since="@$since" --until="@$now" --no-pager -o json -n 100 MESSAGE_ID=be02cf6855d2428ba40df7e9d022f03d MESSAGE_ID=98e322203f7a4ed290d09fe03c09fe15 MESSAGE_ID=fe6faa94e7774663a0da52717891d8ef 2>/dev/null); then
 printf '%s\n' "$journal"
 printf 'NORYN_JOURNAL_OK\n'
else printf 'NORYN_NOTE\tDiensthistorie nicht lesbar; Journal-Rechte oder gespeicherte Logs fehlen.\n'; fi
# Journal output is always limited to the current user's permissions.
printf 'NORYN_NOTE\tDiensthistorie: höchstens 100 Ereignisse aus dem für diesen Benutzer lesbaren Journal; fehlende Ereignisse sind kein Nachweis eines fehlerfreien Betriebs.\n'
printf 'NORYN_SECTION\tupdates\n'
if command -v apt-get >/dev/null 2>&1; then
 sources=$(timeout 1 apt-cache policy 2>/dev/null)
 if printf '%s\n' "$sources" | grep -q ' Packages$' && updates=$(timeout 4 apt-get -s -o Debug::NoLocking=1 upgrade 2>/dev/null); then
  printf '%s\n' "$updates" | awk '/^Inst / {total++; if (tolower($0) ~ /security/) security++} END {printf "NORYN_UPDATES\tapt\t%d\t%d\n", total, security}'
  printf 'NORYN_NOTE\tAPT: Security-Zahl zählt angebotene Pakete mit Security-Repository-Kennung. Nur vorhandene Paketlisten, kein apt update.\n'
 fi
elif command -v dnf >/dev/null 2>&1; then
 if updates=$(timeout 3 dnf -q --cacheonly repoquery --upgrades --qf '%{name}.%{arch}' 2>/dev/null); then
  total=$(printf '%s\n' "$updates" | sed '/^$/d' | sort -u | wc -l)
  security=unknown
  if security_updates=$(timeout 3 dnf -q --cacheonly repoquery --upgrades --security --qf '%{name}.%{arch}' 2>/dev/null); then
   security=$(printf '%s\n' "$security_updates" | sed '/^$/d' | sort -u | wc -l)
  fi
  printf 'NORYN_UPDATES\tdnf\t%s\t%s\n' "$total" "$security"
 fi
elif command -v pacman >/dev/null 2>&1; then
 updates=$(timeout 2 pacman -Qu 2>/dev/null); result=$?
 if [ "$result" -le 1 ]; then
  total=$(printf '%s\n' "$updates" | sed '/^$/d' | wc -l)
  printf 'NORYN_UPDATES\tpacman\t%s\tunknown\n' "$total"
 fi
elif command -v apk >/dev/null 2>&1; then
 if updates=$(timeout 2 apk version -l '<' 2>/dev/null); then
  total=$(printf '%s\n' "$updates" | sed '/^$/d' | wc -l)
  printf 'NORYN_UPDATES\tapk\t%s\tunknown\n' "$total"
 fi
fi
printf 'NORYN_SECTION\tload\n'
# Existing sysstat recordings only. Exclude the first five minutes of this boot.
window=$since
[ "$window" -ge "$((boot + 300))" ] || window=$((boot + 300))
[ "$window" -ge "$((now - 604800))" ] || window=$((now - 604800))
printf 'NORYN_LOAD_WINDOW\t%s\n' "$window"
if command -v sadf >/dev/null 2>&1; then
 for offset in 0 1 2 3 4 5 6; do
  day=$(date -d "$offset days ago" +%d 2>/dev/null) || break
  for directory in /var/log/sysstat /var/log/sa; do
   file="$directory/sa$day"
   [ -r "$file" ] || continue
   timeout 1 sadf -p -U -s 00:00:00 -e 23:59:59 "$file" -- -q 2>/dev/null | awk -F '\t' -v start="$window" -v end="$now" -v cores="$cores" '
    $5 == "ldavg-1" && $3 >= start && $3 <= end {count++; if ($6 > cores) {recent[peaks % 100] = sprintf("NORYN_LOAD\t%s\t%s", $3, $6); peaks++}}
    END {first = peaks > 100 ? peaks - 100 : 0; for (i=first; i<peaks; i++) print recent[i % 100]; printf "NORYN_LOAD_SAMPLES\t%d\n", count}'
   break
  done
 done
fi
printf 'NORYN_SECTION\tports\n'
if ports=$(timeout 2 ss -H -lntup 2>/dev/null); then
 printf '%s\n' "$ports" | head -n 512
 printf 'NORYN_PORTS_OK\n'
else printf 'NORYN_NOTE\tListener nicht verfügbar (ss fehlt oder Zugriff verweigert).\n'; fi
