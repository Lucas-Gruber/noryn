# Base-Prompt für eine Desktop-App

Diesen Prompt in ein neues Projekt kopieren und die Projektangaben ausfüllen. Er beschreibt die gemeinsame App-Basis einschließlich des Update-Flows; die eigentliche Fachfunktion kommt unter „App-Funktion“ hinzu.

---

Erstelle eine produktionsfähige Desktop-App mit Tauri 2, Rust, Vue 3, TypeScript und Vite. Implementiere die folgende Basis vollständig, einschließlich Einstellungen, Gestaltung, Paketierung, Versionierung, GitHub-Releases und signierten Updates.

## Projektangaben

- App-Name: `<APP_NAME>`
- Technische Paketnamen: `<PACKAGE_NAME>`
- Dauerhafte App-ID im Reverse-DNS-Format: `<APP_ID>`
- GitHub-Repository: `<OWNER>/<REPO>`
- Öffentliche GitHub-Pages-Adresse: `<PAGES_BASE_URL>`
- App-Funktion: `<BESCHREIBUNG_DER_EIGENTLICHEN_APP>`
- Separater Server erforderlich: `<JA/NEIN; FALLS_JA: AUFGABE_DES_SERVERS>`

Verwende ausschließlich diese Projektidentität für App-Titel, Icons, Paketnamen, Konfiguration, Datenpfade, lokale Einstellungs-Keys und Dokumentation. Die App-ID bleibt über Updates hinweg stabil. Übernimm keine Namen, Schlüssel oder Server-Adressen aus einem anderen Projekt.

## Technische Basis

- Vue 3 mit Composition API, TypeScript und klar getrennten Komponenten und Composables.
- Rust für native Funktionen; bei Bedarf gemeinsamer Core als eigenes Cargo-Paket. Kleine, validierte Tauri-IPC-Schnittstellen und möglichst enge Capabilities.
- Rust stable, Node.js 22 oder neuer und pnpm mit Lockfile. Kein automatischer Wechsel auf einen fest eingetragenen pnpm-Manager.
- Einheitliche Befehle für Entwicklung, Frontend-Build, Formatierung, Prüfungen, Tauri-Build und Versionsänderung.
- Falls eine Browser-Vorschau angeboten wird, diese als Demo kennzeichnen und native Funktionen sinnvoll deaktivieren. Keine funktionierenden OS-Integrationen vortäuschen.
- OS-spezifische Implementierungen hinter klaren Schnittstellen kapseln. Fehler sichtbar und verständlich behandeln, laufende Aktionen begrenzen und nach Möglichkeit abbrechbar machen.

## Gestaltung, Schrift und Einstellungen

- Baue eine hochwertige, konsistente Oberfläche mit klarer Hierarchie, gut lesbaren Formularen und sinnvollen Abständen.
- Verwende semantische Design-Tokens für Hintergrund, Panels, Text, gedämpften Text, Akzente, Ränder, Fokus sowie Erfolgs-, Warn- und Fehlerzustände. Keine verstreuten festen Farben.
- Unterstütze Graphite als neutrales Standardtheme sowie Midnight, Nord, Carbon, Matrix und Light. Carbon soll neutral dunkelgrau mit blauen Akzenten sein; Matrix schwarz/grün, aber durchgehend lesbar. Light ist ein vollwertiges helles Theme.
- Liefere Inter Variable für die Oberfläche und JetBrains Mono Variable für Code/Terminal lokal mit. Keine externen Font-CDNs; Lizenzdateien beilegen.
- Biete unter Einstellungen eine globale Basisschriftgröße XS/SM/MD/LG/XL mit 12/13/14/16/18 px an. MD ist Standard. Größen in `rem` auf dieser Basis aufbauen; auch Eingabefelder verwenden dieselbe Basis.
- Theme- und Schriftänderungen sofort auf bestehende Ansichten anwenden, ohne Formulareingaben oder aktive Arbeit zu verlieren. Falls ein Terminal existiert, dessen Theme, Schrift und Layout ebenfalls aktualisieren, ohne die Verbindung neu aufzubauen. Helle, gut unterscheidbare ANSI-Textfarben erhalten.
- Textmarkierung, Caret, Fokus, Auswahl, deaktivierte Elemente und Fehler müssen in jedem Theme sichtbar sein.
- Verwende gemeinsame eigene Dropdown- und Kontextmenü-Komponenten mit Tastaturbedienung, Escape, Außenklick und korrekten ARIA-Zuständen. Keine nativen HTML-Selects.
- Einstellungen enthalten mindestens Darstellung, Sprache, Updates und einen Button „Programminfo“.
- Unterstütze Deutsch und Englisch mit lokaler System/DE/EN-Auswahl. Sprache an Darstellungsgrenzen übersetzen; Nutzerdaten, Dateipfade, Befehle und technische Protokollwerte unverändert lassen.
- Erscheinungsbild, Sprache und Update-Präferenzen bleiben lokal pro Installation. Nicht mit Team- oder Geschäftsdaten synchronisieren.

