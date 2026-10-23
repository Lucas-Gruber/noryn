# GitHub-Builds, Releases und GHCR

`.github/workflows/build.yml` prüft Frontend/Rust, OTP, FTP/FTPS, verschlüsselten Sync und Paketierung. Linux prüft zusätzlich OpenSSH/SFTP/sudo, Weiterleitungen und Systemverwaltung. Branches und Pull Requests erzeugen Actions-Artefakte. Nur ein Push eines Versionstags `vX.Y.Z` (optional `-rc.1`) veröffentlicht Images und Releases. Alle Paketversionen in `package.json`, `src-tauri/tauri.conf.json`, Cargo-Paketen und `Cargo.lock` vor dem Tag gemeinsam erhöhen. Das Staging-Skript prüft den Tag gegen die Frontendversion.

## Einrichtung nach dem Umzug zu GitHub

1. Den aktuellen Repository-Stand einschließlich `.github/workflows` zu GitHub übertragen.
2. GitHub Actions im Repository zulassen. Der Workflow verwendet gehostete Ubuntu-, Windows- und macOS-Runner; zusätzliche eigene Runner sind nicht nötig. Für private Repositories gelten die Minuten-/Nutzungsgrenzen des GitHub-Tarifs.
3. Die Job-Berechtigungen `contents: write` für Releases und `packages: write` für den Image-Job zulassen. Repository-/Organisationsrichtlinien dürfen diese Rechte nicht einschränken. Ein zusätzlicher Registry-Login oder PAT ist normalerweise nicht nötig: verwendet wird das automatische `GITHUB_TOKEN`.
4. Versionsnummern committen und einen passenden Versionstag pushen.

Das GHCR-Paket heißt `ghcr.io/<owner>/<repo>` (alles kleingeschrieben). Der Container ist der Sync-Server. Der Workflow setzt die OCI-Quell-URL, Revision und Version und veröffentlicht mit dem Token desselben Repositories; damit wird das Paket mit dem Repository verknüpft. Tags: `X.Y.Z`, für stabile Releases zusätzlich `latest`; Vorabversionen ändern `latest` nicht. Ein neues GHCR-Paket ist standardmäßig privat. Für anonymes `docker pull` dessen Sichtbarkeit in den Package-Einstellungen auf öffentlich stellen; andernfalls vor dem Pull bei GHCR anmelden.

Veröffentlichungsjobs laufen nach erfolgreichen Desktop-Builds ausschließlich bei Versions-Tag-Pushes. Pull-Request-Jobs bekommen keine Schreib- oder Registry-Zugangsdaten als Umgebungsvariablen. Image-/Release-Publikation wurde hier nicht ausgeführt; der lokale Git-Remote wird nicht automatisch geändert.

## Downloads im GitHub Release

| System  | Portable                                            | Installer                              |
| ------- | --------------------------------------------------- | -------------------------------------- |
| Windows | `Noryn-X.Y.Z-windows-x64-portable.exe`              | `Noryn-X.Y.Z-windows-x64-setup.exe`    |
| macOS   | `Noryn-X.Y.Z-macos-arm64-portable.app.zip` bzw. x64 | `Noryn-X.Y.Z-macos-arm64.dmg` bzw. x64 |
| Linux   | `Noryn-X.Y.Z-linux-x64-portable.AppImage`           | `Noryn-X.Y.Z-linux-x64.deb`            |

Jede Datei erhält eine SHA-256-Prüfsumme. Das macOS-Paket enthält die Architektur des tatsächlichen Runners, kein Universal-Binary. Die App aus dem ZIP ist direkt nutzbar; das DMG bietet die übliche Installation nach Applications. AppImages bei Bedarf ausführbar machen (`chmod +x`). „Portable“ bedeutet ohne Installation ausführbar; der Tresor bleibt im normalen Benutzer-Datenverzeichnis.

Die portable Windows-EXE benötigt eine vorhandene WebView2-Runtime; der NSIS-Installer enthält den WebView2-Offline-Installer. Windows-Signierung und Apple-Signierung/Notarisierung sind noch nicht eingerichtet. Windows und macOS wurden lokal nicht ausgeführt; sie werden von der GitHub-Matrix geprüft.

Linux baut auf Ubuntu 22.04 für eine ältere glibc-Basis. `APPIMAGE_EXTRACT_AND_RUN=1` vermeidet FUSE-Mounts während der Paketierung. Der native Linux-Build, AppImage und DEB sowie das Server-Container-Image wurden lokal erstellt und geprüft.

Der Publisher erstellt erst einen Entwurf und veröffentlicht nach vollständigem Upload. Wiederholungen überschreiben keine Dateien: identische Assets werden übersprungen, unterschiedliche Bytes bei gleichem Namen brechen ab. Für neue Builds daher neue Versions-Tags verwenden.

```sh
# Nach Versionsänderung, Commit und Push des Codes:
git tag v0.1.0
git push origin v0.1.0
```

## Containerbetrieb

`Dockerfile` baut den Server in mehreren Stufen. Die Laufzeit enthält CA-Zertifikate, läuft als UID/GID 10001 und schreibt verschlüsselte Snapshots nach `/var/lib/noryn`. Team-Schlüssel gehören nur in Clients; der Server kennt lediglich Team-IDs und Server-Tokens.

```sh
# noryn-server.env außerhalb des Repositories anlegen, chmod 600:
# NORYN_SYNC_TEAMS=team-id:EIN_ZUFAELLIGER_TOKEN_MIT_MINDESTENS_32_ZEICHEN
docker run -d --name noryn-sync --restart unless-stopped \
  --env-file /sicherer/pfad/noryn-server.env \
  -p 127.0.0.1:8787:8787 \
  -v noryn-sync-data:/var/lib/noryn \
  ghcr.io/OWNER/REPO:0.1.0
```

`OWNER/REPO` durch den tatsächlichen kleingeschriebenen GitHub-Namen ersetzen. Für externen Zugriff einen HTTPS-Reverse-Proxy einrichten, siehe README. Bind-Mounts müssen für UID 10001 schreibbar sein. Bestehende Daten explizit weiter einbinden; nur eine Serverinstanz pro Verzeichnis betreiben.

Offizielle Grundlagen: [GitHub Container Registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry), [Images aus Actions veröffentlichen](https://docs.github.com/en/actions/tutorials/publish-packages/publish-docker-images), [Tauri-Paketierung](https://v2.tauri.app/distribute/windows-installer/).
