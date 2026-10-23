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

`TeamSnapshot` ist eine eigene, streng deserialisierte Struktur mit ausschließlich `version`, `teamId` und `nodes`. Es wird niemals der gesamte Tresor serialisiert, um ihn zu synchronisieren. Zugangsdaten, Zuordnungen, Hostschlüssel-Pins, Sync-Token, Team-Schlüssel, andere Teams und private Einträge sind ausgeschlossen. Ein Abruf validiert den gesamten resultierenden Baum vor dem Ersetzen. Fremde Workspace-Einträge und ID-Kollisionen werden abgelehnt.

## Tresor

- Ein zufälliges 16-Byte-Salz pro Tresor.
- Argon2id v0x13 mit 64 MiB Speicher, drei Durchläufen, einer Lane und 32-Byte-Schlüssel.
- XChaCha20-Poly1305 mit neuer zufälliger 24-Byte-Nonce für jeden Schreibvorgang.
- Authentifizierte Kontextbindung `harbor:vault:v1`.
- Schreiben über temporäre Datei im selben Verzeichnis, `sync_all` und atomisches Ersetzen. Unix: Verzeichnis `0700`, Datei `0600`.
- Änderungstransaktionen setzen bei Fehlern den vorherigen Speicherzustand wieder ein.
- Schlüssel, Geheimnisse und entschlüsselte Serialisierungsbuffer werden mit `zeroize` bestmöglich bereinigt. Vollständiger Schutz gegen Swap, Prozess-Dumps oder einen kompromittierten Desktop ist dadurch nicht gegeben.

Das Frontend erhält ausschließlich `VaultView` mit Verbindungsmetadaten, Kontonamen/-benutzernamen und persönlichen Zuordnungen. Passwort, private Schlüssel, Schlüssel-Passphrasen und Sync-Geheimnisse werden nach dem Speichern nicht zurückgegeben. Browser-Vorschau und native App verwenden dieselbe Oberfläche; die Vorschau arbeitet nur im Speicher.

Sperren schließt registrierte Sitzungen, verwirft den entschlüsselten Rust-Tresor und leert den Frontend-Status. Noch laufende blockierende Netzwerk-/Dateioperationen können erst bei Abschluss oder Timeout freigegeben werden. Es gibt noch keine automatische Sperre bei Inaktivität oder Betriebssystem-Sperre.

## SSH, Terminal und Dateien

`ssh2`/libssh2 bildet den nativen Transport auf Linux und Windows. Vendortes OpenSSL ermöglicht Schlüssel aus verschlüsseltem Speicher auch unter Windows. Für die Anmeldung werden Passwort, privater Schlüssel mit optionaler Passphrase und lokaler SSH-Agent unterstützt. Agent-Verfügbarkeit ist plattformabhängig; zusätzliche Verfahren wie Keyboard-Interactive/MFA und FIDO2 sind noch offen.

Beim ersten Kontakt wird nur ein SSH-Handshake ausgeführt. Vor der Anmeldung muss der Nutzer den SHA-256-Hostschlüssel bestätigen. Geänderte Schlüssel erzeugen eine gesonderte Warnung. Der beim tatsächlichen Verbindungsversuch ausgehandelte Schlüssel wird erneut verglichen; auch der zusätzliche Dateitransport muss denselben Pin erfüllen. Pins bleiben lokal. Eine Unterstützung von `known_hosts`-Dateien und Host-Zertifikaten ist noch offen.

Pro Sitzung verwendet das MVP zwei getrennt authentifizierte SSH-Transporte zum selben Host:

1. Ein nicht blockierender PTY-Kanal für das Terminal. Ein Rust-Thread verarbeitet Ein-/Ausgabe, partielle Schreibvorgänge, Größenänderungen und Verbindungsende. Bytes werden ohne verlustbehaftete UTF-8-Konvertierung an xterm.js übertragen.
2. Ein seriell verwendeter Dateitransport mit SFTP bzw. zusätzlichen Exec-Kanälen für den sudo-Dateimodus. So blockieren größere Transfers nicht den Terminal-Transport.

Die SFTP-Funktionen unterstützen Einzeldateien und Ordnererstellung. Downloads schreiben zunächst in eine temporäre lokale Datei. Uploads erstellen Remote-Dateien exklusiv, damit bestehende Dateien nicht still überschrieben werden. Abgebrochene Uploads können eine unvollständige Remote-Datei hinterlassen; Resume und automatisches Cleanup sind noch offen. Dateiverknüpfungen werden angezeigt, rekursive Verzeichnisübertragungen sind noch nicht implementiert.

### sudo-Dateimodus

Eine SFTP-Sitzung kann nicht nachträglich mit `sudo` ihren Benutzer wechseln. Harbor öffnet im sudo-Modus stattdessen Exec-Kanäle auf dem Datei-SSH-Transport und führt einen fest eingebauten Python-3-Helfer über `sudo` aus.

