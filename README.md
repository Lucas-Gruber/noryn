# Noryn.

Noryn ist ein SSH-/SFTP-/FTP-/FTPS-Client mit **Tauri 2, Vue 3, TypeScript und Rust** für Linux, Windows und macOS. Der Fokus liegt auf einem gemeinsamen Verbindungsbaum mit persönlicher Kontoauswahl und optional freigegebenen Team-Konten.

## Implementiert

- Private und mehrere Team-Workspaces; ein zentraler Verbindungsbaum mit verschachtelten Ordnern, Kategorien/Tags, Notizen und Favoriten. Die Seitenleiste enthält nur Workspace-Navigation, ohne zweiten Baum.
- Großer Workspace-Button mit eigenem Dropdown, aktiver Markierung und Tastatursteuerung. „Alle Workspaces“ zeigt Teams und private Verbindungen gemeinsam, mit getrennten Abschnitten im Baum. Neue Einträge erhalten einen ausdrücklichen Ziel-Workspace. Das Terminal übernimmt das gewählte Theme mit kräftigen ANSI-Textfarben.
- Einträge können im Baum per Drag & Drop in Ordner verschoben werden. Der Ordner kann auch im Editor geändert werden.
- Persönliche Zugangsdaten mit SSH-Schlüssel, Passwort oder lokalem SSH-Agent. Ein Konto kann Schlüssel und Benutzerpasswort gleichzeitig enthalten; die Schlüssel-Passphrase ist davon getrennt. Am Eintrag oder Ordner auswählen; Kinder erben die Auswahl, bis sie diese überschreiben. Bestehende Konten können ohne Offenlegen gespeicherter Geheimnisse bearbeitet werden.
- Optionaler Team-Tresor: einzelne Passwort-/Schlüsselkonten ausdrücklich per Häkchen für genau ein Team freigeben. Standardmäßig bleiben Konten privat; lokale Agenten sind nicht teilbar. Host-/Ordner-Zuordnungen bleiben immer persönlich.
- Lokal verschlüsselter Tresor: Argon2id (64 MiB, 3 Durchläufe) und XChaCha20-Poly1305. Geheimnisse werden nicht an die Oberfläche zurückgegeben.
- SSH-Hostschlüsselprüfung vor der Anmeldung; unbekannte und geänderte Schlüssel müssen ausdrücklich bestätigt werden.
- Interaktives xterm.js-Terminal mit PTY, Größenanpassung und mehreren Sitzungstabs. Keyboard-Interactive/OTP-Abfragen erscheinen beim Verbinden und werden nicht gespeichert. Terminal und Dateien teilen einen authentifizierten SSH-Transport, sodass kein zweiter OTP für den Dateikanal nötig ist.
- Lokale SSH-Portweiterleitung direkt im Sitzungstab, z. B. Server `127.0.0.1:5432` → lokal `127.0.0.1:5432`; freier lokaler Port mit `0`. Listener ausschließlich auf Loopback und an die Sitzung gebunden.
- Interaktive Prozesse (Details, SIGTERM/SIGKILL mit Startzeitprüfung) und systemd-Dienste (Status/Logs, Unit-Dateien, Autostart, Start/Stop/Restart/Reload). Aktionen verwenden bei Bedarf sudo mit gespeichertem Passwort; gestartete Aktionen werden nicht automatisch wiederholt.
- Hardwareansicht mit CPU-/RAM-/Last-/Datenträger-/Netzwerkgraphen. Live alle 5 Sekunden nur bei sichtbarer Hardwareansicht; Prozess-/Dienstintervalle sind optional. LVM-PVs/VGs/LVs und Mounts separat auf Wunsch laden, auch mit sudo. Kein Zielagent.
- Ziehbare Seitenleiste und Terminal-/Datei-Aufteilung mit Min-/Max-Werten, Tastaturbedienung und lokal gespeicherten Layout-Einstellungen. Kleine UI-Texte wurden für bessere Lesbarkeit vergrößert.
- SSH-Verbindungen zeigen Terminal und Dateibrowser; SFTP, FTP und FTPS ausschließlich Dateien. Bei diesen Dateiverbindungen ist ein optionaler absoluter Startordner konfigurierbar; ein nicht erreichbarer Startordner meldet beim Verbinden einen Fehler. Dateiansicht mit Zurück/Vorwärts, Pfadsegmenten, direkter Pfadeingabe (Strg L), Suche, versteckten Dateien und neuen Ordnern. Der SSH-Dateibrowser ist maximierbar.
- FTP und explizites FTPS (AUTH TLS, üblicherweise Port 21) mit Benutzername/Passwort, passiven Datenverbindungen, Binärtransfers und MLSD-Verzeichnislisten mit POSIX-/DOS-LIST-Fallback. FTPS prüft Serverzertifikat und Hostnamen und verschlüsselt Steuer- und Datenverbindung; es gibt keinen automatischen Rückfall auf unverschlüsseltes FTP. Klassisches FTP ist ausdrücklich als unverschlüsselt gekennzeichnet. sudo steht nur bei SSH/SFTP bereit.
- Lokaler und entfernter Dateibereich nebeneinander, bei SFTP/FTP/FTPS standardmäßig aktiv und bei SSH per „Lokal“-Button. Dateien oder ganze Ordner einschließlich leerer Unterordner zwischen den Bereichen ziehen oder mit Transferbuttons übertragen. Rekursive Transfers funktionieren auch über sudo und FTP/FTPS. Vorhandene Zielordner werden nicht zusammengeführt; Verknüpfungen und Spezialdateien werden ausgelassen und gemeldet. Betriebssystem-Datei-Drops und Dateiauswahl bleiben verfügbar. Warteschlange mit Fortschritt, Abbruch und Fehleranzeige; bereits übertragene Teile bleiben bei Abbruch erhalten. SFTP-Uploads erstellen Dateien exklusiv, FTP-Uploads prüfen vorhandene Namen und veröffentlichen erst nach erfolgreicher Übertragung. Lokale Downloads benötigen zum Ersetzen eine Bestätigung.
- Eigene Rechtsklickmenüs für Verbindungen, Ordner, Dateien und Terminal.
- Optionaler sudo-Dateimodus über SSH **ohne Python**. Benötigt sudo-Rechte, eine Shell und GNU find sowie übliche Dateiwerkzeuge (`cat`, `mkdir`, `wc`, `id`) auf dem Linux-Ziel. Zuerst wird passwortloses sudo versucht, danach das gespeicherte Benutzerpasswort und nur bei Bedarf eine zusätzliche Eingabe. Der Wechsel betrifft den Dateibrowser; im Terminal ist `sudo` separat nutzbar.
- Ende-zu-Ende verschlüsselter Team-Sync: manuell oder optional live, mit Server-Benachrichtigungen, feldweiser Zusammenführung und Schutz vor konkurrierenden Uploads.

