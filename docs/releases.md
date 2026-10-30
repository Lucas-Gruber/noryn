# GitHub-Builds, Releases und GHCR

`.github/workflows/build.yml` prüft gemeinsame Arbeit einmal im Linux-Job „Shared quality checks“: Frontend-Build und Controller, vollständige Rust-Suite einschließlich Server, verschlüsselten Sync, OpenSSH/SFTP/sudo, OTP, FTP/FTPS, Formatierung, Clippy und Paketierungslogik. Erst danach starten die nativen Paket-Jobs. Sie laden denselben geprüften `dist`-Stand als internes Actions-Artefakt und deaktivieren ausschließlich im CI-Aufruf den erneuten `beforeBuildCommand`; lokale Tauri-Builds bauen das Frontend weiterhin selbst.

Windows und macOS behalten native Regressionen für lokale Dateipfade/-sperren und die Tauri-Anwendung sowie echte OTP-/FTP-/FTPS-Prüfungen. Diese verwenden durchgehend `--release`, einschließlich der Transport-Harnesses über `NORYN_TEST_PROFILE=release`: OpenSSL und Rust-Abhängigkeiten gehören dadurch zum selben Profil wie die spätere Paketierung. Ein zusätzlicher vollständiger Debug-Build, Serverprüfungen und Clippy auf jedem teureren Runner entfallen. Linux hat seine Prüfungen bereits im gemeinsamen Job erledigt und baut danach nur Pakete. Das reduziert doppelte Arbeit, ist aber kein Ersatz für native Plattformtests; die Gesamtbeschleunigung muss der nächste Actions-Lauf zeigen.

Normale Actions-Caches werden zwischen verschiedenen Versionstags nicht geteilt. Ohne zusätzlichen Cache-Aufbau auf dem Standardbranch wäre ein neuer Release weiterhin kalt; deshalb fügt diese Umstellung keine wirkungslose Zusage für einen Cache über Release-Tags hinweg und keine zusätzlichen Branch-Builds hinzu. Siehe [GitHub: Cache-Zugriffsgrenzen](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache). Der gesamte Workflow startet ausschließlich beim Push eines Versionstags `X.Y.Z` oder `vX.Y.Z` (optional `-rc.1`) und veröffentlicht nach erfolgreichen Prüfungen Images und Releases. Normale Branch-Pushes und Pull Requests starten keine Builds; ein manueller Workflow-Start ist nicht vorgesehen. Alle Paketversionen in `package.json`, `src-tauri/tauri.conf.json`, Cargo-Paketen und `Cargo.lock` vor dem Tag gemeinsam erhöhen. Der erste Job „Validate release version“ prüft den Tag gegen Frontend, Tauri-Konfiguration, alle lokalen Cargo-Pakete und deren Lockfile-Einträge. Abweichungen brechen vor den aufwendigen Builds mit einer Liste der betroffenen Dateien ab; das Staging prüft erneut.

Die Frontend-Prüfung deckt außerdem automatische SSH-Anmeldungen für Befehls-Batches und den Abschluss externer Editor-Arbeitskopien ab. Die FTP-Prüfung verwendet explizit ersetzende und nicht ersetzende Testserver; damit hängt RNTO-Verhalten nicht mehr vom Betriebssystem des Runners ab.

## Version mit einem Befehl erhöhen

Mit Python 3.11+ setzt ein Befehl alle sechs Versionsstellen gemeinsam:

```sh
pnpm release:version patch        # 0.1.0 → 0.1.1
pnpm release:version minor        # 0.1.0 → 0.2.0
pnpm release:version major        # 0.1.0 → 1.0.0
pnpm release:version 0.2.0-rc.1   # genaue Versionsnummer; auch mit v-Präfix
pnpm release:version patch --dry-run
```

Die Beispiele sind Alternativen. Aktualisiert werden `package.json`, `src-tauri/tauri.conf.json`, alle drei Cargo-Manifeste und ausschließlich die lokalen Pakete/Verweise in `Cargo.lock`. Abhängigkeiten werden nicht aktualisiert. Formatierung, Kommentare und sonstige Änderungen bleiben erhalten. Die Befehle `patch`, `minor` und `major` erhöhen die jeweilige Zahl und entfernen einen vorhandenen Vorabversionssuffix; für die Freigabe einer bestehenden Vorabversion die gewünschte stabile Nummer ausdrücklich angeben.