## Branding und Programminfo

- Erstelle ein klares skalierbares App-Icon mit angemessenem Innenabstand und optisch passender Dock-/Taskleisten-Größe. Liefere scharfe PNG-Größen, Windows-ICO und macOS-ICNS sowie die nötigen Linux-/Flatpak-Icons und Desktop-Metadaten.
- Zeige die tatsächlich gebaute Versionsnummer dezent in der App und, falls vorhanden, auf dem Sperrbildschirm an.
- Die Programminfo zeigt App-Name, Icon, Version, Beschreibung, Projekt-/Release-Link sowie die tatsächlich vorhandenen Lizenz- und Drittanbieterinformationen.
- Versionsnummern aus Paket-/Build-Metadaten beziehen und nicht separat im UI pflegen. Projekt-/Release-Links auf die konfigurierten HTTPS-Ziele beschränken.

## Versionierung

- Implementiere `pnpm release:version patch`, `minor`, `major`, eine explizite SemVer-Version und `--dry-run`.
- Aktualisiere damit zusammenhängend `package.json`, Tauri-Konfiguration, alle eigenen Cargo-Pakete und deren Einträge in `Cargo.lock`. Fremde Abhängigkeiten und deren Lockfile-Versionen unverändert lassen.
- Prüfe den Ausgangsstand und lehne ungültige Versionen, inkonsistente Manifeste und Downgrades vor einer Änderung ab. Bei Schreibfehlern keine teilweise umgestellten Versionen zurücklassen.
- Dieser Befehl ändert ausschließlich Versionen. Er erstellt weder Commit noch Tag und pusht nicht automatisch.
- Verwende Release-Tags `X.Y.Z` oder `vX.Y.Z`; Vorabversionen wie `vX.Y.Z-rc.1` ebenfalls berücksichtigen. Ein Tag muss auf dem Commit mit genau den passenden Manifestversionen liegen.
- Dokumentiere den Ablauf: Version erhöhen, prüfen, committen, neuen Tag erstellen, Commit und Tag pushen. Veröffentlichte Tags nicht nachträglich verschieben.

## GitHub-CI und Release-Dateien

- Lege die Pipeline unter `.github/workflows` an. Builds und Veröffentlichungen starten ausschließlich bei Push eines Versionstags, nicht bei normalen Branch-Pushes, Pull Requests oder manuellem Dispatch.
- Prüfe in einem kurzen Preflight den Tag gegen alle Paketversionen und benötigte Signierkonfiguration, bevor aufwendige Builds starten.
- Verwende Actions mit Node-24-Laufzeit. Die Node-Version des Projekts separat konfigurieren.
- Führe gemeinsame Frontend-Prüfungen, Rust-Tests, Formatierung, Clippy und Paketierungsprüfungen einmal unter Linux aus. Übergebe den geprüften Frontend-Build an die nativen Builder.
- Behalte sinnvolle native Windows-/macOS-Regressionen. Verwende dort für native Prüfungen und Paketierung dasselbe Release-Profil, damit schwere Abhängigkeiten nicht zusätzlich im Debug-Profil gebaut werden.
- Baue folgende Downloads und benenne sie nach Version, Plattform und tatsächlicher Architektur:
  - Windows: portable `.exe` und NSIS-Installer `-setup.exe`. Die portable Datei benötigt WebView2; der Installer richtet die benötigte Runtime ein.
  - macOS: portable `.app.zip` und `.dmg`. Eine `.app` ist ein Verzeichnis und wird deshalb einzeln als ZIP verpackt. Nur als Universal-Binary bezeichnen, wenn tatsächlich beide Architekturen enthalten sind.
  - Linux: `.AppImage`, `.deb` und `.flatpak`; für stabile Releases zusätzlich `.flatpakref`. Das AppImage heißt beispielsweise `<APP_NAME>-X.Y.Z-linux-x64.AppImage`, ohne „portable“ oder Build-Runner-Namen im Dateinamen.