## Darstellung

Unter **Einstellungen** gibt es die Themes **Graphit**, **Mitternacht**, **Nord**, **Carbon**, **Matrix** und **Licht** sowie die Basis-Textgrößen **XS (12 px), SM (13 px), MD (14 px), LG (16 px) und XL (18 px)**. Carbon bietet neutrales Dunkelgrau mit blauen Akzenten, Matrix tiefschwarze Flächen mit leuchtendem Grün und weiterhin unterscheidbaren, kräftigen ANSI-Farben im Terminal. Eingabefelder verwenden dieselbe Basis; Überschriften und kleinere Hinweise skalieren relativ dazu. Standard ist Graphit mit MD. Die Vorschau zeigt Formular- und Terminalschrift, Änderungen gelten sofort auch in bereits geöffneten Terminals.

**Inter Variable** und **JetBrains Mono Variable** sind als lokale Fontdateien mitgeliefert und funktionieren offline. Beide verwenden die SIL Open Font License; die Lizenztexte liegen unter `public/licenses/` und werden mitgebaut. Darstellungseinstellungen werden auf diesem Gerät gespeichert und weder an Teams übertragen noch im Sync-Verlauf aufgezeichnet. „Zurücksetzen“ stellt Graphit und MD wieder her.

## Noch offen