- Ohne Passwort: `sudo -n -- python3 ...`.
- Mit Passwort: `sudo -S -p '' -- python3 ...`; das Passwort wird über stdin übertragen, niemals im Kommando oder in Argumenten.
- Pfade und Operationen sind Base64-kodiertes JSON. Shell-Argumente werden zusätzlich gequotet; Remote-Pfade werden nicht in ausführbaren Code eingesetzt.
- Nur Auflisten, Einzeldateien lesen/schreiben, Ordner erstellen und Prüfung der Root-Identität sind implementiert.
- Der Wechsel wird erst nach erfolgreicher Rechteprüfung aktiviert. Bei Fehler bleibt der vorherige Modus erhalten.
- Das sudo-Passwort bleibt nur im Speicher der Dateisitzung. Es wird nach Trennen verworfen. Der Dateibrowser behält seinen Modus auch beim Ein-/Ausblenden.

Das ist eine erste Umsetzung des SSH-basierten Dateizugriffs, keine vollständige XPipe-Kompatibilität. Ein restriktives sudo-Setup kann einen fest installierten, separat auditierten Helfer und eine entsprechend begrenzte sudoers-Regel verwenden. Das MVP setzt vorhandene sudo-Rechte voraus und verändert keine sudoers-Konfiguration.

## Verschlüsseltes Sync-Protokoll

Der Team-Schlüssel besteht aus 32 zufälligen Bytes, als Base64 geteilt. Er ist vom lokalen Tresor-Passwort und dem Server-Token unabhängig. Jeder Upload verschlüsselt einen vollständigen Team-Snapshot mit einer neuen Nonce und Kontext `harbor:team:v1:<team-id>`.

```text
Client → TeamSnapshot → XChaCha20-Poly1305 → HTTPS → Server
Client ← validierter Baum ← Entschlüsselung ← HTTPS ← Server
```

`GET /v1/teams/{id}/snapshot` liefert einen verschlüsselten Envelope und eine `ETag`-Revision. `PUT` erwartet bei neuen Snapshots `If-None-Match: *`, bei vorhandenen `If-Match: <revision>`. Eine veraltete Revision liefert `412 Precondition Failed`. Authentifizierung: `Authorization: Bearer <team-token>`. Nicht konfigurierte Teams und falsche Tokens liefern `401`.

Der Server speichert ausschließlich Envelope, Nonce und Versionsnummer. Er kennt aber Team-IDs, Zugriffsmuster, Größe und Zeitpunkt von Snapshots. Token-Vergleich erfolgt über SHA-256-Hashes mit zeitkonstantem Vergleich. Ein globaler Prozess-Mutex serialisiert Schreiben und Revisionsprüfung; ein gemeinsames Datenverzeichnis über mehrere Serverprozesse wird nicht unterstützt.

Transportseitig werden ausschließlich HTTPS oder explizite lokale HTTP-Endpunkte akzeptiert. Zertifikatsprüfung bleibt aktiv, HTTP-Redirects sind abgeschaltet. Anfragen haben Timeouts, Server und Client begrenzen die Snapshot-Größe. Authentifizierte Verschlüsselung erkennt falsche Schlüssel, Inhaltsmanipulation und Vertauschen von Teams. Sie verhindert allein keine Wiedergabe einer älteren gültigen Snapshot-Version durch einen bösartigen Server. Ein monotones, signiertes Änderungsprotokoll und Rollback-Erkennung gehören in eine spätere Version.

Abrufen ersetzt nach Bestätigung den lokalen Team-Baum. Es gibt noch kein automatisches Merge, kein CRDT, keine Echtzeit-Übertragung, keine Mitgliederverwaltung und keinen automatischen Schlüsselwiderruf. Nach Verlust eines Team-Schlüssels oder Austritt eines Mitglieds muss eine geregelte Schlüsselrotation außerhalb des aktuellen MVP erfolgen.

## Nächste sinnvolle Ausbaustufen

1. Integrationstests mit einem Windows-Runner und einem Linux-Ziel mit eingeschränktem sudo; Abbruch/Timeouts für Transfers und Verbindungen.
2. Verschlüsselte Sicherungen, Tresor-Passwortwechsel, automatische Sperre und System-Keychain zum optionalen Entsperren.
3. Benutzer-/Gerätemitgliedschaften, Schlüsselaustausch und Rotation; Hintergrundsync mit sichtbarer Konfliktauflösung.
4. SSH-Konfiguration, Jump Hosts, Tunnel und weitere Authentifizierungsverfahren.
5. Rekursive Transfers, Transferwarteschlange, atomische Remote-Uploads und Dateiverwaltung.
6. RDP-/VNC-Integration und FTPS. Unverschlüsseltes FTP nur als explizit gekennzeichneter Legacy-Modus.