- Dokumentiere Linux-Voraussetzungen ehrlich: Eine ältere Build-Basis verbessert die Kompatibilität, garantiert aber keine Ausführung auf jeder Distribution.
- Stelle jedes Paket und seine SHA-256-Prüfsumme einzeln als Actions-Artefakt und GitHub-Release-Asset bereit. Keine Sammel-ZIPs pro Betriebssystem. Verwende dafür bei passenden Artifact-Actions `archive: false`; beim Download das macOS-App-ZIP selbst erhalten, beispielsweise mit `skip-decompress: true`.
- Die Pipeline erstellt einen GitHub-Release-Entwurf für den exakten Tag, lädt alle Dateien hoch und veröffentlicht anschließend. Bei einem bestehenden Release fehlende Assets ergänzen; unterschiedliche Bytes unter einem bereits veröffentlichten Namen nicht überschreiben.
- Veröffentliche stabile Releases und Vorabversionen getrennt. Vorabversionen dürfen den stabilen Updatekanal nicht ersetzen.
- Falls ein Server erforderlich ist: zusätzlich einen mehrstufigen Container-Build, ein Image unter `ghcr.io/<owner-kleingeschrieben>/<repo-kleingeschrieben>` und eine kommentierte Compose-Datei mit `.env.example` liefern. Für GHCR den Workflow-`GITHUB_TOKEN` nutzen, Versionstags vergeben und `latest` nur für stabile Releases aktualisieren. Ein Server-Update ist ein separater Betreiber-Schritt und darf nicht als automatisch vom Desktop-Updater erledigt dargestellt werden.

## Gemeinsamer Update-Flow in der App

1. Erkenne die tatsächliche Installationsart und verwende den dafür vorgesehenen Updateweg.
2. Biete in Einstellungen → Updates die aktuelle Version, automatische Prüfung, „Jetzt prüfen“, Status, Fehler und einen Zugang zum Update-Dialog an.
3. Prüfe optional beim App-Start und bei erneutem Fokus nach mindestens sechs Stunden. Kein dauernder Polling-Timer. Debug-Builds installieren keine Updates; eine Browser-Demo sendet keine Update-Anfragen.
4. Kündige eine neue Version mit einem schließbaren Hinweis an. Der Dialog zeigt installierte/neue Version, Release-Notizen als reinen Text, den passenden Installationsweg und Fortschritt.
5. Download und Installation beginnen ausschließlich nach einem Klick. Anzeigen eines Angebots ist keine Zustimmung zur Installation.
6. Vor einer Installation aktive Sitzungen, Anmeldungen, Transfers, Sync- und Hintergrundaufträge berücksichtigen. Während einer Installation keine neuen widersprechenden Aktionen zulassen. Falls die App einen Tresor/geschützten Datenbereich besitzt, diesen vorher sperren und Entsperren während der Installation verhindern. Ungespeicherte Änderungen vor dem Schließen eines Formulars behandeln oder deutlich ankündigen.
7. Update-Aktionen auch im Rust-Backend serialisieren und Voraussetzungen dort erneut prüfen. Kein Vertrauen allein auf deaktivierte UI-Buttons.
8. Verbindungs-, Download- und Gesamtzeitlimits setzen. Abbruch, fehlende Quelle, Netzwerkfehler, ungültige Signaturen und Installationsfehler verständlich anzeigen. Fehlgeschlagene oder unvollständige Downloads nicht installieren.
9. Nutzerdaten bei Updates erhalten. Den Downloadweg und eine manuelle Alternative transparent anzeigen.

## Direkte Updates: AppImage, macOS-App und installiertes Windows

