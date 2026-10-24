# Architektur und Grenzen

## Geteilte Metadaten, persönliche Identität

Ein `Node` enthält ID, Parent-ID, Workspace-ID, Typ, Anzeigenamen, Protokoll, Host, Port, Notizen, Tags und Favoritenstatus. Ordner können beliebig verschachtelt werden. Der Rust-Kern prüft eindeutige IDs, existierende Ordner als Eltern, Workspace-Grenzen und Zyklen. Verbindungen können keine Kinder haben.

Der Workspace `private` ist reserviert. Jeder Team-Workspace hat eine eigene ID und einen eigenen E2E-Schlüssel. Einträge können innerhalb ihres Workspaces verschoben werden. Eine Verschiebung zwischen privat und Team ist bewusst nicht implementiert, um eine versehentliche Freigabe zu vermeiden.

Die Zuordnung ist separat:

```text
TeamSnapshot                     Lokaler Tresor von Alice
  Ordner production [id: A]        Credential X → Benutzer alice
    Server web-01 [id: B]           bindings[A] = X
                                  Ergebnis für B: alice (geerbt)

                                 Lokaler Tresor von Bob
                                   Credential Y → Benutzer bob
                                   bindings[A] = Y
                                   Ergebnis für B: bob (geerbt)
```

Eine Zuordnung direkt an B überschreibt die geerbte Zuordnung. Entfernen der direkten Zuordnung aktiviert die Vererbung wieder. Fehlende Zugangsdaten verhindern eine Verbindung mit einer verständlichen Meldung.

`TeamSnapshot` ist eine eigene, streng deserialisierte Struktur mit `version`, `teamId`, `nodes` in Version 2 optional `credentials` und in Version 3 optional `activity`. Nur Konten mit ausdrücklicher `teamId` des betreffenden Teams werden aufgenommen. Der gesamte Tresor wird niemals synchronisiert. Persönliche Zuordnungen, Hostschlüssel-Pins, Sync-Token, Team-Schlüssel, andere Teams, Privatkonten und private Einträge bleiben ausgeschlossen. Lokale Agentenkonten dürfen nicht geteilt werden. Kontokollisionen mit Privatkonten oder anderen Teams werden vor jeder Änderung abgelehnt. Version-1-Snapshots bleiben lesbar und entfernen keine neueren Team-Konten. Ein Abruf validiert den gesamten resultierenden Baum vor dem Ersetzen. Fremde Workspace-Einträge und ID-Kollisionen werden abgelehnt.

## Tresor

- Ein zufälliges 16-Byte-Salz pro Tresor.
- Argon2id v0x13 mit 64 MiB Speicher, drei Durchläufen, einer Lane und 32-Byte-Schlüssel.
- XChaCha20-Poly1305 mit neuer zufälliger 24-Byte-Nonce für jeden Schreibvorgang.
- Authentifizierte Kontextbindung `noryn:vault:v1`.
- Schreiben über temporäre Datei im selben Verzeichnis, `sync_all` und atomisches Ersetzen. Unix: Verzeichnis `0700`, Datei `0600`.
- Änderungstransaktionen setzen bei Fehlern den vorherigen Speicherzustand wieder ein.
- Schlüssel, Geheimnisse und entschlüsselte Serialisierungsbuffer werden mit `zeroize` bestmöglich bereinigt. Vollständiger Schutz gegen Swap, Prozess-Dumps oder einen kompromittierten Desktop ist dadurch nicht gegeben.

Das Frontend erhält ausschließlich `VaultView` mit Verbindungsmetadaten, Kontonamen/-benutzernamen und persönlichen Zuordnungen. Passwort, private Schlüssel, Schlüssel-Passphrasen und Sync-Geheimnisse werden nach dem Speichern nicht zurückgegeben. Browser-Vorschau und native App verwenden dieselbe Oberfläche; die Vorschau arbeitet nur im Speicher.

Zugangsdaten lassen sich zur persönlichen Übersicht in verschachtelten Ordnern ablegen. `credentialFolders` und `credentialLocations` sind separate Metadaten im verschlüsselten lokalen Tresor, mit leeren Standardwerten für bestehende Tresore. Sie gelangen nie in `TeamSnapshot` oder `Credential`. Ablage und Ordner besitzen keine Vererbung, keine Verbindungsfunktion und keine Freigabewirkung; reine Ablageänderungen markieren Teams nicht als geändert. Löschen eines Ordners verschiebt seine Konten und unmittelbaren Unterordner eine Ebene höher, ohne Konten oder Zuordnungen zu löschen. Der Kern prüft IDs, Eltern, Zyklen und höchstens 32 Elternebenen. Ein Sync-Abruf erhält persönliche Ablagen für weiterhin vorhandene Konto-IDs und bereinigt nur Verweise auf entfernte Konten.

Sperren schließt registrierte Sitzungen, verwirft den entschlüsselten Rust-Tresor und leert den Frontend-Status. Noch laufende blockierende Netzwerk-/Dateioperationen können erst bei Abschluss oder Timeout freigegeben werden. Es gibt noch keine automatische Sperre bei Inaktivität oder Betriebssystem-Sperre.

