# Harbor

Ein SSH-/SFTP-Client mit **Tauri 2, Vue 3, TypeScript und Rust** für Linux und Windows. Der Fokus liegt auf einem gemeinsamen Verbindungsbaum bei strikt persönlicher Authentifizierung.

## Implementiert

- Private und mehrere Team-Workspaces; beliebig verschachtelte Ordner, Verbindungen, Kategorien/Tags, Notizen und Favoriten.
- Baum und Verbindungskacheln können per Drag & Drop in Ordner verschoben werden. Der Ordner kann auch im Editor geändert werden.
- Persönliche Zugangsdaten für Passwort, SSH-Schlüssel oder lokalen SSH-Agent. Am Eintrag oder Ordner auswählen; Kinder erben die Auswahl, bis sie diese überschreiben.
- Lokal verschlüsselter Tresor: Argon2id (64 MiB, 3 Durchläufe) und XChaCha20-Poly1305. Geheimnisse werden nicht an die Oberfläche zurückgegeben.
- SSH-Hostschlüsselprüfung vor der Anmeldung; unbekannte und geänderte Schlüssel müssen ausdrücklich bestätigt werden.
- Interaktives xterm.js-Terminal mit PTY, Größenanpassung und mehreren Sitzungstabs.
- SFTP über SSH: Verzeichnisse, versteckte Dateien, Upload per Dateiauswahl oder Betriebssystem-Drag-&-Drop, Download und neue Ordner. Vorhandene Remote-Dateien werden beim Upload nicht still überschrieben.
- Optionaler sudo-Dateimodus über SSH; benötigt `sudo` und Python 3 auf dem Linux-Ziel. Unterstützt sudo-Passwort oder NOPASSWD. Der Wechsel betrifft den Dateibrowser; im Terminal ist `sudo` separat nutzbar.
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

Die Desktop-App speichert `vault.json` in Tauris plattformspezifischem App-Datenverzeichnis für `dev.harbor.client`. Unter Linux ist dies üblicherweise `$XDG_DATA_HOME/dev.harbor.client` bzw. `~/.local/share/dev.harbor.client`. Eine Sicherung dieser Datei ist nur mit dem Tresor-Passwort wiederherstellbar. Es gibt keine Passwort-Wiederherstellung und momentan keinen automatischen Export.

## Erste Verbindung

1. In der Desktop-App einen Tresor mit mindestens 12 Zeichen Passwort erstellen.
2. Unter „Meine Zugangsdaten“ ein Konto anlegen. Private Schlüssel werden im verschlüsselten Tresor gespeichert; eine Schlüssel-Passphrase ist optional.
3. Ordner und Verbindung erstellen. Zugangsdaten am Ordner oder am Eintrag auswählen.
4. „Verbinden“ wählen und den SSH-Fingerabdruck über einen unabhängigen Kanal prüfen.
5. Im Sitzungstab Terminal und Dateibrowser verwenden. Für Dateizugriff als root „sudo-Modus“ aktivieren.

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

# Native Installer auf dem jeweiligen Zielsystem erstellen:
pnpm tauri build
```

Das SSH-Testskript verwendet temporäre Schlüssel und verändert weder `~/.ssh` noch den System-SSH-Dienst. Es testet Hostschlüsselablehnung, Public-Key-Anmeldung, Terminal, SFTP-Auflistung, Dateitransfers, Schutz vor Überschreiben und Ordnererstellung. Es erteilt keine sudo-Rechte. Der privilegierte sudo-Dateimodus muss gegen ein entsprechend eingerichtetes Testziel geprüft werden.

Harbor aktiviert beim Linux-Start einen Kompatibilitätsmodus für die [bekannten WebKitGTK-/Wayland-Rendererprobleme](https://bugs.webkit.org/show_bug.cgi?id=280210): Wenn `DISPLAY` vorhanden und `GDK_BACKEND` nicht ausdrücklich gesetzt ist, wird X11/XWayland verwendet. Der problematische DMABUF-Renderer wird standardmäßig deaktiviert. Diese Einstellungen gelten nur für den Harbor-Prozess, greifen vor der GTK-Initialisierung und sind auch im gebauten Programm aktiv. Windows ist davon nicht betroffen. `pnpm tauri dev` benötigt deshalb keine zusätzlichen Umgebungsvariablen.

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
