# Noryn.

Noryn ist ein SSH-/SFTP-Client mit **Tauri 2, Vue 3, TypeScript und Rust** für Linux und Windows. Der Fokus liegt auf einem gemeinsamen Verbindungsbaum bei strikt persönlicher Authentifizierung.

## Implementiert

- Private und mehrere Team-Workspaces; ein zentraler Verbindungsbaum mit verschachtelten Ordnern, Kategorien/Tags, Notizen und Favoriten. Die Seitenleiste enthält nur Workspace-Navigation, ohne zweiten Baum.
- Einträge können im Baum per Drag & Drop in Ordner verschoben werden. Der Ordner kann auch im Editor geändert werden.
- Persönliche Zugangsdaten mit SSH-Schlüssel, Passwort oder lokalem SSH-Agent. Ein Konto kann Schlüssel und Benutzerpasswort gleichzeitig enthalten; die Schlüssel-Passphrase ist davon getrennt. Am Eintrag oder Ordner auswählen; Kinder erben die Auswahl, bis sie diese überschreiben. Bestehende Konten können ohne Offenlegen gespeicherter Geheimnisse bearbeitet werden.
- Lokal verschlüsselter Tresor: Argon2id (64 MiB, 3 Durchläufe) und XChaCha20-Poly1305. Geheimnisse werden nicht an die Oberfläche zurückgegeben.
- SSH-Hostschlüsselprüfung vor der Anmeldung; unbekannte und geänderte Schlüssel müssen ausdrücklich bestätigt werden.
- Interaktives xterm.js-Terminal mit PTY, Größenanpassung und mehreren Sitzungstabs. Keyboard-Interactive/OTP-Abfragen erscheinen beim Verbinden und werden nicht gespeichert. Terminal und Dateien teilen einen authentifizierten SSH-Transport, sodass kein zweiter OTP für den Dateikanal nötig ist.
- SSH-Verbindungen zeigen Terminal und Dateibrowser; SFTP-Verbindungen ausschließlich Dateien. Dateiansicht mit Zurück/Vorwärts, Pfadsegmenten, direkter Pfadeingabe (Strg L), Suche, versteckten Dateien und neuen Ordnern. Der SSH-Dateibrowser ist maximierbar.
- Lokaler und entfernter Dateibereich nebeneinander, bei SFTP standardmäßig aktiv und bei SSH per „Lokal“-Button. Dateien zwischen den Bereichen ziehen oder mit Transferbuttons übertragen. Betriebssystem-Datei-Drops und Dateiauswahl bleiben verfügbar. Warteschlange mit Fehleranzeige; Remote-Uploads überschreiben keine vorhandenen Dateien, lokale Downloads benötigen zum Ersetzen eine Bestätigung.
- Eigene Rechtsklickmenüs für Verbindungen, Ordner, Dateien und Terminal.
- Optionaler sudo-Dateimodus über SSH **ohne Python**. Benötigt sudo-Rechte, eine Shell und GNU find sowie übliche Dateiwerkzeuge (`cat`, `mkdir`, `wc`, `id`) auf dem Linux-Ziel. Zuerst wird passwortloses sudo versucht, danach das gespeicherte Benutzerpasswort und nur bei Bedarf eine zusätzliche Eingabe. Der Wechsel betrifft den Dateibrowser; im Terminal ist `sudo` separat nutzbar.
- Ende-zu-Ende verschlüsselter **manueller** Team-Sync mit einem eigenen Rust-Server, HTTPS-Prüfung und revisionsbasiertem Schutz gegen konkurrierende Uploads.

## Noch offen

RDP, VNC und FTP sind als Eintragstypen vorbereitet, haben aber **noch keine Verbindungsimplementierung**. Ebenfalls offen: automatischer Hintergrundsync, Zusammenführen von Konflikten, individuelle Server-Mitgliedschaften/Rollen, Schlüsselrotation, Jump Hosts, SSH-Config-Import, rekursive Ordnertransfers, Dateiumbenennung/-löschung, Transferfortsetzung und Passphrase-Wechsel des Tresors.

Der Linux-Build und ein echter OpenSSH-/SFTP-Durchlauf wurden lokal geprüft. Die Windows-Konfiguration und eine CI-Buildmatrix sind vorhanden; Windows wurde hier nicht ausgeführt. Dieses erste MVP hat kein unabhängiges Sicherheitsaudit.

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

Die Umbenennung behält absichtlich die bisherige App-ID `dev.harbor.client`, die Kryptografie-Kontexte und das Sync-Protokoll bei, damit vorhandene Tresore und Teams weiter nutzbar sind.

Die Desktop-App speichert `vault.json` in Tauris plattformspezifischem App-Datenverzeichnis für `dev.harbor.client`. Unter Linux ist dies üblicherweise `$XDG_DATA_HOME/dev.harbor.client` bzw. `~/.local/share/dev.harbor.client`. Eine Sicherung dieser Datei ist nur mit dem Tresor-Passwort wiederherstellbar. Es gibt keine Passwort-Wiederherstellung und momentan keinen automatischen Export.

## Erste Verbindung

1. In der Desktop-App einen Tresor mit mindestens 12 Zeichen Passwort erstellen.
2. Unter „Meine Zugangsdaten“ ein Konto anlegen. Bei einem SSH-Schlüssel kann zusätzlich das Benutzerpasswort für sudo gespeichert werden. Eine Schlüssel-Passphrase entschlüsselt nur den privaten Schlüssel.
3. Ordner und Verbindung erstellen. Zugangsdaten am Ordner oder am Eintrag auswählen.
4. „Verbinden“ wählen und den SSH-Fingerabdruck über einen unabhängigen Kanal prüfen. Bei OTP-/MFA-Abfragen die vom Server angeforderten Antworten eingeben.
5. Im SSH-Sitzungstab Terminal und Dateibrowser verwenden, bei SFTP nur den Dateibrowser. „Lokal“ öffnet die lokale Ansicht, das Maximieren-Symbol vergrößert bei SSH den Dateibrowser. Für Dateizugriff als root „sudo“ aktivieren.