Der neue Speicherpfad verwendet `dev.noryn.client`; die Hilfe zeigt den konkreten Pfad. `Vault::open_migrating` übernimmt einen alten Tresor nur, wenn noch keine neue Datei existiert und das Passwort erfolgreich geprüft wurde. Die alte Datei bleibt unverändert. Es gibt keinen Rückfall auf die alte Datei bei einem falschen Passwort für einen vorhandenen neuen Tresor. Neue Schreibvorgänge verwenden ausschließlich Noryn-Kontexte; alte Tresor-/Team-Kontexte sind auf das Migrationsmodul begrenzt.

### Verschlüsselte Sicherungen

`backup` verwendet ein eigenes versioniertes `.noryn`-Format mit zufälligem 16-Byte-Salz, Argon2id und XChaCha20-Poly1305 unter `noryn:backup:v1`. Das separate Backup-Passwort hat mindestens 12 Zeichen; es wird nicht gespeichert. Archive sind auf 64 MiB begrenzt, Eingaben werden begrenzt gelesen und vor Mutation auf Baum, Teams, Zugangsdaten, Bindings, persönliche Ordner und Verlauf validiert. Atomisches exklusives Veröffentlichen verhindert auch bei konkurrierenden Exporten ein Überschreiben bestehender Dateien.

Die explizite Auswahl umfasst alle Tresore, den privaten Bereich oder genau ein Team. Nur vollständige Sicherungen enthalten sämtliche lokalen Zuordnungen und Host-Pins. Private Exporte enthalten keine Team-Konten/-Konfiguration; Team-Exporte nur freigegebene Konten und die Team-Konfiguration inklusive Team-Schlüssel/Server-Token, ohne persönliche Bindings, Ordner oder Mitgliedsnamen. Diese Archive sind keine Sync-Snapshots und werden nie an den Sync-Server gesendet.

Der native Import hält eine geprüfte Sicherung hinter einer zufälligen Vorschau-ID im Backend. Das Frontend erhält ausschließlich Metadaten/Zähler. Anwenden verlangt eine höchstens fünf Minuten alte Vorschau, unveränderte lokale Daten und geschlossene Verbindungen/Anmeldungen; Abbruch, Komponentenwechsel und Tresorsperre verwerfen die Vorschau. Ein fehlgeschlagener Schreibvorgang rollt durch `Vault::change` zurück. Der ausgewählte Bereich wird ersetzt, IDs fremder Workspaces dürfen nicht überschrieben werden. Team-Importe erhalten bestehende persönliche Konfiguration/Zuordnungen; neue Teams benötigen einen eigenen Namen. Vollständige Wiederherstellung übernimmt die enthaltenen Konfigurationen, erhält aber vorhandene eigene Mitgliedsnamen. `imported`/`workspace` erfasst den Import ohne Secret-Werte im Team-Verlauf; Team-Änderungen werden als lokal geändert markiert. Der aktuelle lokale Verschlüsselungsschlüssel bleibt erhalten. Für die neuen Verlaufstypen müssen alle Team-Clients aktualisiert sein.

## Signierte App-Updates

`src-tauri/src/updates.rs` kapselt `tauri-plugin-updater` ab Version 2.14 hinter eigenen IPC-Kommandos. Das Frontend bekommt nur Versionsnummern, gekürzte Textnotizen und Download-Fortschritt. Es gibt keine direkten Updater-/Opener-Plugin-Berechtigungen im Webview; nur die festen öffentlichen GitHub-Links zu Projekt und Releases dürfen im Browser geöffnet werden. Prüfen benötigt keinen entsperrten Tresor und sendet keine Zugangsdaten oder GitHub-Tokens. `latest.json` und Pakete müssen öffentlich erreichbar sein.

Die Release-App prüft acht Sekunden nach Start und bei erneutem Fokus nach mindestens sechs Stunden; es gibt keinen wiederkehrenden Polling-Timer und keinen automatischen Download. Die lokale Option liegt unter `noryn.updates.v1`, außerhalb von Tresor, Export, Sync und Verlauf. In Debug-Builds ist die automatische Prüfung deaktiviert; im Browser werden keine Update-Aufrufe ausgeführt. Ein verfügbares Update zeigt einen schließbaren Hinweis und einen Dialog mit Notizen, Fortschritt und Abbruch. Notizen werden als Text gerendert, nie als HTML.

Der native Check verwendet HTTPS mit Zertifikatsprüfung und 20 Sekunden Timeout. Der gesonderte Download akzeptiert nur HTTPS, auch nach Redirects, verwendet 15 Sekunden Verbindungs-/10 Minuten Gesamttimeout und höchstens 768 MiB. Ein Watch-Signal bricht den Stream ab, unvollständige Daten werden verworfen. Die vollständigen Bytes bleiben nur im Arbeitsspeicher. Vor Freigabe zur Installation prüft der offizielle Tauri-Verifier die Ed25519/Minisign-Signatur und die signierte Version (`requireSignedVersion: true`). Installation prüft beides nochmals. Es werden ausschließlich neuere Versionen angeboten; Downgrades sind nicht aktiviert. Der öffentliche Schlüssel ist Teil der App-Konfiguration, der private Release-Schlüssel wird weder eingebaut noch ins Repository geschrieben.

