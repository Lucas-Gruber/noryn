# Setup und Betrieb

[Zurück zu Noryn](../README.md)

## Desktop-App

Installiere Rust stable, Node.js ab 22.13, pnpm 11 und die
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

### Tresordatei

Noryn speichert `vault.json` in Tauris App-Datenverzeichnis unter `dev.noryn.client`.
Unter Linux ist das üblicherweise `$XDG_DATA_HOME/dev.noryn.client` oder
`~/.local/share/dev.noryn.client`. Eine gesicherte Tresordatei lässt sich mit dem
Tresorpasswort wiederherstellen. Beim Entsperren kann Noryn einen vorhandenen
Harbor-Tresor aus dem Verzeichnis `dev.harbor.client` übernehmen.

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
einen HTTPS-Reverse-Proxy, zum Beispiel Caddy.

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
   um ihn zu übernehmen.

Beim Abrufen ersetzt Noryn nach Bestätigung den lokalen Team-Baum und die freigegebenen
Team-Konten. Lokale, noch nicht hochgeladene Team-Änderungen gehen dabei verloren.
Beim Hochladen signalisiert eine abweichende Serverrevision einen Konflikt.

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

| Kommando                                | Abdeckung                                                              |
| --------------------------------------- | ---------------------------------------------------------------------- |
| `pnpm test:ssh`                         | OpenSSH, Hostschlüssel, Terminal, SFTP und sudo-Simulation unter Linux |
| `pnpm test:otp`                         | Passwort oder Schlüssel mit Keyboard-Interactive/OTP                   |
| `pnpm test:ftp`                         | FTP/FTPS, Transfers, Verzeichnislisten und TLS-Prüfung                 |
| `cargo test -p noryn-sync -- --ignored` | Client-/Server-Abgleich über Loopback                                  |

Für den SSH-Test werden `sshd`, `ssh-keygen`, `ssh-keyscan` und `sftp-server` benötigt.
Für FTP/FTPS installiere die Python-Testabhängigkeiten aus
[`scripts/requirements-ftp.txt`](../scripts/requirements-ftp.txt) in einer lokalen virtuellen
Umgebung und führe den Test mit deren Python aus.

Die [Gitea-Pipeline](../.gitea/workflows/build.yml) definiert Builds für Linux und Windows.
Sie setzt aktivierte Repository-Actions und Runner mit den Labels `ubuntu-latest` und
`windows-latest` voraus. Der Windows-Runner braucht Git Bash, MSVC Build Tools und Perl.

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