Ein inkonsistenter Ausgangsstand, ungültige Versionen und Downgrades werden vor dem Schreiben abgelehnt. Eine bereits überall gesetzte Version ändert nichts. Der Befehl erstellt keine Commits/Tags und pusht nichts. Danach die Änderungen prüfen, committen und den ausgegebenen passenden Tag pushen; dieser startet den Release-Workflow.

Beispiel für eine neue Version `0.2.0` (den Tag an die tatsächlich ausgegebene Version anpassen):

```sh
pnpm release:version 0.2.0
git diff
git add -- package.json src-tauri/tauri.conf.json crates/core/Cargo.toml src-tauri/Cargo.toml server/Cargo.toml Cargo.lock
git commit -m "chore: release version 0.2.0"
git tag -a v0.2.0 -m "Noryn 0.2.0"
git push --atomic origin HEAD refs/tags/v0.2.0
```

Vor dem Commit die Versionsänderungen prüfen und die Projektprüfungen ausführen. `git push` überträgt nur Commits, keine uncommitteten Dateien. Auch `git tag` markiert ausschließlich einen Commit: Ein Stern am lokalen Branch (`main*`) weist auf noch nicht committete Änderungen hin. Der atomische Push überträgt Branch und Tag gemeinsam. Falls ein Tag bereits auf einen Commit mit falscher Version gepusht wurde, eine neue Versionsnummer samt Commit und neuem Tag verwenden. Ein erneuter Lauf des alten Tags verwendet weiterhin dessen alten Commit.

## Einrichtung nach dem Umzug zu GitHub

1. Den aktuellen Repository-Stand einschließlich `.github/workflows` zu GitHub übertragen.
2. GitHub Actions im Repository zulassen. Der Workflow verwendet gehostete Ubuntu-, Windows- und macOS-Runner; zusätzliche eigene Runner sind nicht nötig. Für private Repositories gelten die Minuten-/Nutzungsgrenzen des GitHub-Tarifs.
3. Die Job-Berechtigungen `contents: write` für Releases und `packages: write` für den Image-Job zulassen. Repository-/Organisationsrichtlinien dürfen diese Rechte nicht einschränken. Ein zusätzlicher Registry-Login oder PAT ist normalerweise nicht nötig: verwendet wird das automatische `GITHUB_TOKEN`.
4. Versionsnummern committen, den aktuellen Code pushen und anschließend einen passenden Versionstag auf diesem Commit erstellen und pushen. Ein GitHub Release muss vorher nicht manuell angelegt werden: Der letzte Job „Create GitHub Release and upload individual assets“ übernimmt das nach erfolgreichen Builds und dem Image-Upload.

Das GHCR-Paket heißt `ghcr.io/<owner>/<repo>` (alles kleingeschrieben). Der Container ist der Sync-Server. Der Workflow setzt die OCI-Quell-URL, Revision und Version und veröffentlicht mit dem Token desselben Repositories; damit wird das Paket mit dem Repository verknüpft. Tags: `X.Y.Z`, für stabile Releases zusätzlich `latest`; Vorabversionen ändern `latest` nicht. Ein neues GHCR-Paket ist standardmäßig privat. Für anonymes `docker pull` dessen Sichtbarkeit in den Package-Einstellungen auf öffentlich stellen; andernfalls vor dem Pull bei GHCR anmelden.

Veröffentlichungsjobs laufen nach erfolgreichen Desktop-Builds ausschließlich bei Versions-Tag-Pushes. Checkout, Node- und Python-Setup verwenden v7 mit Node-24-Laufzeit; die Projekt-Node-Version bleibt 22. Image-/Release-Publikation wurde hier nicht ausgeführt; der lokale Git-Remote wird nicht automatisch geändert.

## Downloads im GitHub Release