Update-Aktionen werden nativ serialisiert. Die Oberfläche blockiert Installation während Sitzungen/Anmeldungen/Live-Abgleichen und sperrt den Tresor vor dem Installationskommando. Der Installer prüft unabhängig davon, dass der Tresor gesperrt ist und keine Sitzungen, Anmeldungen oder Sync-Jobs laufen. Die Tresorsperre bleibt während des Austauschs gehalten; ein zusätzlicher Installationsstatus verhindert erneutes Entsperren bis zum Neustart. Ein Fehler bleibt sichtbar, der Tresor gesperrt. Installation und Neustart geschehen ausschließlich nach ausdrücklichem Klick; Formulareingaben werden dabei verworfen.

Direkte Installation ist für AppImage, macOS-App und installierte NSIS-Versionen vorgesehen. Das neben der Windows-EXE vorhandene NSIS-`uninstall.exe` unterscheidet die installierte Version von der ebenfalls vom Bundler markierten portablen EXE. Flatpak wird über `/.flatpak-info` erkannt und ausschließlich über Flatpak aktualisiert. Portable Windows-Dateien und DEB/RPM/unpaketierte Builds bieten den Release-Download statt einer stillen Umwandlung der Installationsart an. Updater-Signaturen sind unabhängig von Windows-Authenticode und Apple-Notarisierung. Für bestehende ältere Apps ohne Updater ist zunächst ein manueller Wechsel auf einen Release mit Updater nötig.

## SSH, Terminal und Dateien

`ssh2`/libssh2 bildet den nativen Transport auf Linux und Windows. Die Features `vendored-openssl` und `openssl-on-win32` bündeln OpenSSL und wählen es auch unter Windows ausdrücklich als SSH-Kryptobackend aus; Vendoring allein lässt dort WinCNG aktiv. Damit stehen moderne SSH-Algorithmen und Schlüssel aus verschlüsseltem Speicher plattformübergreifend bereit. Für die Anmeldung werden Passwort, privater Schlüssel mit optionaler Passphrase, lokaler SSH-Agent und Keyboard-Interactive-Abfragen (z. B. OTP) unterstützt. Schlüssel und Benutzerpasswort können in einem Konto zusammen gespeichert werden. Antworten auf Serverabfragen werden nur im laufenden Anmeldedialog verarbeitet. Der Dialog unterstützt mehrere und aufeinanderfolgende Fragen, Abbruch und ein Zeitlimit. Agent-Verfügbarkeit ist plattformabhängig; FIDO2 ist noch offen.

Beim ersten Kontakt wird nur ein SSH-Handshake ausgeführt. Vor der Anmeldung muss der Nutzer den SHA-256-Hostschlüssel bestätigen. Geänderte Schlüssel erzeugen eine gesonderte Warnung. Der beim tatsächlichen Verbindungsversuch ausgehandelte Schlüssel wird erneut verglichen; Dateikanäle verwenden denselben geprüften Transport. Pins bleiben lokal. Eine Unterstützung von `known_hosts`-Dateien und Host-Zertifikaten ist noch offen.

Pro Sitzung verwendet das MVP einen authentifizierten SSH-Transport mit getrennten Kanälen. Die Anmeldung einschließlich OTP erfolgt einmal. Bei einer SSH-Verbindung verarbeitet ein Rust-Thread den PTY-Kanal für das Terminal. Bei einer reinen SFTP-Verbindung wird kein Shell-/PTY-Kanal angefordert.

Der Dateizugriff verwendet SFTP- und bei sudo zusätzliche Exec-Kanäle auf demselben Transport. Die libssh2-Sitzung bleibt nicht blockierend; Dateioperationen behandeln EAGAIN mit begrenzten Wiederholungen. Dadurch kann das Terminal auch während Dateitransfers Daten verarbeiten. Das Session-Objekt wird intern durch libssh2-rs synchronisiert, Dateioperationen werden zusätzlich pro Verbindung serialisiert.

Die Oberfläche verwendet einen einzigen zentralen Verbindungsbaum. Lokale und entfernte Dateibereiche bieten Verlauf, Pfadsegmente, direkte Eingabe und eine Transferwarteschlange. Interne Datei-Drags verwenden Pointer-Events, weil Tauris nativer Datei-Drop-Handler auf Windows HTML5-DnD ersetzt. Externe Datei-Drops verwenden weiterhin Tauris native Ereignisse. Diese Kombination erhält beide Transferwege.

Lokale Verzeichnisse werden im Rust-Backend gelesen. Windows-Laufwerke werden angeboten, Pfad-Trennzeichen werden vom Backend geliefert. Downloads aus direkten Drags ersetzen keine bestehenden Dateien automatisch; ein erneuter Versuch mit Ersetzen benötigt eine explizite Bestätigung.

