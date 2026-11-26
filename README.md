# Noryn

**Dein Desktop-Workspace für Server und Dateien.**

**SSH · [RDP](#remote-desktop) · SFTP · FTP · FTPS**

Öffne SSH- und Remote-Desktop-Sitzungen, übertrage Dateien und organisiere deine Server in privaten oder gemeinsamen Workspaces. Mit Noryn kann dein Team denselben Verbindungsbaum nutzen, während jeder sich mit seinem eigenen Konto anmeldet.

[Jetzt ausprobieren](#ausprobieren) · [Team-Sync einrichten](docs/setup.md#team-sync) · [Feedback geben](https://github.com/Lucas-Gruber/noryn/issues)

![Noryn. Deine Server. Ein Workspace. SSH, RDP, SFTP, FTP und FTPS. Die Oberfläche zeigt einen Team-Verbindungsbaum und den geerbten persönlichen Zugang eines Servers.](docs/noryn-hero.webp)

## Was du mit Noryn machen kannst

- **Server organisieren.** Lege verschachtelte Ordner an, filtere nach Protokoll und finde wichtige Hosts über Favoriten, Tags und Suche wieder. Zugangsdaten lassen sich vom Ordner an seine Verbindungen vererben.
- **Per SSH arbeiten.** Öffne mehrere Terminal-Tabs und nutze den Dateibrowser in derselben Sitzung. Terminal und Dateien teilen sich die Anmeldung, auch wenn der Server einen Einmalcode abfragt.
- **Per RDP arbeiten.** Öffne Remote-Desktop-Sitzungen aus deinem Verbindungsbaum. Aktiviere „Auflösung automatisch“, um die Remote-Auflösung an die Fenstergröße anzupassen.
- **Dateien übertragen.** Ziehe Dateien zwischen lokalen und entfernten Ordnern, verfolge Transfers in der Warteschlange und speichere Startpfade für SFTP-, FTP- und FTPS-Verbindungen.
- **SSH-Sitzungen verwalten.** Richte lokale TCP-Portweiterleitungen ein und sieh dir Prozesse, Dienste und Hardwaredaten auf unterstützten Linux-Hosts an.

[SSH-Terminal ansehen](docs/terminal-workspace-preview.png) · [Dateibrowser ansehen](docs/files-preview.png) · [Systemübersicht ansehen](docs/ssh-system-preview.png)

## Remote Desktop

RDP ist in den Noryn-Workspace integriert. Speichere Remote-Desktop-Ziele im selben Verbindungsbaum wie deine SSH- und Dateiverbindungen und starte die Sitzung direkt über den Eintrag. So liegen deine grafischen und terminalbasierten Zugänge an einem Ort.

**Automatische Auflösungsanpassung.** Aktiviere **Auflösung automatisch** im RDP-Tab, damit Noryn die tatsächliche Remote-Auflösung bei Fensteränderungen anpasst. Der RDP-Server muss dafür Display Control unterstützen. Auf anderen Servern bleibt die Bildskalierung verfügbar.

## Gemeinsam arbeiten

Lege für dein Team einen Workspace mit seinen Hosts, Ordnern und Notizen an. Noryn synchronisiert den Baum auf Knopfdruck oder optional im Live-Sync über einen selbst gehosteten Server. Die Daten werden vor dem Upload auf dem Gerät verschlüsselt.

Persönliche Konten und ihre Zuordnungen liegen im lokalen verschlüsselten Tresor. Ein gemeinsam genutztes Konto kannst du gezielt für einen Team-Workspace freigeben. Der private Workspace funktioniert auch ohne Sync-Server.

## Ausprobieren

Für Linux, Windows und macOS (Apple Silicon) findest du Installer und portable Pakete in den [GitHub Releases](https://github.com/Lucas-Gruber/noryn/releases/latest). Hinweise zur Installation findest du im [Setup](docs/setup.md#desktop-app) und für macOS [weiter unten](#macos-installation-ohne-apple-notarisierung).

### Browser-Vorschau

Du brauchst [Node.js 22.13 oder neuer](https://nodejs.org/) und [pnpm ab 10.26](https://pnpm.io/installation).

```sh
git clone https://github.com/Lucas-Gruber/noryn.git
cd noryn
pnpm install --frozen-lockfile
pnpm dev
```

Öffne [localhost:1420](http://127.0.0.1:1420). Die Vorschau simuliert Sitzungen und Transfers mit flüchtigen Beispieldaten. Echte Verbindungen laufen über die Desktop-App.

### Desktop-App

Installiere zusätzlich Rust stable, Perl für den OpenSSL-Build und die [Tauri-Systemabhängigkeiten](https://v2.tauri.app/start/prerequisites/). Starte die App aus dem Projektordner.

```sh
pnpm tauri dev
```

Erstelle beim ersten Start deinen Tresor, hinterlege ein Konto unter **Meine Zugangsdaten** und weise es einer Verbindung oder einem Ordner zu. Prüfe vor der ersten SSH-Anmeldung den Host-Fingerabdruck.

## macOS: Installation ohne Apple-Notarisierung

> [!WARNING]
> **Die macOS-Version von Noryn ist derzeit weder mit einer Apple Developer ID signiert noch von Apple notarisiert.** Deshalb kann macOS (Gatekeeper) den Start blockieren oder die Meldung „Noryn.app ist beschädigt“ anzeigen. Die Signatur des Tauri-Updaters ersetzt keine Apple-Notarisierung.

1. Lade die passende **DMG**-Datei oder das **portable .app.zip** ausschließlich aus den [offiziellen GitHub Releases](https://github.com/Lucas-Gruber/noryn/releases) herunter. Auf einem Mac mit Apple Silicon (M1/M2/M3/M4) ist normalerweise die **arm64**-Variante richtig.
2. Lade die zugehörige `.sha256`-Datei herunter und prüfe beide Dateien im selben Verzeichnis mit `shasum -a 256 -c DATEINAME.dmg.sha256` (bei ZIP entsprechend `.app.zip.sha256`). Fahre nur fort, wenn die Prüfung `OK` meldet.
3. Öffne die DMG und ziehe `Noryn.app` nach **Programme** (`/Applications`); bei der ZIP-Datei entpacke die App und verschiebe sie dorthin.
4. Versuche, `Noryn.app` einmal zu öffnen. Wird sie von macOS blockiert, gehe zu **Systemeinstellungen → Datenschutz & Sicherheit**, suche unten die Meldung zu Noryn und wähle **Dennoch öffnen** (`Open Anyway`). Bestätige den Dialog. Diese Option erscheint normalerweise erst nach einem blockierten Startversuch.

**Nur falls „beschädigt“ weiterhin angezeigt wird und „Dennoch öffnen“ nicht angeboten wird:** Vergewissere dich nochmals, dass Downloadquelle und SHA-256-Prüfsumme stimmen. Wenn du dieser konkreten App vertraust, kannst du als gezielte Ausnahme im Terminal `xattr -dr com.apple.quarantine "/Applications/Noryn.app"` ausführen und die App erneut starten. **Das entfernt die Gatekeeper-Quarantänemarkierung für diese App und umgeht damit eine Schutzprüfung; es signiert oder repariert keine tatsächlich beschädigte Datei.** Deaktiviere Gatekeeper nicht systemweit. Bei weiterhin auftretenden Fehlern melde die genaue macOS-Version und den heruntergeladenen Dateinamen im [Issue #8](https://github.com/Lucas-Gruber/noryn/issues/8).

## Projektstatus

Unterstützt werden SSH, RDP, SFTP, FTP und explizites FTPS. VNC-Sitzungen sind geplant. Der optionale Live-Sync führt unabhängige Änderungen zusammen und hält bei Konflikten an.

Gebaut mit Tauri 2, Vue 3, TypeScript, Rust und xterm.js.

[Setup, Builds und Tests](docs/setup.md) · [Architektur und Sicherheitsgrenzen](docs/architecture.md) · [Release- und Serverbetrieb](docs/releases.md) · [Issues und Ideen](https://github.com/Lucas-Gruber/noryn/issues)