Die unten genannten Dateien stehen sowohl im Actions-Lauf als auch im GitHub Release einzeln zum Download bereit, jeweils mit eigener `.sha256`-Datei. [Upload Artifact v7](https://github.com/actions/upload-artifact/tree/v7#upload-an-individual-file-unzipped) lädt jede Datei mit `archive: false` direkt hoch; zusätzliche Sammel-ZIPs pro Betriebssystem entfallen. [Download Artifact v8](https://github.com/actions/download-artifact/tree/v8) übernimmt die Dateien unverändert für den Release-Publisher, einschließlich des macOS-App-Archivs (`skip-decompress: true`). Die Namen hängen vom Zielsystem ab, nicht von der Version des Build-Runners. Das Linux-AppImage ist distributionsübergreifend vorgesehen; das DEB ist für Debian/Ubuntu. Architektur und Version stehen im jeweiligen Dateinamen.

| System  | Portable                                            | Installer                                                    |
| ------- | --------------------------------------------------- | ------------------------------------------------------------ |
| Windows | `Noryn-X.Y.Z-windows-x64-portable.exe`              | `Noryn-X.Y.Z-windows-x64-setup.exe`                          |
| macOS   | `Noryn-X.Y.Z-macos-arm64-portable.app.zip` bzw. x64 | `Noryn-X.Y.Z-macos-arm64.dmg` bzw. x64                       |
| Linux   | `Noryn-X.Y.Z-linux-x64.AppImage`                    | `Noryn-X.Y.Z-linux-x64.deb`, `Noryn-X.Y.Z-linux-x64.flatpak` |

Jedes Anwendungs- und Installationspaket erhält eine SHA-256-Prüfsumme. Das macOS-Paket enthält die Architektur des tatsächlichen Runners, kein Universal-Binary. Nur die portable macOS-`.app` benötigt weiterhin ein ZIP, weil sie ein Verzeichnis mit ausführbaren Dateien und Ressourcen ist. Die App aus dem ZIP ist direkt nutzbar; das DMG bietet die übliche Installation nach Applications. AppImages bei Bedarf ausführbar machen (`chmod +x`). „Portable“ bedeutet ohne Installation ausführbar; der Tresor bleibt im normalen Benutzer-Datenverzeichnis.

Die portable Windows-EXE benötigt eine vorhandene WebView2-Runtime; der NSIS-Installer enthält den WebView2-Offline-Installer. Windows-Signierung und Apple-Signierung/Notarisierung sind noch nicht eingerichtet. Windows und macOS wurden lokal nicht ausgeführt; sie werden von der GitHub-Matrix geprüft.

### macOS-Downloads ohne Developer-ID-Signierung

Die macOS-Pakete sind derzeit **nicht Apple-Developer-ID-signiert und nicht notarisiert**. Die Signatur für den Tauri-Updater ist davon unabhängig. Gatekeeper kann daher einen Start blockieren oder die App als „beschädigt“ melden. Das [README](../README.md#macos-installation-ohne-apple-notarisierung) enthält die Anleitung mit SHA-256-Prüfung, dem gezielten macOS-Dialog **Datenschutz & Sicherheit → Dennoch öffnen** und einer ausdrücklich gekennzeichneten, nur bei vertrauenswürdigem Download zu verwendenden Quarantäne-Ausnahme. Gatekeeper nicht systemweit deaktivieren.

`scripts/publish-release.py` fügt **bei jeder neu erstellten GitHub-Release-Beschreibung** automatisch einen sichtbaren Warnhinweis und Installationsschritte ein. Der Link im Hinweis zeigt auf das README des jeweiligen Release-Tags. Bereits veröffentlichte Releases werden bei erneutem Ausführen des Publishers nicht umgeschrieben; dafür wäre eine separate, bewusste Aktualisierung erforderlich. Die Regressionstests in `scripts/test-release.py` prüfen die Warnung und README-Anleitung. Der Hinweis bleibt notwendig, bis Apple-Developer-ID-Signierung und Notarisierung tatsächlich eingerichtet und verifiziert sind.

### Flatpak

Zusätzlich steht ein einzelnes `.flatpak`-Paket mit Prüfsumme bereit, auch für Arch Linux. Installieren und starten:

```sh
flatpak install --user ./Noryn-0.1.0-linux-x64.flatpak
flatpak run dev.noryn.client
```

Die GNOME-51-Runtime wird bei Bedarf von Flathub geladen; sie ist nicht im Paket enthalten. Es erfolgt keine Veröffentlichung auf Flathub. Derzeit gibt es kein eigenes Flatpak-Update-Repository: neue `.flatpak`-Dateien aus GitHub Releases erneut installieren. Die App zeigt bei Updates deshalb den Release-Download statt einer Installation über den AppImage-Updater an.

Externe Textdateien und Skripte werden über das Desktop-Portal mit bestätigter Dateiübergabe geöffnet. Der manuelle Abschluss „Bearbeitung beenden“ übernimmt gespeicherte Änderungen. Der Portal-Test im gemeinsamen Linux-Prüfjob verwendet einen privaten D-Bus und startet keine Desktop-Programme.

Die Freigaben stehen in `packaging/flatpak/config.json`: Netzwerk, Wayland/X11, Grafikbeschleunigung, SSH-Agent sowie Home und die üblichen Mount-Pfade `/run/media`, `/media`, `/mnt` für den lokalen Dateibrowser. Kein Zugriff auf den gesamten Host oder den System-D-Bus. Zusätzliche externe Pfade bei Bedarf gezielt über Flatseal/Flatpak freigeben. Die verschlüsselten App-Daten liegen getrennt unter `~/.var/app/dev.noryn.client/`; vorhandene native Tresore über Export/Import übernehmen.

`scripts/build-flatpak.py` verpackt den zuvor gebauten nativen Linux-Client und prüft dessen Bibliotheken innerhalb der Runtime. Dafür genügt die Platform, ohne zweiten Rust-/Node-Build oder SDK. Lokal zuerst die Runtime installieren (`flatpak install flathub org.gnome.Platform//51`), dann nach dem nativen Paket-Build `python scripts/build-flatpak.py` ausführen. Der Release-Workflow verlangt das Flatpak beim Staging und lädt Paket/Prüfsumme einzeln hoch. Paketierung und Runtime-Auflösung wurden lokal unter Linux geprüft. Grundlagen: [Flatpak-Einzeldateien](https://docs.flatpak.org/en/latest/single-file-bundles.html).

Windows/MSVC bettet das Anwendungsmanifest über den Linker in alle finalen Ziele des Tauri-Pakets ein, einschließlich des Unit-Testprogramms. `windows-app-manifest.xml` fordert dieselben Common Controls 6 wie Tauris Standardmanifest an. Tauris zusätzliches Ressourcenmanifest wird dafür deaktiviert; Icon und Versionsressourcen bleiben erhalten. Ohne dieses Manifest kann der Testprozess vor dem ersten Test mit `0xc0000139 / STATUS_ENTRYPOINT_NOT_FOUND` abbrechen, weil native Dialoge `TaskDialogIndirect` aus Common Controls 6 importieren. Nur `rustc-link-arg-tests` würde den Bibliotheks-Unit-Test nicht erfassen. Die vorhandenen Windows-App-Tests bleiben aktiv; ein [Tauri-Maintainer beschreibt den Manifest-Workaround](https://github.com/tauri-apps/tauri/issues/13419). `LNK4099` zu `ossl_static.pdb` betrifft dagegen fehlende Debugsymbole des vendorten OpenSSL und erklärt diesen Ladefehler nicht.

Der Windows-Job setzt `OPENSSL_SRC_PERL` ausdrücklich auf das native Strawberry Perl und prüft die benötigten Module vorab. Git Bash stellt sonst seine eigene unvollständige Perl-Installation voran; dann kann der vendorte OpenSSL-Build an `Locale::Maketext::Simple` scheitern. Der verwendete [openssl-src-Buildcode](https://github.com/alexcrichton/openssl-src-rs/blob/main/src/lib.rs) unterstützt diese explizite Interpreter-Auswahl.

SSH aktiviert zusätzlich zu `vendored-openssl` das Feature `openssl-on-win32`: Nur dieses wählt im [libssh2-Build](https://github.com/rust-lang/ssh2-rs/blob/master/libssh2-sys/build.rs) OpenSSL anstelle des Windows-Backends WinCNG aus. `node scripts/test-otp.mjs` prüft Schlüssel/Passwort mit OTP, mehrere Fragen, Abbruch und falsche Codes sowohl mit Standardalgorithmen als auch mit ausschließlich Curve25519 und RSA-SHA2-512. Der zusätzliche Durchlauf verlangt diese Algorithmen ohne Fallback und erkennt damit ein versehentlich anderes Kryptobackend. Handshake-Fehler des Testservers werden ausgegeben; Authentifizierungsantworten und Schlüssel werden nicht protokolliert.

Linux baut auf Ubuntu 22.04 für eine ältere glibc-Basis. `APPIMAGE_EXTRACT_AND_RUN=1` vermeidet FUSE-Mounts während der Paketierung. Der native Linux-Build, AppImage und DEB sowie das Server-Container-Image wurden lokal erstellt und geprüft.

Der Publisher erstellt in der Pipeline erst einen Release-Entwurf und veröffentlicht nach vollständigem Upload aller einzelnen Dateien. Existiert bereits ein Release für genau diesen Tag, ergänzt er fehlende Assets; ein bereits veröffentlichtes Release wird dabei nicht erneut veröffentlicht. Tag-Namen mit und ohne `v` werden unverändert im GitHub Release verwendet, während das GHCR-Image immer die reine Version ohne `v` erhält. Wiederholungen überschreiben keine Dateien: identische Assets werden übersprungen, unterschiedliche Bytes bei gleichem Namen brechen ab. Für neue Builds daher neue Versions-Tags verwenden.

```sh
# Nach Versionsänderung, Commit und Push des Codes:
git tag v0.1.0
git push origin v0.1.0
```

Ein Tag bezeichnet einen festen Commit. Ältere Tags verwenden daher auch nach späteren Code-Pushes oder bei „Re-run jobs“ ihre alte Workflow-Version. Für korrigierte Builds einen neuen, zur Paketversion passenden Tag auf dem aktuellen Commit verwenden; veröffentlichte Tags nicht nachträglich verschieben. Wird ein Release vorher von Hand veröffentlicht, können die Downloads bis zum Abschluss der Pipeline fehlen. Ein Release für einen bereits bestehenden Tag startet diesen Tag-Push-Workflow nicht erneut. Die zwei automatisch von GitHub angebotenen Quellcode-Archive sind keine Desktop-Installer.

## App-Updates einrichten

Unter **Einstellungen → Updates** können Nutzer automatisch prüfen lassen oder die Prüfung selbst starten. Geprüft wird beim Start und bei erneutem Fokus nach mindestens sechs Stunden, heruntergeladen und installiert erst auf Klick. Direkte Updates unterstützen Linux-AppImage, die macOS-App und installierte Windows-NSIS-Versionen. Die portable Windows-EXE wird manuell ersetzt; DEB/RPM werden über den Paketmanager aktualisiert. Der Dialog öffnet dafür das Release im Browser. Vor der Installation Sitzungen schließen; Noryn sperrt den Tresor und startet neu. Debug-Builds installieren keine Updates, die Browser-Vorschau sendet keine Update-Anfragen.

Die App lädt `https://github.com/Lucas-Gruber/noryn/releases/latest/download/latest.json`. Repository **und Release-Dateien müssen öffentlich erreichbar sein**; ein GitHub-Login im Browser reicht für die App nicht. Keine persönlichen GitHub-Tokens in die App einbauen. Der stabile Kanal verwendet GitHubs „latest“-Release; Vorabversionen gelangen nicht automatisch in diesen Kanal.

Für dieses Projekt wurde ein passwortgeschütztes Signierpaar lokal erzeugt:

- Privater Schlüssel: `~/.config/dev.noryn.client/release-signing/updater.key`
- Passwort: `~/.config/dev.noryn.client/release-signing/password`
- Öffentlicher Schlüssel: fest in `src-tauri/tauri.conf.json` eingebettet.

Das Verzeichnis hat Modus `0700`, Schlüssel und Passwort `0600`. Beide privaten Dateien separat sicher sichern und niemals committen. Ein Verlust des Schlüssels verhindert weitere automatische Updates für bereits ausgelieferte Apps; einen bestehenden Schlüssel daher nicht beiläufig neu erzeugen oder ersetzen.

**Vor dem nächsten Release** unter Repository → Settings → Secrets and variables → Actions zwei Repository Secrets setzen:

| Secret                               | Inhalt                                                                |
| ------------------------------------ | --------------------------------------------------------------------- |
| `TAURI_SIGNING_PRIVATE_KEY`          | Vollständiger Inhalt der Datei `updater.key`, kein lokaler Dateipfad. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Inhalt der Datei `password`.                                          |

Optional mit bereits eingerichtetem GitHub CLI aus dem Repository-Verzeichnis (Bash und fish):

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.config/dev.noryn.client/release-signing/updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD < ~/.config/dev.noryn.client/release-signing/password
```

Die Pipeline bricht bei fehlendem Schlüssel früh ab. Nach Installation der Build-Werkzeuge signiert eine Probe eine temporäre Datei und prüft Passwort, Schlüssel-ID, öffentlichen App-Schlüssel und Versionsbindung, ohne Schlüsselmaterial zu protokollieren. Beim nativen Build erzeugt Tauri die Signaturen. Für lokale Paket-Builds dieselben beiden Variablen setzen; dort darf `TAURI_SIGNING_PRIVATE_KEY` alternativ auf die lokale Schlüsseldatei zeigen. Die Variablen werden direkt im Prozess benötigt, eine `.env`-Datei allein genügt nicht. Für versionsgebundene Signaturen wird Tauri CLI ab 2.12.4 verwendet.

Zusätzlich zu den normalen Downloads veröffentlicht der Workflow die `.sig`-Dateien, ein separates macOS-Updatepaket `Noryn-X.Y.Z-macos-ARCH-update.app.tar.gz` samt Prüfsumme und `latest.json`. AppImage und Windows-Setup werden als Updatepakete wiederverwendet. Die internen `Noryn-…-update.json`-Fragmente bleiben Actions-Artefakte und werden vor Veröffentlichung zu einem vollständigen Feed für alle drei Systeme zusammengeführt. Fehlende Plattformen, fremde URLs, falsche Schlüssel-IDs oder Versionsbindungen werden abgelehnt. Die App prüft zusätzlich die eigentliche kryptografische Signatur der Paketbytes vor Download-Freigabe und Installation.

Neue Releases entstehen erst nach vollständigem Upload aus einem Entwurf. Bei einem bereits existierenden Release wird `latest.json` zuletzt hochgeladen, damit es keine fehlenden Pakete ankündigt. Aktualisierte Apps werden mit demselben Schlüssel signiert; ein gültig signiertes altes Paket lässt sich durch Umbenennen der Manifestversion nicht als neues Update ausgeben. Windows-Code-Signing und Apple-Notarisierung bleiben getrennte, noch nicht eingerichtete Schritte. Grundlagen: [Tauri Updater](https://v2.tauri.app/plugin/updater/).

## Containerbetrieb

Für Live-Sync auch den Server aktualisieren; ältere Images unterstützen weiterhin manuellen Sync, besitzen aber den `/events`-Endpunkt noch nicht. Der Reverse-Proxy muss `text/event-stream` ungepuffert weiterleiten und langlebige Verbindungen zulassen. Der Server sendet alle 30 Sekunden einen Keepalive und setzt `X-Accel-Buffering: no`. Ein unterbrochener Stream wird erneut aufgebaut; die erste Meldung enthält die aktuelle Revision, sodass keine Zwischenrevisionen nachgeladen werden müssen.

`Dockerfile` baut den Server in mehreren Stufen. Die Laufzeit enthält CA-Zertifikate, läuft als UID/GID 10001 und schreibt verschlüsselte Snapshots nach `/var/lib/noryn`. Team-Schlüssel gehören nur in Clients; der Server kennt lediglich Team-IDs und Server-Tokens.

Neue Datenverzeichnisse werden unter Unix mit `0700` angefordert. Vorhandene Volume-Rechte und ACLs verwaltet der Betreiber; der Server verändert sie nicht mit `chmod`. Das unterstützt auch fremde Eigentümer mit ausdrücklich eingerichtetem Gruppenzugriff, etwa Kubernetes-`fsGroup`. Beim Start testet der Server einmal mit eigenen temporären Dateien die benötigten Dateioperationen. Ein Fehler nennt den Datenpfad und die betroffene Operation, bevor der Dienst Verbindungen annimmt. Bestehende Snapshot-Dateien bleiben unverändert.

### Docker Compose

Die kommentierte [`compose.yaml`](../compose.yaml) und [`.env.example`](../.env.example) liegen im Repository-Stamm. Beide Dateien können auch zusammen in einen eigenen Ordner auf dem Server kopiert werden; der Quellcode wird für das veröffentlichte Image nicht benötigt.

```sh
cp .env.example .env
chmod 600 .env
# Pro Team einen eigenen zufälligen Server-Token erzeugen:
openssl rand -hex 32
# .env im Editor öffnen: Image-Pfad und NORYN_SYNC_TEAMS eintragen.
docker compose config --quiet
docker compose up -d
docker compose logs -f noryn-sync
```

Zum Beispiel sieht die Team-Zeile mit zwei Teams so aus (Platzhalter ersetzen):

```dotenv
NORYN_SYNC_TEAMS=platform-team:DEIN_ERSTER_ZUFAELLIGER_TOKEN_MIT_64_HEX_ZEICHEN,office-team:DEIN_ZWEITER_ZUFAELLIGER_TOKEN_MIT_64_HEX_ZEICHEN
```

| Variable           | Bedeutung                                                                                                                                                                    |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `NORYN_SYNC_IMAGE` | Nur Compose: tatsächlicher GHCR-Paketname mit Version, z. B. `ghcr.io/dein-name/dein-repo:0.1.0`. Owner und Repo kleingeschrieben.                                           |
| `NORYN_SYNC_TEAMS` | Server: kommagetrennte `team-id:token`-Paare, ohne Leerzeichen. Mindestens ein Team; pro Team ein zufälliger Token mit mindestens 32 Zeichen. Keine doppelten Team-IDs.      |
| `NORYN_SYNC_BIND`  | Server: Listen-Adresse **im Container**, hier `0.0.0.0:8787`. `127.0.0.1` würde dort nur Verbindungen aus demselben Container annehmen.                                      |
| `NORYN_SYNC_DATA`  | Server: Speicherpfad **im Container**, hier `/var/lib/noryn`. Das benannte Docker-Volume hält die verschlüsselten Snapshots über Container-Neustarts und Updates hinweg vor. |
| `NORYN_SYNC_PORT`  | Nur Compose: Port auf dem Docker-Host, standardmäßig `8787`. Die rechte Portnummer in `ports` bleibt passend zu `NORYN_SYNC_BIND` auf `8787`.                                |

Nur `NORYN_SYNC_IMAGE`, `NORYN_SYNC_TEAMS` und optional `NORYN_SYNC_PORT` in `.env` bearbeiten. Die beiden internen Server-Pfade/-Adressen sind direkt in `compose.yaml` gesetzt. [Compose liest `.env` zur Variablenersetzung](https://docs.docker.com/compose/how-tos/environment-variables/variable-interpolation/); die `environment`-Einträge reichen die drei Server-Variablen in den Container weiter. Ein fehlender oder leerer Team-Eintrag verhindert den Start. `config --quiet` prüft die Konfiguration, ohne Tokens auszugeben.

Die Team-ID ist frei wählbar: 3–80 Buchstaben, Zahlen oder Bindestriche; `private` ist reserviert. Im Client dieselbe Team-ID und denselben Server-Token hinterlegen. **Der zusätzliche Team-Schlüssel verschlüsselt die Daten bereits im Client und gehört niemals in die Compose-Datei oder Server-Umgebung.** Es gibt keine serverseitigen Umgebungsvariablen für SSH-Konten, Passwörter oder diesen Schlüssel.

Der Port ist ausschließlich an `127.0.0.1` des Docker-Hosts gebunden. Für externe Clients einen HTTPS-Reverse-Proxy auf diesem Host vorschalten, siehe [Caddy-Beispiel im README](../README.md#team-sync). Bei einem Reverse-Proxy in einem weiteren Container stattdessen ein gemeinsames Docker-Netz verwenden und intern auf `noryn-sync:8787` weiterleiten; dessen `127.0.0.1` bezeichnet nicht den Docker-Host. Als Server-URL im Client z. B. `https://sync.example.com` ohne API-Pfad eintragen. Der Server selbst bietet HTTP; TLS übernimmt der Proxy.

Änderungen an Team-Tokens oder dem Image mit `docker compose up -d` übernehmen; `docker compose restart` lädt geänderte Umgebungsvariablen nicht neu. Ein privates GHCR-Paket erfordert vorher `docker login ghcr.io`. Für Updates die veröffentlichte Version in `.env` ändern, `docker compose pull` und dann `docker compose up -d` ausführen.

`docker compose down` behält das Volume; **`docker compose down -v` löscht es samt Snapshots**. Den Compose-Projektnamen bzw. Verzeichnisnamen für dieselbe Installation beibehalten, damit Docker dasselbe Volume verwendet. Bind-Mounts als Alternative müssen für UID 10001 schreibbar sein. Bestehende Daten explizit weiter einbinden; nur eine Serverinstanz pro Datenverzeichnis betreiben.

### Kubernetes-PVC

Das PVC an `/var/lib/noryn` einbinden und `NORYN_SYNC_DATA` auf den tatsächlich verwendeten Datenordner setzen. Bereits gespeicherte Snapshots bleiben am bisherigen Pfad; ein Wechsel in einen neuen Unterordner verschiebt keine Dateien. Nur eine Serverinstanz pro Datenverzeichnis betreiben und beim Aktualisieren überlappende Pods vermeiden, etwa mit einer Deployment-Strategie `Recreate`.

Für Storage-Treiber mit `fsGroup`-Unterstützung unter `spec.template.spec` ergänzen:

```yaml
securityContext:
  runAsUser: 10001
  runAsGroup: 10001
  runAsNonRoot: true
  fsGroup: 10001
```

`fsGroup` gehört zum Pod, nicht zur Container-Konfiguration. Es ermöglicht Gruppenzugriff, macht den Prozess aber nicht zum Dateieigentümer. Deshalb änderte das frühere erzwungene `chmod 0700` einen vorhandenen, fremden Volume-Ordner nicht erfolgreich. Bei diesen älteren Images muss der Datenordner dem Server-Benutzer gehören; die neue Startprüfung benötigt stattdessen tatsächlich funktionierende Lese-/Schreib-/Umbenennungsrechte. Die StorageClass kann die Rechteverwaltung an ihren CSI-Treiber delegieren. Bei NFS/SMB sind zusätzlich die serverseitigen Export-/Mount-Einstellungen zu beachten. Den Zugriff auf den Server-Benutzer bzw. seine vorgesehene Volume-Gruppe begrenzen. [Kubernetes: Volume-Berechtigungen](https://kubernetes.io/docs/tasks/configure-pod-container/security-context/#configure-volume-permission-and-ownership-change-policy-for-pods).

Zur Diagnose nur Benutzer, Verzeichnisrechte, Datenpfad und StorageClass prüfen; keine vollständige Container-Umgebung oder Secret-Manifeste in Logs kopieren. `NORYN_SYNC_DATA` muss beim laufenden Pod gelten; eine lokale Manifeständerung allein aktualisiert den Container nicht. Der Server benötigt Anlegen, Schreiben, Synchronisieren, atomisches Ersetzen, Lesen und Löschen im Datenordner. Verweigert der Storage dies, bricht die Startprüfung mit dem konkreten Schritt ab.

Offizielle Grundlagen: [GitHub Container Registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry), [Images aus Actions veröffentlichen](https://docs.github.com/en/actions/tutorials/publish-packages/publish-docker-images), [Tauri-Paketierung](https://v2.tauri.app/distribute/windows-installer/).