Die SFTP-Funktionen unterstützen Einzeldateien und Ordnererstellung. Downloads schreiben zunächst in eine temporäre lokale Datei. Uploads erstellen Remote-Dateien exklusiv, damit bestehende Dateien nicht still überschrieben werden. Abgebrochene Uploads können eine unvollständige Remote-Datei hinterlassen; Resume und automatisches Cleanup sind noch offen. Dateiverknüpfungen werden angezeigt, rekursive Verzeichnisübertragungen sind noch nicht implementiert.

### Weiterleitungen und Systemansicht

`ssh_tools::Tools` verwaltet zusätzliche Kanäle derselben authentifizierten SSH-Sitzung. Lokale Weiterleitungen binden ausschließlich an `127.0.0.1`, akzeptieren einen frei wählbaren Port (0 = automatisch) und verwenden `direct-tcpip`. Bis zu 16 Listener und 32 gleichzeitige TCP-Streams pro Sitzung; feste 32-KiB-Puffer pro Richtung, Rückstau und TCP-Halbschluss. Inaktive Listener warten auf Socket-Ereignisse ohne periodische Aufweckvorgänge. Aktive Streams prüfen gepufferte libssh2-Daten mit höchstens 100 ms Wartezeit. Stoppen, Tresorsperre und Terminalende schließen die Listener und brechen Streams ab.

Systemdaten werden mit festen, nur lesenden Befehlen abgefragt: procps `ps` (maximal 100 Prozesse, CPU-Durchschnitt seit Prozessstart, keine vollständigen Befehlszeilen), `systemctl` (maximal 400 geladene oder installierte Dienste samt UnitFileState) und `/proc`, `uname`, `df` für Hardware. Linux-Ziele benötigen keinen Agenten und keine Rechteerhöhung. Fehlendes systemd meldet eine nicht verfügbare Ansicht. Ausgabe ist auf 256 KiB pro Stream und die aktive Abfrage auf 8 Sekunden begrenzt; Kanalaufbau besitzt zusätzlich SSH-Zeitlimits. Ergebnisse werden eine Sekunde gecacht. Prozess-/Dienstlisten werden standardmäßig manuell, optional alle 5 Sekunden abgefragt. Hardwaregraphen lesen bei sichtbarer Ansicht standardmäßig alle 5 Sekunden /proc-Zähler; CPU- und I/O-Raten entstehen aus den tatsächlichen Differenzen, ohne erfundene Vergangenheit. Bis zu 120 Samples bleiben im Arbeitsspeicher. Ausgeblendete Ansichten starten keine weiteren Abfragen; eine bereits laufende Abfrage darf noch abschließen.

### FTP und FTPS

`suppaftp` stellt den separaten nativen FTP-Transport bereit. Der gemeinsame `remote::Remote`-Adapter serialisiert SSH- und FTP-Dateioperationen über dieselben Tauri-Kommandos. FTP-/FTPS-Sitzungen besitzen weder PTY noch sudo-Modus. Sie verwenden die persönliche Benutzername-/Passwort-Zuordnung; SSH-Agent und private Schlüssel sind keine FTP-Anmeldemethoden.

Explizites FTPS sichert vor der Anmeldung die Steuerverbindung per AUTH TLS und danach alle Datenverbindungen per PROT P. Native TLS prüft Zertifikatskette und Hostnamen mit dem System-Vertrauensspeicher. Zertifikatsfehler, fehlendes AUTH TLS oder eine verweigerte TLS-Datenverbindung führen zum Abbruch, ohne Klartext-Fallback. Klassisches FTP bleibt als eigener unverschlüsselter Eintragstyp verfügbar. Die Ende-zu-Ende-Verschlüsselung des Team-Syncs ist davon unabhängig.

Passive Verbindungen verwenden ausschließlich die IP der tatsächlich aufgebauten Steuerverbindung, auch bei abweichenden PASV-Adressen. IPv6 verwendet EPSV. Steuer- und Datensockets besitzen Zeitlimits. Verzeichnislisten werden zuerst per MLSD gelesen; nur bei nicht unterstützten Befehlen wird auf LIST zurückgegriffen. Die LIST-Parser unterstützen POSIX- und DOS-Formate; nicht interpretierbare Einträge melden einen Fehler. CR, LF und NUL in FTP-Kommandoparametern werden vor der Übertragung abgelehnt.

Downloads werden lokal temporär gespeichert und erst nach positiver Serverbestätigung veröffentlicht. FTP-Uploads verwenden einen zufälligen temporären Namen und anschließend RNTO. Existierende Zielnamen werden vor dem Upload und vor der Veröffentlichung geprüft. FTP bietet keine portable atomische Operation „nur erstellen, wenn nicht vorhanden“; ein gleichzeitiger Schreibzugriff zwischen letzter Prüfung und RNTO kann deshalb je nach Server trotzdem eine Datei ersetzen. Fehlerpfade versuchen nur die eigene temporäre Datei aufzuräumen.