## Team-Sync

Der Server benötigt ein eigenes Authentifizierungs-Token pro Team. Dieses Token und der zufällige 256-Bit-Team-Schlüssel sind **unterschiedliche Geheimnisse**. Der Server kennt den Team-Schlüssel nicht.

```sh
# Einen zufälligen Token z. B. mit „openssl rand -hex 32“ erzeugen.
HARBOR_SYNC_TEAMS='platform-team:REPLACE_WITH_A_RANDOM_TOKEN_OF_AT_LEAST_32_CHARACTERS' \
  cargo run -p harbor-sync
```

Der Server lauscht standardmäßig nur auf `127.0.0.1:8787` und speichert verschlüsselte Snapshots in `harbor-sync-data/`. Konfiguration: `HARBOR_SYNC_BIND`, `HARBOR_SYNC_DATA`, `HARBOR_SYNC_TEAMS`. Mehrere Teams werden als `team-id:token,anderes-team:token` angegeben. Nur eine Serverinstanz pro Datenverzeichnis betreiben.

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

- **Hochladen:** Teilt den aktuellen Baum. Ein abweichender Serverstand erzeugt einen Konflikt und wird nicht überschrieben.
- **Abrufen:** Ersetzt nach Bestätigung den lokalen Baum dieses Teams. Nicht hochgeladene Änderungen gehen verloren. Private Einträge und persönliche Zugangsdaten bleiben erhalten; Zuordnungen bleiben erhalten, solange die zugehörigen Eintrags-IDs existieren.
- HTTP wird ausschließlich für `localhost`, `127.0.0.1` und `[::1]` für lokale Tests akzeptiert. Externe Server erfordern gültiges HTTPS; Zertifikatsprüfung bleibt aktiv.

## Prüfen und bauen

```sh
pnpm build
cargo check --workspace
cargo test -p harbor-core -p harbor-sync
cargo fmt --all -- --check
pnpm format:check

# Linux: isolierter, kurzlebiger OpenSSH-Testserver nur auf localhost.
# Benötigt sshd, ssh-keygen, ssh-keyscan und sftp-server.
bash scripts/test-ssh.sh

# Echter verschlüsselter Client-/Server-Abgleich über localhost:
cargo test -p harbor-sync -- --ignored

# SSH-Passwort/Schlüssel + OTP, mehrere Abfragen und Abbruch:
node scripts/test-otp.mjs

# Native Installer auf dem jeweiligen Zielsystem erstellen:
pnpm tauri build
```

Das SSH-Testskript verwendet temporäre Schlüssel und verändert weder `~/.ssh` noch den System-SSH-Dienst. Es testet Hostschlüsselablehnung, Public-Key-Anmeldung, Terminal, SFTP, gemeinsamen SSH-Transport, Dateitransfers, Schutz vor Überschreiben und Ordnererstellung. Ein nur im privaten Testserver-PATH sichtbarer sudo-/id-Stub prüft NOPASSWD, gespeichertes Passwort, abgelehnte Passwörter und Moduswechsel **ohne tatsächliche Rechteerhöhung**. Echte privilegierte Rechte müssen weiterhin gegen ein entsprechend eingerichtetes Testziel geprüft werden.

Der OTP-Test startet einen echten lokalen SSH-Server aus der reinen JavaScript-Implementierung des Entwicklungsmoduls `ssh2`. Er prüft Public-Key + OTP, Passwort + OTP, mehrere gleichzeitige Abfragen, falschen Code und Abbruch. Native optionale Node-Addons werden dafür nicht gebaut.

Noryn aktiviert beim Linux-Start einen Kompatibilitätsmodus für die [bekannten WebKitGTK-/Wayland-Rendererprobleme](https://bugs.webkit.org/show_bug.cgi?id=280210): Wenn `DISPLAY` vorhanden und `GDK_BACKEND` nicht ausdrücklich gesetzt ist, wird X11/XWayland verwendet. Der problematische DMABUF-Renderer wird standardmäßig deaktiviert. Diese Einstellungen gelten nur für den Noryn-Prozess, greifen vor der GTK-Initialisierung und sind auch im gebauten Programm aktiv. Windows ist davon nicht betroffen. `pnpm tauri dev` benötigt deshalb keine zusätzlichen Umgebungsvariablen.

Eigene Werte für `GDK_BACKEND` und `WEBKIT_DISABLE_DMABUF_RENDERER` werden beibehalten. Um natives Wayland mit dem DMABUF-Renderer gezielt zu testen:

```sh
GDK_BACKEND=wayland WEBKIT_DISABLE_DMABUF_RENDERER=0 pnpm tauri dev
```

## Aufbau

```text
src/                 Vue-Oberfläche, xterm.js, Desktop-Bridge, Browser-Vorschau
src-tauri/           Tauri-Kommandos und Desktop-Sitzungsverwaltung
crates/core/         Datenmodell, Tresor, Kryptografie, SSH/SFTP, Sync-Client
server/              Kleiner Axum-Server für verschlüsselte Team-Snapshots
docs/architecture.md Daten- und Sicherheitsgrenzen, Protokoll und nächste Schritte
```

API-Grundlagen: [ssh2](https://docs.rs/ssh2/latest/ssh2/), [Tauri Dialog](https://v2.tauri.app/plugin/dialog/), [RustCrypto XChaCha20-Poly1305](https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/).
