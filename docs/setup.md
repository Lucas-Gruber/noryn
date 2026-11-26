# Setup und Betrieb

[Zurück zu Noryn](../README.md)

## Desktop-App

Installiere Rust stable, Node.js ab 22.13, pnpm ab 10.26 und die
[Tauri-Systemabhängigkeiten](https://v2.tauri.app/start/prerequisites/).
Für Windows werden die MSVC Build Tools und WebView2 benötigt.
Der Build von OpenSSL benötigt Perl.

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

Der Desktop-Tresor benötigt bei der Einrichtung ein Passwort mit mindestens 12 Zeichen.
Ein SSH-Konto kann Schlüssel und Benutzerpasswort gleichzeitig enthalten. Das Benutzerpasswort
kann Noryn für sudo verwenden. Die Schlüssel-Passphrase dient zur Entschlüsselung des Schlüssels.

Ein SSH-Tab enthält Terminal und Dateibrowser. **Lokal** öffnet den lokalen Dateibereich.
Bei SFTP, FTP und FTPS erscheint er standardmäßig. Für diese Protokolle lässt sich ein
absoluter Startordner wie `/srv/files` setzen. In Team-Workspaces wird der Startordner als
Verbindungsdetail synchronisiert.

Über **Schnell verbinden** kannst du eine temporäre Sitzung starten und sie später im privaten
Tresor speichern. Details stehen unter [Schnellverbindungen](architecture.md#direkte-private-anmeldungen-und-schnellverbindungen).
Die [Architekturdokumentation](architecture.md) beschreibt auch [Dateiaktionen](architecture.md#dateiaktionen-und-persönliche-programmzuordnungen),
[Dateirechte](architecture.md#dateirechte-und-eigentümer), den [Befehls-Store](architecture.md#befehls-store)
und die [Team-Skript-Sammlung](architecture.md#team-skript-sammlung).

### Tresordatei

Noryn speichert `vault.json` in Tauris App-Datenverzeichnis unter `dev.noryn.client`.
Unter Linux ist das üblicherweise `$XDG_DATA_HOME/dev.noryn.client` oder
`~/.local/share/dev.noryn.client`, unter Windows `%APPDATA%/dev.noryn.client` und unter macOS
`~/Library/Application Support/dev.noryn.client`. Eine gesicherte Tresordatei lässt sich mit dem
Tresorpasswort wiederherstellen. Beim Entsperren kann Noryn einen vorhandenen
Harbor-Tresor aus dem Verzeichnis `dev.harbor.client` übernehmen.

Unter **Einstellungen → Tresore sichern & übertragen** kannst du verschlüsselte `.noryn`-Dateien
mit einem eigenen Passwort exportieren oder importieren. Die [Backup-Dokumentation](architecture.md#verschlüsselte-sicherungen)
beschreibt die verfügbaren Bereiche und Importoptionen. Wenn du das Tresorpasswort vergessen hast,
kannst du den lokalen Tresor [zurücksetzen](architecture.md#lokalen-tresor-zurücksetzen). Dabei wird
er durch einen leeren Tresor ersetzt.

### App-Updates

Unter **Einstellungen → Updates** kannst du nach neuen Versionen suchen. AppImage, die macOS-App
und die installierte Windows-Version unterstützen direkte Updates. Flatpak nutzt seine signierte
Paketquelle. Für die portable Windows-App und Systempakete gibt es den manuellen Release-Download.
Die [Release-Dokumentation](releases.md#downloads-im-github-release) beschreibt die Pakete und ihre Installation.

## Team-Sync

Der Server braucht pro Team ein eigenes Server-Token. Das Token authentifiziert den
Zugriff auf den Server. Der Team-Schlüssel dient der Verschlüsselung im Client.

Erzeuge mit `openssl rand -hex 32` ein Token und bewahre es für die Konfiguration der Clients auf.
Starte den Server in Bash und gib das Token an der Eingabeaufforderung ein.

```bash
read -rs -p 'Server-Token: ' noryn_sync_token
printf '\n'
NORYN_SYNC_TEAMS="platform-team:$noryn_sync_token" \
  cargo run --locked -p noryn-sync
```

| Variable           | Bedeutung                                | Standard           |
| ------------------ | ---------------------------------------- | ------------------ |
| `NORYN_SYNC_TEAMS` | Kommagetrennte Liste aus `team-id:token` | Pflichtangabe      |
| `NORYN_SYNC_BIND`  | Adresse und Port                         | `127.0.0.1:8787`   |
| `NORYN_SYNC_DATA`  | Verzeichnis für verschlüsselte Snapshots | `noryn-sync-data/` |

Betreibe eine Serverinstanz pro Datenverzeichnis. Für externen Zugriff braucht der Server
einen HTTPS-Reverse-Proxy, zum Beispiel Caddy. Für Docker steht eine
[Compose-Datei mit eigener Anleitung](releases.md#docker-compose) bereit.

```caddyfile
sync.example.com {
  request_body {
    max_size 32MB
  }
  reverse_proxy 127.0.0.1:8787
}
```

1. Öffne in der Desktop-App **Team-Sync** und wähle **Team hinzufügen**.
2. Hinterlege auf allen Geräten dieselbe Team-ID, URL und dasselbe Server-Token.
3. Erzeuge im ersten Client einen Team-Schlüssel, kopiere ihn vor dem Speichern und teile ihn
   über einen sicheren Kanal. Verwende auf den anderen Geräten den vorhandenen Schlüssel.
4. Wähle **Änderungen hochladen**, um den Team-Stand zu teilen, oder **Vom Server abrufen**,
   um ihn zu übernehmen. Trage deinen Anzeigenamen in der Team-Konfiguration ein, bevor du
   gemeinsame Einträge erstellst oder bearbeitest.

Beim Abrufen ersetzt Noryn nach Bestätigung den lokalen Team-Baum und die freigegebenen
Team-Konten. Lokale, noch nicht hochgeladene Team-Änderungen gehen dabei verloren.
Beim Hochladen signalisiert eine abweichende Serverrevision einen Konflikt.

Der [optionale Live-Sync](architecture.md#ereignisgesteuerter-live-sync) führt unabhängige Änderungen
zusammen und hält bei Konflikten an. Er läuft nur bei entsperrtem Tresor.

Für lokale Tests erlaubt Noryn HTTP auf `localhost`, `127.0.0.1` und `[::1]`.
Details zu Snapshots und Datenfreigaben stehen in der
[Architekturdokumentation](architecture.md).

## Bauen und prüfen

```sh
pnpm build
pnpm format:check
cargo check --workspace --locked
cargo test -p noryn-core -p noryn-sync --locked
cargo fmt --all -- --check

# Native Pakete auf dem jeweiligen Zielsystem erstellen
pnpm tauri build
```

Weitere Integrationstests laufen gegen kurzlebige lokale Server.

| Kommando                                   | Abdeckung                                                              |
| ------------------------------------------ | ---------------------------------------------------------------------- |
| `pnpm test:ssh`                            | OpenSSH, Hostschlüssel, Terminal, SFTP und sudo-Simulation unter Linux |
| `pnpm test:otp`                            | Passwort oder Schlüssel mit Keyboard-Interactive/OTP                   |
| `pnpm test:ftp`                            | FTP/FTPS, Transfers, Verzeichnislisten und TLS-Prüfung                 |
| `.venv-ftp/bin/python scripts/test-rdp.py` | RDP mit TLS/NLA, Anmeldung, Bildausgabe und Sitzungsende               |
| `cargo test -p noryn-sync -- --ignored`    | Client-/Server-Abgleich über Loopback                                  |

Für den SSH-Test werden `sshd`, `ssh-keygen`, `ssh-keyscan` und `sftp-server` benötigt.
Für FTP/FTPS und RDP installiere die Python-Testabhängigkeiten aus
[`scripts/requirements-ftp.txt`](../scripts/requirements-ftp.txt) in einer lokalen virtuellen
Umgebung und führe die Tests mit deren Python aus. Der RDP-Test baut zusätzlich einen isolierten
Rust-Testserver.

Die [GitHub-Pipeline](../.github/workflows/build.yml) baut Linux-, Windows- und macOS-Pakete
bei einem Versions-Tag. Details zu Versionierung und Veröffentlichung stehen in der
[Release-Dokumentation](releases.md).

Unter Linux startet Noryn bei verfügbarem `DISPLAY` standardmäßig mit X11/XWayland und
deaktiviert den WebKitGTK-DMABUF-Renderer. Eigene Werte für `GDK_BACKEND` und
`WEBKIT_DISABLE_DMABUF_RENDERER` haben Vorrang. Details zum
[WebKitGTK-Rendererproblem](https://bugs.webkit.org/show_bug.cgi?id=280210).

## Quellcode

| Pfad                              | Inhalt                                            |
| --------------------------------- | ------------------------------------------------- |
| [`src/`](../src/)                 | Vue-Oberfläche, Terminal und Browser-Vorschau     |
| [`src-tauri/`](../src-tauri/)     | Tauri-Kommandos und Desktop-Sitzungsverwaltung    |
| [`crates/core/`](../crates/core/) | Tresor, Protokolle, SSH-Werkzeuge und Sync-Client |
| [`server/`](../server/)           | Axum-Server für verschlüsselte Team-Snapshots     |

[AGENTS.md](../AGENTS.md) enthält die Projektvorgaben. [Codex-Aufgaben](codex-tasks.md) beschreibt
das Cloud-Setup und den Ablauf für Issue-Fixes.