Der optionale `initialPath` ist gemeinsame Verbindungsmetadaten für SFTP/FTP/FTPS. Alte Einträge ohne dieses Feld werden mit leerem Startpfad geladen. Nur absolute Pfade sind erlaubt. Der Backend-Verbindungsaufbau prüft die Lesbarkeit vor Registrierung der Sitzung; das Frontend beginnt dort, während der Home-Button weiter den tatsächlichen Benutzerordner öffnet. SSH-Verbindungen haben kein konfigurierbares Startordnerfeld.

„Alle Workspaces“ ist ausschließlich ein lokaler Ansichtsfilter (`*`, kein gültiger Team-Bezeichner). Der Baum bildet getrennte Gruppen pro Team und privatem Workspace. Neue Einträge wählen immer einen tatsächlichen Workspace; sie können nicht mit dem Ansichtsfilter als Workspace gespeichert oder zwischen Workspaces verschoben werden.

### sudo-Dateimodus

Eine SFTP-Sitzung kann nicht nachträglich mit `sudo` ihren Benutzer wechseln. Noryn öffnet stattdessen Exec-Kanäle auf dem bestehenden SSH-Transport und führt einen fest eingebauten Shell-Befehl über sudo aus. Auf dem Ziel wird **kein Python und kein installierter Noryn-Agent** benötigt.

Voraussetzungen sind passende sudo-Rechte, `sh`, GNU find (mit `-printf`) sowie die üblichen Linux-Werkzeuge `cat`, `mkdir`, `wc` und `id`.