RDP und VNC sind als Eintragstypen vorbereitet, haben aber **noch keine Verbindungsimplementierung**. Ebenfalls offen: interaktive Auswahl widersprüchlicher Feldwerte, individuelle Server-Mitgliedschaften/Rollen, Schlüsselrotation, Jump Hosts, SSH-Config-Import, Dateiumbenennung/-löschung, Transferfortsetzung, implizites FTPS (Port 990), aktive FTP-Datenverbindungen, alte FTP-Zeichensätze und Passphrase-Wechsel des Tresors.

Der Linux-Build und echte OpenSSH-/SFTP-/FTP-/FTPS-Durchläufe wurden lokal geprüft. Die GitHub-Release-Matrix für Windows und macOS ist vorbereitet; diese beiden Plattformen wurden hier nicht ausgeführt. Dieses erste MVP hat kein unabhängiges Sicherheitsaudit.

## Starten

Voraussetzungen: Rust stable, Node.js 22+, pnpm ab 10.26 (oder npm) sowie die [Tauri-Systemabhängigkeiten](https://v2.tauri.app/start/prerequisites/). Das Projekt verwendet die installierte pnpm-Version und erzwingt keinen automatischen Versionswechsel. Für Windows werden MSVC Build Tools und WebView2 benötigt. Das vendorte OpenSSL benötigt außerdem Perl und unter Windows eine passende Build-Umgebung.

```sh
pnpm install
pnpm tauri dev
```

Alternativ:

```sh
npm install
npm run tauri dev
```

Reine Browser-Vorschau:

```sh
pnpm dev
```

Die Browser-Vorschau unter `http://127.0.0.1:1420` enthält **flüchtige Beispieldaten**. Sie baut keine echten Verbindungen auf und speichert keine Geheimnisse. Die Desktop-App startet mit einem leeren, passwortgeschützten Tresor.

App-ID, Speicherpfade, Rust-Pakete, Umgebungsvariablen und neue Verschlüsselungskontexte verwenden Noryn. Ein vorhandener Tresor wird beim ersten erfolgreichen Entsperren verschlüsselt in den neuen Pfad kopiert; die alte Datei bleibt als Sicherung unverändert. Bestehende Team-Snapshots bleiben lesbar. Nur das Migrationsmodul kennt die alten Bezeichner.

Die Desktop-App speichert `vault.json` in Tauris plattformspezifischem App-Datenverzeichnis für `dev.noryn.client`. Unter Linux ist dies üblicherweise `$XDG_DATA_HOME/dev.noryn.client` bzw. `~/.local/share/dev.noryn.client`. Der vollständige Pfad ist in der Hilfe sichtbar. Windows: `%APPDATA%/dev.noryn.client/vault.json`; macOS: `~/Library/Application Support/dev.noryn.client/vault.json`. Eine Sicherung dieser Datei ist nur mit dem Tresor-Passwort wiederherstellbar. Es gibt keine Passwort-Wiederherstellung.

## Erste Verbindung

1. In der Desktop-App einen Tresor mit mindestens 12 Zeichen Passwort erstellen.
2. Unter „Meine Zugangsdaten“ ein Konto anlegen. Bei einem SSH-Schlüssel kann zusätzlich das Benutzerpasswort für sudo gespeichert werden. Eine Schlüssel-Passphrase entschlüsselt nur den privaten Schlüssel.
3. Ordner und Verbindung erstellen. Zugangsdaten am Ordner oder am Eintrag auswählen. SFTP/FTP/FTPS können einen Startordner wie `/foldery/folderx` erhalten; diese Angabe wird als Verbindungsdetail mit dem Team synchronisiert.
4. „Verbinden“ wählen und den SSH-Fingerabdruck über einen unabhängigen Kanal prüfen. Bei OTP-/MFA-Abfragen die vom Server angeforderten Antworten eingeben.
5. Im SSH-Sitzungstab Terminal und Dateibrowser verwenden, bei SFTP nur den Dateibrowser. „Lokal“ öffnet die lokale Ansicht, das Maximieren-Symbol vergrößert bei SSH den Dateibrowser. Für Dateizugriff als root „sudo“ aktivieren.

Über **Schnell verbinden** startest du eine als temporär markierte Sitzung ohne gespeicherten Eintrag. „Im privaten Tresor speichern“ übernimmt sie später mit einem Klick, während sie weiterläuft. Private Verbindungen können außerdem eigene Zugangsdaten enthalten, einschließlich Schlüssel und separatem Benutzerpasswort. Diese direkten Anmeldungen werden nie mit Teams geteilt oder in Team-Bäume kopiert; vollständige/private Sicherungen enthalten sie (neues Backup-Format, aktueller Client erforderlich).

Textdateien öffnest du im Dateibrowser per Doppelklick, Enter oder **Rechtsklick → Im Standardprogramm öffnen / bearbeiten**. Erst im externen Editor speichern, dann unter **Extern geöffnet → Hochladen** (remote) bzw. **Speichern** (lokal) übernehmen. Noryn verwendet geschützte Arbeitskopien und prüft das Original auf zwischenzeitliche Änderungen. Unterstützt werden reguläre UTF-8-/UTF-16-Textdateien bis 8 MiB; ausführbare Skript-/Starter-Endungen werden als `.txt` geöffnet. Die Kopien werden beim Schließen der Sitzung oder Sperren entfernt; Änderungen daher vorher übernehmen. Gleichzeitige Änderungen unmittelbar beim finalen Rename können weiterhin kollidieren. Details zu Voraussetzungen und Grenzen stehen in [der Architektur](docs/architecture.md#textdateien-im-standardprogramm-bearbeiten).

## Export und Import

Unter **Einstellungen → Tresore sichern & übertragen** lassen sich verschlüsselte `.noryn`-Dateien exportieren und importieren. Jede Sicherung hat ein eigenes Passwort mit mindestens 12 Zeichen, unabhängig vom lokalen Tresor-Passwort. Ohne dieses Passwort ist die Sicherung nicht wiederherstellbar.

- **Alle Tresore:** vollständige Sicherung einschließlich privater und Team-Zugangsdaten, persönlicher Zuordnungen/Ablage, Hostschlüssel sowie Team-Schlüssel und Server-Tokens.
- **Privater Tresor:** nur private Einträge und Konten mit persönlicher Ablage. Zuordnungen zu Konten anderer Tresore sind nicht enthalten.
- **Ein Team:** Team-Baum, ausdrücklich freigegebene Konten, Verlauf und Team-Konfiguration einschließlich Schlüssel/Server-Token. Keine private Kontoauswahl, persönlichen Konten, Ablageordner oder eigenen Anzeigenamen.

Der Import zeigt zuerst Bereich und Anzahl der Einträge/Konten. Als Ziel kannst du den ursprünglichen Bereich wiederherstellen (ersetzen) oder Daten in einen vorhandenen privaten bzw. Team-Tresor hinzufügen. Beim Hinzufügen werden neue IDs vergeben; bestehende Zieldaten bleiben erhalten. Bei vollständigen Archiven wählst du zusätzlich den Quell-Tresor. Konten werden dabei nur mit ausdrücklichem Häkchen übernommen und bei einem Team-Ziel freigegeben; lokale SSH-Agent-Zugänge werden bei Team-Zielen ausgelassen. Persönliche Kontoauswahl, Ablage und Quell-Team-Schlüssel werden beim Hinzufügen nicht kopiert. Zuordnungen zu entfernten Konten werden bereinigt. Der aktuelle Tresor-Schlüssel und sein Passwort bleiben bestehen. Bestehende persönliche Team-Namen/Kontoauswahlen bleiben bei Team-Importen erhalten; für neu importierte Teams den eigenen Namen angeben. Team-Importe werden protokolliert; Live-Sync wird für importierte Teams ausgeschaltet. Importierte Daten bleiben bis zum bewussten Abgleich lokal. Der interne Sync-Cache ist nie Teil einer Exportdatei. Vor dem Import alle Verbindungen schließen. Die Vorschau ist fünf Minuten gültig und wird bei zwischenzeitlichen Tresoränderungen abgelehnt. Vorhandene Exportdateien werden nicht überschrieben. SSH-Agent-Konten verweisen auf den Agenten des Zielgeräts; Agent-Schlüssel werden nicht exportiert. Darstellung/Layout werden separat auf dem Gerät gespeichert und sind kein Bestandteil der Sicherung.

## Team-Sync

Unter **Team konfigurieren → Live-Sync für dieses Team aktivieren** lässt sich der Abgleich auf jedem Gerät separat automatisieren. Dafür Desktop-App und Server aktualisieren. Speicherungen werden 600 ms gebündelt; der Server meldet neue Revisionen über eine authentifizierte SSE-Verbindung. Im Leerlauf gibt es keine Snapshot-Abfragen, lediglich ein kleines Lebenszeichen alle 30 Sekunden. Bei Verbindungsfehlern wird mit steigender Pause erneut verbunden. Nach Wiederverbindung wird die aktuelle Revision geprüft; offline gespeicherte Änderungen bleiben lokal erhalten.

Über Rechtsklick im Verbindungsbaum kannst du einzelne Verbindungen, ganze Ordner mit Untereinträgen oder einen vollständigen Workspace in einen anderen Workspace/Zielordner kopieren: privat → Team, Team → privat und Team → Team. Quellen bleiben erhalten. Nur die Baumdaten werden kopiert; deine Kontoauswahl richtest du am Ziel persönlich ein. Beim Team-Ziel weist der Dialog auf die Freigabe von Verbindungsdaten und Notizen hin.

Unter **Team-Sync → Papierkorb am Team** lässt sich ein Team aus diesem Gerät entfernen, einschließlich dessen lokaler Einträge und geteilter Konten. Private Konten bleiben erhalten; der Server und andere Mitglieder bleiben unverändert. Vorher Sitzungen schließen und noch benötigte lokale Änderungen exportieren.

Die letzte bestätigte Version dient als gemeinsame Basis: Änderungen an verschiedenen Einträgen oder unterschiedlichen Feldern werden zusammengeführt. Abweichende Werte desselben Felds, Löschen gegen Bearbeiten oder widersprüchliche Ordnerstrukturen halten den automatischen Abgleich an. Unter Team-Sync erscheinen betroffene Einträge/Feldnamen; keine Secret-Werte. Lokale Daten dann exportieren, bevor du bewusst den Serverstand übernimmst und Änderungen erneut einträgst. Eine interaktive Auswahl einzelner Konfliktwerte ist noch nicht implementiert. Ohne gemeinsame Erstversion und mit beidseitigen Inhalten verlangt der Client einen bewussten ersten Abgleich. Namen und Geräteuhren bestimmen niemals, wessen Wert gewinnt.

Live-Sync ist standardmäßig aus, läuft nur bei entsperrtem Tresor und benötigt weder geöffnete SSH-Sitzungen noch zusätzliche SSH-Abfragen. Persönliche Kontoauswahl, private Konten und Ablageänderungen lösen keine Team-Uploads aus. Übertragen wird weiterhin ein verschlüsselter vollständiger Team-Snapshot je Änderungsserie; es werden keine einzelnen geheimen Feldwerte unverschlüsselt an den Server gesendet.

Der Server benötigt ein eigenes Authentifizierungs-Token pro Team. Dieses Token und der zufällige 256-Bit-Team-Schlüssel sind **unterschiedliche Geheimnisse**. Der Server kennt den Team-Schlüssel nicht.

```sh
# Einen zufälligen Token z. B. mit „openssl rand -hex 32“ erzeugen.
NORYN_SYNC_TEAMS='platform-team:REPLACE_WITH_A_RANDOM_TOKEN_OF_AT_LEAST_32_CHARACTERS' \
  cargo run -p noryn-sync
```

Der Server lauscht standardmäßig nur auf `127.0.0.1:8787` und speichert verschlüsselte Snapshots in `noryn-sync-data/`. Konfiguration: `NORYN_SYNC_BIND`, `NORYN_SYNC_DATA`, `NORYN_SYNC_TEAMS`. Mehrere Teams werden als `team-id:token,anderes-team:token` angegeben. Nur eine Serverinstanz pro Datenverzeichnis betreiben.

Für Docker gibt es eine kommentierte [`compose.yaml`](compose.yaml) mit [`.env.example`](.env.example). Einrichtung und Erklärung aller Variablen stehen in [Docker Compose](docs/releases.md#docker-compose).

Für externen Zugriff einen HTTPS-Reverse-Proxy vorschalten, etwa mit Caddy:

```caddyfile
sync.example.com {
  request_body {
    max_size 32MB
  }
  reverse_proxy 127.0.0.1:8787
}
```

In „Team-Sync“ auf jedem Gerät dieselbe Team-ID, Server-URL, Server-Token und denselben Team-Schlüssel hinterlegen. Einen Schlüssel im ersten Client erzeugen, **vor dem Speichern kopieren** und über einen sicheren Kanal weitergeben. Die Team-ID ist unveränderlich.

- **Hochladen:** Teilt den aktuellen Baum und ausschließlich für dieses Team freigegebene Konten. Ein abweichender Serverstand erzeugt einen Konflikt und wird nicht überschrieben.
- **Abrufen:** Ersetzt nach Bestätigung den lokalen Baum dieses Teams. Nicht hochgeladene Änderungen gehen verloren. Private Einträge und Privatkonten bleiben erhalten. Team-Konten werden mit dem Snapshot ersetzt; persönliche Zuordnungen bleiben erhalten, solange Eintrag und Konto noch existieren.
- HTTP wird ausschließlich für `localhost`, `127.0.0.1` und `[::1]` für lokale Tests akzeptiert. Externe Server erfordern gültiges HTTPS; Zertifikatsprüfung bleibt aktiv.

Konten werden unter „Meine Zugangsdaten“ über das Freigabe-Häkchen und die Team-Auswahl geteilt. Neue Konten innerhalb eines ausgewählten Team-Tresors sind als Team-Konto vorbelegt. Beim Zurückstellen auf privat wird das Konto nach dem nächsten Upload aus dem Team-Snapshot entfernt; bereits erhaltene Geheimnisse können nicht rückwirkend entzogen werden. Snapshot-Version 3 erfordert auf allen Team-Geräten einen aktuellen Client; ältere Snapshots bleiben lesbar.

Vor dem Erstellen oder Bearbeiten gemeinsamer Einträge trägt jedes Mitglied unter „Team-Sync → Team konfigurieren“ den eigenen Anzeigenamen ein. Bei bestehenden Teams zeigt „Deinen Namen ergänzen“ den fehlenden Namen an. Unter „Änderungsverlauf“ stehen Person, Zeitpunkt, Aktion, betroffener Eintrag und geänderte Felder. Die letzten 1.000 Änderungen werden Ende-zu-Ende verschlüsselt geteilt; Namen und Zeitstempel stammen vom jeweiligen Client. Passwort-, Schlüssel- und Notizinhalte werden nicht im Verlauf gespeichert. Persönliche Kontoauswahl und persönliche Zugangsdaten-Ordner werden nicht protokolliert. Andere Mitglieder sehen Änderungen nach Hochladen und Abrufen.

## Zugangsdaten organisieren

Unter „Meine Zugangsdaten“ organisieren persönliche Ordner und Unterordner die Konten. Rechtsklick bzw. das Aktionsmenü bietet Verschieben und Ordnerverwaltung. Ordner bleiben verschlüsselt im lokalen Tresor; sie beeinflussen weder Anmeldung, Vererbung, Freigaben noch die Kontoauswahl an Verbindungen. Beim Entfernen eines Ordners bleiben Konten und Unterordner erhalten.

## Prüfen und bauen

```sh
pnpm build
cargo check --workspace
cargo test -p noryn-core -p noryn-sync
cargo fmt --all -- --check
pnpm format:check

# Linux: isolierter, kurzlebiger OpenSSH-Testserver nur auf localhost.
# Benötigt sshd, ssh-keygen, ssh-keyscan und sftp-server.
bash scripts/test-ssh.sh

# Echter verschlüsselter Client-/Server-Abgleich über localhost:
cargo test -p noryn-sync -- --ignored

# SSH-Passwort/Schlüssel + OTP, mehrere Abfragen und Abbruch:
node scripts/test-otp.mjs

# FTP/FTPS: einmalig lokale Testabhängigkeiten in einer virtuellen Umgebung installieren:
python3 -m venv .venv-ftp
.venv-ftp/bin/python -m pip install -r scripts/requirements-ftp.txt
.venv-ftp/bin/python scripts/test-ftp.py
# Windows: .venv-ftp/Scripts/python.exe statt .venv-ftp/bin/python verwenden.

# Native Installer auf dem jeweiligen Zielsystem erstellen:
pnpm tauri build
```

Die Controller-Prüfungen `node scripts/test-quick-connect.mjs` und `node scripts/test-text-edit.mjs` prüfen flüchtige Anmeldungen, bewusstes Speichern, Hostschlüssel-/Sperrpfade sowie externe Arbeitskopien mit isolierter IPC-Simulation. Dabei werden keine echten Zugangsdaten oder externen Programme verwendet.

Das SSH-Testskript verwendet temporäre Schlüssel und verändert weder `~/.ssh` noch den System-SSH-Dienst. Es testet Hostschlüsselablehnung, Public-Key-Anmeldung, Terminal, SFTP, gemeinsamen SSH-Transport, Dateitransfers, Schutz vor Überschreiben und Ordnererstellung. Ein nur im privaten Testserver-PATH sichtbarer sudo-/id-Stub prüft NOPASSWD, gespeichertes Passwort, abgelehnte Passwörter und Moduswechsel **ohne tatsächliche Rechteerhöhung**. Echte privilegierte Rechte müssen weiterhin gegen ein entsprechend eingerichtetes Testziel geprüft werden.

Der OTP-Test startet einen echten lokalen SSH-Server aus der reinen JavaScript-Implementierung des Entwicklungsmoduls `ssh2`. Er prüft Public-Key + OTP, Passwort + OTP, mehrere gleichzeitige Abfragen, falschen Code und Abbruch. Native optionale Node-Addons werden dafür nicht gebaut.

Der FTP-Test startet kurzlebige lokale Server für MLSD, POSIX-LIST, DOS-LIST und FTPS mit einer temporären Test-CA. Geprüft werden binäre Transfers, Unicode-Dateinamen, vorhandene Dateien, fehlerhafte Anmeldung, nicht erreichbare Pfade, passive NAT-Adressen sowie Zertifikats- und Hostnamenfehler. Python und die Testmodule werden ausschließlich lokal für diese Tests benötigt; auf FTP- oder SSH-Zielen wird nichts installiert.

## Tresor-Passwort vergessen

Am Sperrbildschirm **„Passwort vergessen? Tresor zurücksetzen“** wählen; alternativ **Einstellungen → Lokaler Tresor → Sperren & zurücksetzen**. Neues Passwort (mindestens zwölf Zeichen) zweimal eingeben und `ZURÜCKSETZEN` bestätigen. Dadurch wird der lokale Tresor dieser Installation durch einen neuen, leeren Tresor ersetzt und entsperrt. Ein unbekanntes Passwort lässt sich nicht unter Erhalt der verschlüsselten Daten zurücksetzen.

**Alle lokalen Verbindungen, Zugangsdaten und Team-Konfigurationen inklusive Team-Schlüsseln gehen dabei verloren.** Der Team-Server, separat exportierte `.noryn`-Backups und frühere Migrationssicherungen bleiben erhalten. Ein eigenes Backup kann danach mit seinem separaten Passwort importiert werden. Teams müssen erneut mit Server-Token und Team-Schlüssel eingerichtet werden. Vorher Verbindungen/Abgleiche beenden und andere Noryn-Instanzen schließen. Der Dialog zeigt die Folgen vor dem Ersetzen ausdrücklich an.

## App-Updates

Unter **Einstellungen → Updates** prüft Noryn auf Wunsch beim Start und bei erneuter Aktivierung nach mindestens sechs Stunden auf neue öffentliche GitHub Releases. Ein dezenter Hinweis öffnet Versionsinformationen und Download-Fortschritt. Download und Installation erfolgen erst auf Klick; vor dem Neustart werden offene Verbindungen berücksichtigt und der Tresor gesperrt. Signaturen prüfen Paketbytes und Versionsnummer. AppImage, macOS-App und installierte Windows-Versionen unterstützen direkte Updates. Portable Windows-EXE und Systempakete bieten den manuellen Release-Download an. Bestehende Apps ohne Updater müssen einmal manuell aktualisiert werden.

Für die Veröffentlichung einmalig die beiden Signier-Secrets einrichten und öffentliche Release-Downloads sicherstellen: [Update-Einrichtung](docs/releases.md#app-updates-einrichten). Das Updatepaket und `latest.json` entstehen danach automatisch in der Release-Pipeline. Browser-Vorschau und Debug-Builds installieren keine Updates.

## GitHub Actions

Die Pipeline liegt in [`.github/workflows/build.yml`](.github/workflows/build.yml). Nur ein Versions-Tag wie `0.1.0` oder `v0.1.0` auf dem aktuellen Commit startet Builds und Tests; normale Pushes und Pull Requests lösen keinen Workflow aus. Die Version lässt sich mit `pnpm release:version patch` (oder `minor`, `major`, genauer Nummer) gemeinsam in allen Manifesten und `Cargo.lock` setzen; Python 3.11+ wird dafür benötigt. Die Paketversionen müssen zum anschließend erstellten Tag passen. Nach erfolgreichen Prüfungen erstellt die Pipeline selbst das GitHub Release und veröffentlicht das Server-Image unter `ghcr.io/<owner>/<repo>:0.1.0` und alle Desktop-Pakete im Release **des ausführenden GitHub-Repositories**. Portable Anwendungen, Installer und Prüfsummen stehen einzeln bereit, auch als Actions-Downloads; nur die macOS-`.app` wird als ZIP verpackt. Details zu Berechtigungen, Downloads und Containerbetrieb: [docs/releases.md](docs/releases.md).

Noryn aktiviert beim Linux-Start einen Kompatibilitätsmodus für die [bekannten WebKitGTK-/Wayland-Rendererprobleme](https://bugs.webkit.org/show_bug.cgi?id=280210): Wenn `DISPLAY` vorhanden und `GDK_BACKEND` nicht ausdrücklich gesetzt ist, wird X11/XWayland verwendet. Der problematische DMABUF-Renderer wird standardmäßig deaktiviert. Diese Einstellungen gelten nur für den Noryn-Prozess, greifen vor der GTK-Initialisierung und sind auch im gebauten Programm aktiv. Windows ist davon nicht betroffen. `pnpm tauri dev` benötigt deshalb keine zusätzlichen Umgebungsvariablen.

Eigene Werte für `GDK_BACKEND` und `WEBKIT_DISABLE_DMABUF_RENDERER` werden beibehalten. Um natives Wayland mit dem DMABUF-Renderer gezielt zu testen:

```sh
GDK_BACKEND=wayland WEBKIT_DISABLE_DMABUF_RENDERER=0 pnpm tauri dev
```

## Aufbau

```text
src/                 Vue-Oberfläche, xterm.js, Desktop-Bridge, Browser-Vorschau
src-tauri/           Tauri-Kommandos und Desktop-Sitzungsverwaltung
crates/core/         Datenmodell, Tresor, Kryptografie, SSH/SFTP, FTP/FTPS, Sync-Client
server/              Kleiner Axum-Server für verschlüsselte Team-Snapshots
docs/architecture.md Daten- und Sicherheitsgrenzen, Protokoll und nächste Schritte
```

API-Grundlagen: [ssh2](https://docs.rs/ssh2/latest/ssh2/), [Tauri Dialog](https://v2.tauri.app/plugin/dialog/), [RustCrypto XChaCha20-Poly1305](https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/).

## Projektwissen und Codex-Aufgaben

[AGENTS.md](AGENTS.md) enthält die verbindlichen Architektur- und Arbeitsregeln für neue Codex-Aufgaben. [docs/codex-tasks.md](docs/codex-tasks.md) erklärt Cloud-Setup und einen geeigneten Auftrag zum Bearbeiten von Issues. Dies ersetzt die Abhängigkeit von einem einzelnen Chatverlauf.
