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

`ssh2`/libssh2 bildet den nativen Transport auf Linux und Windows. Vendortes OpenSSL ermöglicht Schlüssel aus verschlüsseltem Speicher auch unter Windows. Für die Anmeldung werden Passwort, privater Schlüssel mit optionaler Passphrase, lokaler SSH-Agent und Keyboard-Interactive-Abfragen (z. B. OTP) unterstützt. Schlüssel und Benutzerpasswort können in einem Konto zusammen gespeichert werden. Antworten auf Serverabfragen werden nur im laufenden Anmeldedialog verarbeitet. Der Dialog unterstützt mehrere und aufeinanderfolgende Fragen, Abbruch und ein Zeitlimit. Agent-Verfügbarkeit ist plattformabhängig; FIDO2 ist noch offen.

Beim ersten Kontakt wird nur ein SSH-Handshake ausgeführt. Vor der Anmeldung muss der Nutzer den SHA-256-Hostschlüssel bestätigen. Geänderte Schlüssel erzeugen eine gesonderte Warnung. Der beim tatsächlichen Verbindungsversuch ausgehandelte Schlüssel wird erneut verglichen; auch der zusätzliche Dateitransport muss denselben Pin erfüllen. Pins bleiben lokal. Eine Unterstützung von `known_hosts`-Dateien und Host-Zertifikaten ist noch offen.

Pro Sitzung verwendet das MVP einen authentifizierten SSH-Transport mit getrennten Kanälen. Die Anmeldung einschließlich OTP erfolgt einmal. Bei einer SSH-Verbindung verarbeitet ein Rust-Thread den PTY-Kanal für das Terminal. Bei einer reinen SFTP-Verbindung wird kein Shell-/PTY-Kanal angefordert.

Der Dateizugriff verwendet SFTP- und bei sudo zusätzliche Exec-Kanäle auf demselben Transport. Die libssh2-Sitzung bleibt nicht blockierend; Dateioperationen behandeln EAGAIN mit begrenzten Wiederholungen. Dadurch kann das Terminal auch während Dateitransfers Daten verarbeiten. Das Session-Objekt wird intern durch libssh2-rs synchronisiert, Dateioperationen werden zusätzlich pro Verbindung serialisiert.

Die Oberfläche verwendet einen einzigen zentralen Verbindungsbaum. Lokale und entfernte Dateibereiche bieten Verlauf, Pfadsegmente, direkte Eingabe und eine Transferwarteschlange. Interne Datei-Drags verwenden Pointer-Events, weil Tauris nativer Datei-Drop-Handler auf Windows HTML5-DnD ersetzt. Externe Datei-Drops verwenden weiterhin Tauris native Ereignisse. Diese Kombination erhält beide Transferwege.

Lokale Verzeichnisse werden im Rust-Backend gelesen. Windows-Laufwerke werden angeboten, Pfad-Trennzeichen werden vom Backend geliefert. Downloads aus direkten Drags ersetzen keine bestehenden Dateien automatisch; ein erneuter Versuch mit Ersetzen benötigt eine explizite Bestätigung.

Die SFTP-Funktionen unterstützen Einzeldateien und Ordnererstellung. Downloads schreiben zunächst in eine temporäre lokale Datei. Uploads erstellen Remote-Dateien exklusiv, damit bestehende Dateien nicht still überschrieben werden. Abgebrochene Uploads können eine unvollständige Remote-Datei hinterlassen; Resume und automatisches Cleanup sind noch offen. Dateiverknüpfungen werden angezeigt, rekursive Verzeichnisübertragungen sind noch nicht implementiert.

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

1. Ausführung der vorbereiteten Windows-Buildmatrix und Tests gegen ein Linux-Ziel mit echten eingeschränkten sudo-Rechten; benutzerseitiger Abbruch laufender Transfers.
2. Verschlüsselte Sicherungen, Tresor-Passwortwechsel, automatische Sperre und System-Keychain zum optionalen Entsperren.
3. Benutzer-/Gerätemitgliedschaften, Schlüsselaustausch und Rotation; Hintergrundsync mit sichtbarer Konfliktauflösung.
4. SSH-Konfiguration, Jump Hosts, Tunnel und weitere Authentifizierungsverfahren.
5. Rekursive Transfers, Transferwarteschlange, atomische Remote-Uploads und Dateiverwaltung.
6. RDP-/VNC-Integration und FTPS. Unverschlüsseltes FTP nur als explizit gekennzeichneter Legacy-Modus.