- Verwende den Tauri-Updater hinter eigenen kleinen IPC-Kommandos. Das Webview erhält keine uneingeschränkten Updater- oder Shell-Rechte.
- Verwende öffentliche GitHub-Releases und einen HTTPS-Feed wie `https://github.com/<OWNER>/<REPO>/releases/latest/download/latest.json`. Repository und Downloads müssen ohne Login erreichbar sein. Niemals persönliche GitHub-Tokens in die App einbauen.
- Pinne den öffentlichen Signierschlüssel in der App und aktiviere `requireSignedVersion`. Prüfe Paketbytes und signierte Version, bevor die Installation freigegeben wird. Keine Downgrades und keine Umgehung bei Signaturfehlern.
- Begrenze Paketgröße und Downloadzeit, beispielsweise auf 768 MiB und zehn Minuten. HTTPS auch nach Redirects verlangen, Zertifikatsprüfung aktiviert lassen und Teilpakete bei Abbruch verwerfen.
- Zeige Zustände wie verfügbar, Download, geprüft/bereit und Installation an. Download und Installation können getrennte Klicks haben; Installation muss ausdrücklich erfolgen.
- Unterstütze direkte Installation für AppImage, macOS-App und eine tatsächlich installierte NSIS-Version. Ein Bundler-Marker allein darf eine portable Windows-EXE nicht als Installation behandeln.
- Portable Windows-Dateien und Systempakete wie DEB/RPM verwenden einen manuellen Release-Download bzw. den zuständigen Paketmanager. Nicht stillschweigend die Installationsart ändern.
- Die Pipeline erzeugt passende Updatepakete, `.sig`-Dateien und den vollständigen `latest.json`-Feed für alle unterstützten Zielarchitekturen. AppImage und NSIS-Setup können wiederverwendet werden; macOS benötigt zusätzlich das passende Updater-Archiv.
- Prüfe Vollständigkeit, Versionsbindung und Downloadziele des Feeds. Veröffentliche `latest.json` zuletzt, nachdem alle referenzierten Pakete bereitstehen.
- Prüfe Signierfähigkeit früh mit einer temporären Signierprobe. Private Schlüssel und Passwort gehören ausschließlich in geschützte lokale Dateien und Actions-Secrets, etwa `TAURI_SIGNING_PRIVATE_KEY` und `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Nur öffentliche Schlüssel dürfen in den Quellcode. Vorhandene Signieridentitäten erhalten und Schlüssel nicht bei jedem Build neu erzeugen.
- Tauri-Updater-Signaturen, Windows-Authenticode und Apple-Code-Signing/Notarisierung sind getrennte Verfahren. Nicht eingerichtete OS-Signierung ausdrücklich dokumentieren.

## Flatpak-Updates über eine eigene Paketquelle auf GitHub Pages

### Veröffentlichung und Erstinstallation

- Veröffentliche eine eigene GPG-signierte OSTree-/Flatpak-Paketquelle unter `<PAGES_BASE_URL>/flatpak/`. Die Quelle für die App ist unabhängig von der Quelle ihrer GNOME-/Freedesktop-Runtime.
- Baue das Flatpak nach Möglichkeit aus dem bereits gebauten nativen Linux-Client, ohne zweiten vollständigen Rust-/Node-Build. Prüfe die Auflösung aller Bibliotheken innerhalb der ausgewählten Runtime.
- Liefere `<APP_NAME>.flatpakref` zum Installieren der App samt Paketquelle und optional `<APP_NAME>.flatpakrepo` zum separaten Hinzufügen der Quelle. Öffentlicher GPG-Schlüssel, App-ID, Branch und HTTPS-Quelle müssen zueinander passen.
- Füge auch dem einzelnen `.flatpak`-Bundle über `flatpak build-bundle --repo-url=… --gpg-keys=…` die Updatequelle und den öffentlichen Schlüssel hinzu. Dadurch wird bei der Installation des Bundles die Quelle mit eingerichtet; ein separater manueller `remote-add` soll nicht erforderlich sein.
- Bestehende Installationen aus alten Einzelpaketen ohne Quelle benötigen einmal den Wechsel über die neue `.flatpakref` und gegebenenfalls die Bestätigung der neuen Herkunft. App-ID und Datenverzeichnis beibehalten; keine Datenlöschung verlangen.
- Verwende für Flatpak eine eigene langfristige GPG-Signieridentität, unabhängig vom Tauri-Updater. Öffentlichen Schlüssel mitliefern, privaten Schlüssel geschützt sichern und etwa als Base64 in `FLATPAK_GPG_PRIVATE_KEY` hinterlegen. Die Pipeline muss prüfen, dass er zum festgelegten öffentlichen Schlüssel passt. Keine privaten Schlüssel in Repo, Artefakten, Logs oder Pages-Inhalten.
- Aktiviere GitHub Pages mit Quelle „GitHub Actions“. Die `github-pages`-Umgebung muss Deployments der verwendeten Versionstags erlauben, nicht ausschließlich `main`. Vergib `pages: write` und `id-token: write` nur im zuständigen Deployment-Job; keinen zusätzlichen Git-Remote ändern.
- Stelle die vollständige Paketquelle mit den passenden Pages-Actions bereit. Übernimm Vorgänger nur mit aktivierter GPG-Prüfung. Behalte mindestens den unmittelbar vorigen Stand für bereits laufende Downloads und erzeuge statische Deltas.
- Begrenze die Repository-Größe passend zu Pages und dokumentiere Hosting-/Bandbreitenlimits. Verwende bei größerer Verbreitung eine geeignete eigene Hostinglösung.
- Serialisiere Veröffentlichungen und verhindere, dass ein älterer Release eine neuere Paketquelle ersetzt. Publiziere die stabile Paketquelle nach erfolgreichen Builds und vor der GitHub-Release-/Updateankündigung. Vorabversionen verändern den stabilen Flatpak-Branch nicht.
- Die Installationslinks sind erst nach dem ersten erfolgreichen Pages-Deployment nutzbar. Dokumentiere die nötigen Secrets, Einrichtungsschritte, Schlüssel-Backups und den Ablauf bei fehlgeschlagenen Veröffentlichungsjobs.

### Update innerhalb der Flatpak-App

1. Erkenne Flatpak beispielsweise anhand von `/.flatpak-info`. Lade niemals ein AppImage, um eine Flatpak-Installation zu ersetzen.
2. Nutze den öffentlichen GitHub-Feed zur Versionsankündigung. Die eigentliche Installation kommt aus der bereits eingerichteten Flatpak-Herkunft und wird vom Host verifiziert und ausgeführt.
3. Zeige „Über Flatpak aktualisieren“. Erst der Klick prüft aktive Arbeit, sperrt gegebenenfalls den Datenbereich und startet Download und Installation gemeinsam.
4. Erzeuge dafür einen kurzlebigen Monitor über `org.freedesktop.portal.Flatpak.CreateUpdateMonitor` und rufe `org.freedesktop.portal.Flatpak.UpdateMonitor.Update` auf. Keine zusätzlichen umfassenden Host-/D-Bus-Rechte anfordern.
5. Abonniere Fortschritt vor dem Update-Aufruf, damit schnelle Signale nicht verloren gehen. Gewichte den Fortschritt über alle Operationen. Die 100 % einer einzelnen Operation dürfen nicht als Gesamtabschluss gelten; nur der abschließende Done-Status bestätigt Erfolg.
6. Biete Abbruch an und schließe den Monitor über `Close` auf jedem Abschlussweg, auch bei Fehler, Abbruch und Timeout. Keinen dauerhaften Monitor für Hintergrund-Polling offen halten.
7. Wenn die neue Version zusätzliche Berechtigungen benötigt, verweise auf Discover bzw. die Softwareverwaltung. Keine Berechtigungsprüfung umgehen und keine stillschweigende Erweiterung der Sandbox.
8. Melde eine fehlende Quelle, ein noch nicht verfügbares Paket, ein bereits extern installiertes Update oder einen Portalfehler verständlich und biete die passende manuelle Alternative an. Keine erfolgreiche Installation behaupten, wenn der Portalstatus Empty oder Failed ist.
9. Nach Erfolg „App schließen“ anbieten und erklären, dass die App über den App-Starter erneut geöffnet werden muss. Ein Prozessneustart innerhalb der alten Sandbox kann deren alte `/app`-Einbindung weiterverwenden. Einen vorhandenen geschützten Datenbereich bis zum Beenden gesperrt halten.
10. Discover und `flatpak update` müssen dieselbe Paketquelle ebenfalls verwenden können. Ein In-App-Update ersetzt nicht die normale Flatpak-Verwaltung.

## Prüfungen und Übergabe

- Prüfe Frontend-Build, TypeScript, Formatierung, `cargo fmt`, Clippy und relevante Rust-Tests.
- Teste den Update-Controller mit simuliertem IPC: Zustände, Fortschritt, Doppelklicks, Abbruch, aktive Arbeit, Sperren, Fehler und erneute Versuche.
- Teste ungültige Signaturen, manipulierte Paketbytes, falsche Versionsbindungen, unvollständige Feeds und Downgrade-Versuche für den direkten Updater.
- Teste Flatpak-Portalantworten auf einem ausdrücklich isolierten privaten D-Bus: Fortschritt, Done/Empty/Failed, Ablehnung, Timeout und Abbruch. Nie den Portal-Dienst der echten Desktop-Sitzung ersetzen.
- Prüfe Flatpak-Bundle-Herkunft, GPG-Verifikation und Ablehnung unsignierter Inhalte mit temporären Repositorys und einer separaten Testinstallation. Keine echte Nutzerinstallation oder deren Daten für Tests ändern.
- Prüfe Release-Staging, Assetnamen, Prüfsummen, Tag-/Manifest-Abgleich und sichere Wiederholungen von Uploads.
- Stelle lokale Linux-Prüfungen nicht als Windows-/macOS-Verifikation dar. Dokumentiere verbleibende Plattformprüfungen und die einmalige Einrichtung von Secrets/Pages klar.
- Liefere eine README und eine Release-Dokumentation mit allen tatsächlich benötigten Befehlen. Erstelle fokussierte Commits nach erfolgreichen Prüfungen. Commit-/Tag-Push und Veröffentlichung nur im ausdrücklich beauftragten Umfang ausführen.
