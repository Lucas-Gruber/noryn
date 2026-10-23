# Codex im Web und Aufgaben mit Projektwissen

Neue Aufgaben laden `AGENTS.md` und die verlinkten Architektur-/Release-Dokumente aus dem Repository. Darin stehen die hier vereinbarten Produktentscheidungen, Sicherheitsgrenzen und Testbefehle. Aktueller Code und Tests bleiben die Wahrheit. Der vollständige Verlauf dieses Chats wird in einem neuen Chat nicht automatisch übernommen.

## Cloud-Umgebung nach dem GitHub-Umzug

1. Das Projekt einschließlich `AGENTS.md` und `docs/` nach GitHub übertragen.
2. In Codex im Web „Work in → Cloud → Create environment“ wählen, GitHub verbinden und dieses Repository auswählen.
3. Node.js 22, pnpm, Rust stable und die Tauri-Linux-Abhängigkeiten einrichten lassen. Als Prüfungen `pnpm build`, `cargo test -p noryn-core -p noryn-sync` und die relevanten Integrationstests verwenden. Browsermodus enthält nur flüchtige Beispieldaten; native SSH-/Dateifunktionen liegen im Rust-Backend.
4. Einrichtung prüfen und das Environment veröffentlichen. Neue Aufgaben daraus starten und auf `AGENTS.md` verweisen.

Cloud-Aufgaben können weiterarbeiten, während der eigene Rechner schläft. Für wiederkehrende Issue-Bearbeitung müssen zusätzlich Zeitplan, Labelauswahl und Ergebnis feststehen. Sinnvoll ist ein ausdrückliches Label wie `codex-ready`, damit nur ausreichend beschriebene Aufgaben automatisch ausgewählt werden. Repository-/Issue-Zugriff muss für die ausführende Umgebung eingerichtet sein; keine Tokens in Prompts oder Quellcode hinterlegen.

Beispielauftrag:

> Prüfe im aktuellen GitHub-Repository offene Issues mit dem Label `codex-ready`. Lies zuerst AGENTS.md und die Architekturregeln. Wähle ein reproduzierbares Issue, das noch nicht bearbeitet wird. Reproduziere es, implementiere eine vollständige, fokussierte Korrektur, führe die passenden Tests aus und erstelle einen nachvollziehbaren Commit. Berichte Issue-Nummer, Änderung, Tests und verbleibende Hindernisse. Keine automatischen Merges, Kommentare oder Issue-Schließungen. Wenn kein geeignetes Issue vorhanden ist, keine Dateien ändern.

Falls gewünscht, Push und Pull-Request-Erstellung ausdrücklich zum Auftrag hinzufügen. Hier wurde noch keine wiederkehrende Automation angelegt. Eine lokale Codex-Automation ist ebenfalls möglich; dafür muss der Rechner verfügbar sein.

Offizielle Anleitungen: [Cloud-Umgebungen](https://learn.chatgpt.com/docs/environments/cloud-environments), [Projektwissen mit AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md).