- Zuerst wird `sudo -n` ohne Passwort versucht. Das deckt NOPASSWD und gültige sudo-Caches ab.
- Falls ein Passwort nötig ist, wird ein vorhandenes Benutzerpasswort des ausgewählten Zugangs verwendet. Eine Schlüssel-Passphrase wird niemals als sudo-Passwort eingesetzt.
- Ohne passendes gespeichertes Passwort erscheint erst dann ein zusätzlicher Dialog.
- Bei passwortgestütztem sudo wird das Passwort ausschließlich nach der tatsächlichen sudo-Abfrage über stdin gesendet. Bei NOPASSWD wird auch ein vorhandenes Passwort nicht in den Dateistream geschrieben.
- Eine feste Bereitschaftsmarkierung trennt Anmeldung und Dateiinhalt. Stdout und stderr werden gleichzeitig verarbeitet; Zeitlimits und Größenlimits begrenzen fehlerhafte Antworten.
- Pfade sind separat gequotete Positionsargumente für `sh -c`, nicht Teil des Skriptcodes. Auflistungen sind NUL-getrennt und erhalten Leerzeichen, Anführungszeichen und Zeilenumbrüche in Dateinamen. Siehe [GNU findutils](https://www.gnu.org/software/findutils/manual/html_node/find_html/Unusual-Characters-in-File-Names.html).
- Binärdaten laufen direkt über den SSH-Kanal. Uploads verwenden exklusives Anlegen (`noclobber`) mit `0600` und prüfen die übertragene Größe. Vorhandene Dateien und Upload-Symlinks werden nicht überschrieben.
- Der Wechsel wird erst nach Prüfung der Root-Identität und Werkzeuge aktiviert. Ein fehlgeschlagener Wechsel stellt den alten Modus wieder her.
- Passwörter bleiben ausschließlich im verschlüsselten Tresor bzw. im Arbeitsspeicher der Sitzung. Es werden keine sudoers-Regeln verändert.

Die Testumgebung simuliert sudo-Abfragen ohne echte Rechteerhöhung. Reale sudoers-Beschränkungen, requiretty und individuelle PAM-Regeln sind zusätzlich auf dem jeweiligen Ziel zu prüfen; das MVP richtet keine Zielsysteme automatisch um.

## Verschlüsseltes Sync-Protokoll

Der Team-Schlüssel besteht aus 32 zufälligen Bytes, als Base64 geteilt. Er ist vom lokalen Tresor-Passwort und dem Server-Token unabhängig. Jeder Upload verschlüsselt einen vollständigen Team-Snapshot mit einer neuen Nonce und Kontext `noryn:team:v1:<team-id>`.

```text
Client → TeamSnapshot → XChaCha20-Poly1305 → HTTPS → Server
Client ← validierter Baum ← Entschlüsselung ← HTTPS ← Server
```

`GET /v1/teams/{id}/snapshot` liefert einen verschlüsselten Envelope und eine `ETag`-Revision. `PUT` erwartet bei neuen Snapshots `If-None-Match: *`, bei vorhandenen `If-Match: <revision>`. Eine veraltete Revision liefert `412 Precondition Failed`. Authentifizierung: `Authorization: Bearer <team-token>`. Nicht konfigurierte Teams und falsche Tokens liefern `401`.

Der Server speichert ausschließlich Envelope, Nonce und Versionsnummer. Er kennt aber Team-IDs, Zugriffsmuster, Größe und Zeitpunkt von Snapshots. Token-Vergleich erfolgt über SHA-256-Hashes mit zeitkonstantem Vergleich. Ein globaler Prozess-Mutex serialisiert Schreiben und Revisionsprüfung; ein gemeinsames Datenverzeichnis über mehrere Serverprozesse wird nicht unterstützt.

Transportseitig werden ausschließlich HTTPS oder explizite lokale HTTP-Endpunkte akzeptiert. Zertifikatsprüfung bleibt aktiv, HTTP-Redirects sind abgeschaltet. Anfragen haben Timeouts, Server und Client begrenzen die Snapshot-Größe. Authentifizierte Verschlüsselung erkennt falsche Schlüssel, Inhaltsmanipulation und Vertauschen von Teams. Sie verhindert allein keine Wiedergabe einer älteren gültigen Snapshot-Version durch einen bösartigen Server. Ein monotones, signiertes Änderungsprotokoll und Rollback-Erkennung gehören in eine spätere Version.

Manuelles Abrufen ersetzt nach Bestätigung den lokalen Team-Baum. Optionaler Live-Sync führt unabhängige Änderungen automatisch zusammen und hält bei echten Konflikten an. Es gibt kein CRDT, keine interaktive Auswahl widersprüchlicher Feldwerte, keine Mitgliederverwaltung und keinen automatischen Schlüsselwiderruf. Nach Verlust eines Team-Schlüssels oder Austritt eines Mitglieds muss eine geregelte Schlüsselrotation außerhalb des aktuellen MVP erfolgen.

### Ereignisgesteuerter Live-Sync

`Team.liveSync` ist eine lokale, standardmäßig ausgeschaltete Option. `syncBase` enthält die letzte bestätigte kanonische `TeamSnapshot`-Serialisierung ausschließlich im verschlüsselten lokalen Tresor; sie wird weder an das Frontend zurückgegeben noch in Sync-Payloads oder Exportarchive aufgenommen. Importe löschen diese Basis und schalten Live-Sync für die importierten Teams aus. Ohne Basis werden ein unveränderter Serverstand oder ein leeres neues Mitglied sicher initialisiert; beidseitig neue Inhalte erfordern einen manuellen Erstabgleich.

`GET /v1/teams/{id}/events` verlangt denselben Bearer-Token und meldet nur die aktuelle Revision sowie spätere Änderungen als SSE. Abonnieren und Lesen der ersten Revision verwenden dieselbe Schreibsperre wie Uploads, damit kein Update dazwischen verloren geht. Ein Tokio-Watch-Kanal pro verwendetem Team fasst ausstehende Revisionen zusammen; es gibt keine unbeschränkte Ereigniswarteschlange. Nur erfolgreich persistierte Uploads senden Meldungen. Alle 30 Sekunden erhält die Verbindung einen SSE-Keepalive; im Leerlauf werden keine Snapshots abgefragt. Neue Verbindungen erhalten immer den aktuellen Stand, sodass kein dauerhafter Ereignisverlauf auf dem Server nötig ist.

Der native Client hält Token und SSE-Transport im Backend, prüft HTTPS/Zertifikate und lehnt Redirects ab. Eingehende Zeilen sind auf 4 KiB begrenzt; gelesen wird mit 75 Sekunden Timeout. Verbindungsabbrüche verwenden einen Backoff von 2 bis 60 Sekunden, fehlende Unterstützung/Authentifizierung wird sichtbar gemeldet. Sperren, Deaktivieren und Konfigurationswechsel brechen den Stream ab. Das Frontend bündelt lokale Speichervorgänge 600 ms; private Zuordnungen/Ablagen erzeugen keine Team-Ereignisse und damit keine Uploads. Eigene Revisionsmeldungen werden nach erfolgreicher Bestätigung ignoriert. Ein erneuter Abruf unveränderter Daten verursacht keinen Upload und kein erneutes lokales Speichern.

`auto_sync` lädt und validiert den verschlüsselten Serverstand und führt einen Dreiwegevergleich von gemeinsamer Basis, lokalen Daten und Serverstand aus. Unterschiedliche Felder/Einträge werden vereinigt, gleiche Änderungen dedupliziert, Löschung gegen Änderung und ungültige Baumkombinationen als Konflikt gemeldet. SSH-Schlüssel und Passphrase werden zusätzlich als zusammengehörige Einheit geprüft. Ereignisse werden nach UUID ohne Uhrzeit-Vorrang zusammengeführt, ihre Werte dürfen bei gleicher ID nicht abweichen; die letzten 1.000 aufgezeichneten Ereignisse bleiben erhalten. Konflikte enthalten nur IDs, Namen und Feldbezeichner, niemals Notiz-/Secret-Werte. Es gibt keine stille Auswahl nach Zeitstempel und noch keinen Dialog für einzelne Konfliktwerte. Manuelles Abrufen bleibt eine ausdrücklich bestätigte Ersetzung.

Uploads behalten die bestehende Revisionsbedingung; höchstens drei konkurrierende Schreibversuche pro Abgleich. Netzwerkoperationen arbeiten auf einer abgetrennten Kopie genau eines Teams, außerhalb der Tresorsperre. Beim atomischen lokalen Speichern werden zwischenzeitliche lokale Änderungen erneut gegen den angefragten Stand zusammengeführt, fremde Workspace-Kollisionen geprüft und neue ausstehende Änderungen erhalten. Eine Tresor-Epoche verhindert ein spätes Commit nach Sperren/Wiederherstellung. Manuelle und automatische Jobs sind pro Team gegenseitig ausgeschlossen. Die lokale `localRevision` steigt nur nach erfolgreichen Tresor-Transaktionen; das Frontend verwirft verspätete IPC-Ansichten mit älterer Revision, damit ein Hintergrundabgleich keine neueren Formulareingaben im lokalen Datenbestand verdeckt.

Offene Eintragseditoren senden ihren Ausgangsstand mit: Beim Speichern bleiben inzwischen übernommene fremde Feldänderungen erhalten; widersprüchliche Eingaben oder inzwischen entfernte Einträge werden im Dialog abgelehnt. Geteilte Zugangsdaten verlangen für eine Bearbeitung zusätzlich die beim Öffnen bekannte lokale Tresor-Revision. Bei zwischenzeitlichen Änderungen muss dieser Zugang neu geöffnet werden; eingegebene Secret-Werte bleiben im Dialog und werden nicht unbemerkt auf einen geänderten Zugang angewendet.

### Team-Namen und Änderungsverlauf

`Team.memberName` ist ein persönlicher, pro Team erforderlicher Anzeigename. Er bleibt in der lokalen Team-Konfiguration und wird nur als `actor` tatsächlicher Änderungen geteilt. Bestehende Tresore erhalten zunächst einen leeren Namen; Abrufen bleibt im Kern möglich, Team-Änderungen und Hochladen verlangen einen gültigen Namen. Die Team-Oberfläche führt bei fehlendem Namen zur Konfiguration. Namensänderungen schreiben alte Einträge nicht um.

`teamActivity` hält pro Team höchstens 1.000 zuletzt aufgezeichnete Änderungen im verschlüsselten lokalen Tresor. Snapshot-Version 3 überträgt nur die Historie des jeweiligen Teams. Jeder Eintrag enthält UUID, Anzeigename, Client-Zeitstempel in Millisekunden, eine typisierte Aktion, Objekttyp, Eintrags-ID/-Name und eine Liste typisierter Feldnamen. Die Ereignisse entstehen bei lokalen Änderungen an Team-Ordnern, Verbindungen und freigegebenen Konten, einschließlich Freigabe/Widerruf. Passwort-, Schlüssel-, Passphrasen- und Notizinhalte sowie alte/neue Geheimniswerte werden nicht in Ereignisse aufgenommen. Persönliche Bindings, private Konten/Einträge und Zugangsdaten-Ablageordner bleiben ausgeschlossen. Speichern unveränderter Werte erzeugt kein Ereignis.

Fachliche Änderungen und Protokoll werden in derselben Tresor-Transaktion gespeichert. Eingehende Historien werden vor jeder Mutation vollständig auf Schema, Größen, IDs und Zeitstempel geprüft. Ein Abruf ersetzt die Team-Historie durch den v3-Serverstand; bestätigtes Verwerfen lokaler Änderungen verwirft auch deren lokale Historie. Version 1/2 bleibt lesbar und erhält bereits bekannte Ereignisse, darf aber selbst keine `activity` einschleusen. Vorherige Änderungen werden nicht nachträglich erfunden. Für die neue Historie alle Clients aktualisieren; ältere Clients verstehen Version 3 nicht. Der Server bleibt ein Ciphertext-Speicher mit Envelope-Version 1; der zusätzliche Benachrichtigungs-Endpunkt benötigt keine Mitglieder-Datenbank.

Anzeigenamen und Geräteuhren sind selbst verwaltet und nicht serverseitig verifiziert. Wer den Team-Schlüssel besitzt, kann Snapshots einschließlich Historie ersetzen. Dies ist eine gemeinsame Änderungsübersicht, kein revisionssicheres oder individuell signiertes Auditlog. Konflikt-/Revisionserkennung und die bestehenden Grenzen gegenüber einem bösartigen Server gelten weiterhin. Die Oberfläche lädt keine zusätzliche Telemetrie und zeigt zunächst 50 Ereignisse mit Suche, Aktionsfilter und weiteren Seiten.

## Nächste sinnvolle Ausbaustufen

1. Ausführung der vorbereiteten Windows-Buildmatrix und Tests gegen ein Linux-Ziel mit echten eingeschränkten sudo-Rechten; benutzerseitiger Abbruch laufender Transfers.
2. Tresor-Passwortwechsel, automatische Sperre und System-Keychain zum optionalen Entsperren.
3. Benutzer-/Gerätemitgliedschaften, Schlüsselaustausch und Rotation; interaktive Auswahl widersprüchlicher Feldwerte.
4. SSH-Konfiguration, Jump Hosts und weitere Authentifizierungsverfahren.
5. Rekursive Transfers, Transferwarteschlange, atomische Remote-Uploads und Dateiverwaltung.
6. RDP-/VNC-Integration und weitere FTP-Kompatibilität (implizites FTPS, ältere Zeichensätze).

## Darstellung

`useAppearance` speichert ausschließlich die validierten IDs von Theme und Textgröße unter `noryn.appearance.v1` im Webview-LocalStorage. Unbekannte oder beschädigte Werte fallen auf Graphit/MD zurück. Die Auswahl wird vor dem Mounten angewendet und steht auch beim Entsperren bereit; sie ist unabhängig vom Tresor und gelangt nie in Team-Snapshots oder deren Historie. Scheitert das Speichern, gilt die Auswahl weiter für die Sitzung und die Einstellungsseite zeigt dies an.

`themes.css` definiert semantische Farbrollen für Flächen, Text, Rahmen, Akzente, Status, Auswahl, Diagramme und 16 ANSI-Farben. Graphit ist die neutrale Vorgabe; Mitternacht, Nord und Licht ändern dieselben Rollen. Schriftgrößen der Oberfläche sind relativ zur Root-Basis (12/13/14/16/18 px); Formularfelder haben keinen separaten Schriftgrößenaufschlag. Nur die dekorative Wort-/Bildmarke bleibt in ihrer festen Größe.

Inter Variable und JetBrains Mono Variable werden über Fontsource aus lokalen WOFF2-Assets gebündelt; es gibt keine externen Font-Anfragen. Die OFL-Lizenztexte werden unter `licenses/` mit ausgeliefert. xterm wartet beim Öffnen auf die lokale Mono-Schrift und übernimmt Theme und Basisgröße auch nachträglich, ohne Sitzung oder Scrollback neu anzulegen. Anschließend wird das Terminal neu vermessen. Hardwaregraphen verwenden dieselben Farbrollen; Darstellung verursacht keine zusätzlichen Serverabfragen.

## Interaktive Systemverwaltung und Layout

`system_control` akzeptiert ausschließlich typisierte Dienst-/Prozessaktionen und Speicherabfragen. Dienstnamen müssen `.service`-Namen ohne Shell-Metazeichen sein; alle Werte werden zusätzlich als getrennte Shell-Argumente gequotet. Prozesssignale erlauben nur positive PIDs größer 1 und prüfen die Startzeit aus `/proc/<pid>/stat` unmittelbar vor dem Signal, um eine ersetzte PID zu erkennen. Zwischen Prüfung und Signal bleibt die übliche kleine PID-Race ohne einen auf dem Ziel installierten pidfd-Helfer bestehen.

Vor jeder Änderung zeigt die Oberfläche Ziel und Aktion. Dienstaktionen nutzen `systemctl --no-ask-password --no-block`; Start/Stop/Restart/Reload quittieren den Auftrag, der endgültige Zustand wird separat abgefragt. Enable/Disable ändert nur den Autostart. Status/letzte Logs und Unit-Dateien werden gezielt gelesen und nur in der Oberfläche angezeigt. Unprivilegierte Leserechte können Logs einschränken; „Mit sudo lesen“ lädt sie ausdrücklich mit erhöhten Rechten.

Ein eigener READY-Marker trennt sudo-Authentifizierung von der Operation. Nur wenn sudo die Operation noch nicht gestartet hat, darf ein Versuch mit dem gespeicherten Benutzerpasswort folgen. NOPASSWD erhält kein Passwort in stdin. Nach READY werden Fehler/Timeouts gemeldet, ohne die Mutation zu wiederholen. Die eigentliche Ausführung hat ein 20-Sekunden-Limit und 256-KiB-Limits pro Ausgabestream; Kanalaufbau und -abschluss besitzen zusätzliche SSH-Zeitlimits. bei Timeout ist der Ausgang ungewiss und die UI verlangt eine Statusprüfung. Kein Python/Zielagent und keine automatische sudoers-Anpassung.

Mounts stammen aus `/proc/self/mountinfo`, einschließlich Quellen, Dateisystemtypen und Optionen mit dekodierten Linux-Pfad-Escapes. `pvs`, `vgs`, `lvs` werden nur auf Wunsch, wenn vorhanden, mit `--readonly --reportformat json` abgefragt. Fehlende Werkzeuge/Rechte werden getrennt angezeigt. LVM-Readonly-Metadaten geben keine verlässliche Aussage über die aktuelle Device-Mapper-Nutzung; es gibt keine Änderung von Partitionen/LVM.

Layoutpräferenzen liegen ausschließlich lokal unter `noryn.layout.v1` im Webview-LocalStorage. Seitenleiste: mindestens 200 px, höchstens 380 px bzw. abhängig von der Fensterbreite, damit Platz für Inhalte bleibt. SSH-Split: mindestens 240 px Terminal und 300 px Dateien (460 px bei gleichzeitig lokaler und entfernter Ansicht); ResizeObserver passt xterm an. Ziehen, Pfeiltasten und Doppelklick-Reset werden unterstützt. Diese Präferenzen gelangen nie in den Team-Snapshot.
