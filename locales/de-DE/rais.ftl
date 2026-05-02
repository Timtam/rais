app-title = REAPER Accessibility Installation Software
app-short-name = RAIS

common-yes = ja
common-no = nein

action-install = Installieren
action-update = Aktualisieren
action-keep = Beibehalten

package-reaper = REAPER
package-osara = OSARA
package-sws = SWS
package-reapack = ReaPack
package-reakontrol = ReaKontrol
package-jaws-scripts = JAWS-Skripte für REAPER

package-reaper-description = Reaper ist eine der vielseitigsten Digital Audio Workstations (DAWs) auf dem Markt und eignet sich sowohl für Einsteiger als auch für Profis. Besonders durch ihre Vielseitigkeit und Anpassungsfähigkeit hebt sich Reaper von anderen DAWs ab und wird von vielen professionellen Produzenten für anspruchsvolle Projekte genutzt.
package-osara-description = OSARA macht REAPER mit Screenreadern bedienbar — NVDA, JAWS und Narrator unter Windows sowie VoiceOver unter macOS. Installieren Sie OSARA, wenn Sie REAPER mit einem Screenreader nutzen.
package-sws-description = Die SWS-Erweiterung ist eine seit langem etablierte Community-Sammlung zusätzlicher Aktionen, Skripte und Hilfsmittel, die das Bearbeiten in REAPER abrunden. Die meisten barrierefreien REAPER-Setups setzen sie voraus.
package-reapack-description = ReaPack ist der Paketmanager für REAPER: Er installiert und aktualisiert Skripte und Erweiterungen Dritter direkt aus REAPER heraus. Installieren Sie ReaPack, wenn Sie von der REAPER-Community geteilte Skripte verwenden möchten.
package-reakontrol-description = ReaKontrol fügt REAPER Unterstützung für Native Instruments Komplete-Kontrol-Tastaturen hinzu. Installieren Sie es, wenn Sie eine Komplete-Kontrol-Tastatur besitzen und Hardware-Steuerung nutzen möchten.
package-jaws-scripts-description = Die JAWS-Skripte für REAPER ergänzen den Screenreader JAWS unter Windows um Skript-Unterstützung für REAPER. RAIS bietet sie nur an, wenn JAWS auf diesem PC erkannt wird.

# $reason is one of the localized "wizard-package-row-unavailable-*" strings
# explaining *why* the row is unavailable. Appended to the row's main summary
# in the package CheckListBox.
wizard-package-row-unavailable-suffix = (nicht verfügbar: { $reason })
wizard-package-row-unavailable-portable = portable REAPER-Installation

detect-installed = Installiert
detect-not-installed = Nicht installiert
detect-version-unknown = Version unbekannt
detect-source-receipt = RAIS-Beleg
detect-source-files = Vorhandene Datei in UserPlugins
detect-source-reapack-registry = ReaPack-Registry

# $package is the localized package display name.
status-package-installed = { $package } installiert

wizard-step-target = Ziel
wizard-step-version-check = Versionsprüfung
wizard-step-packages = Pakete
wizard-step-reapack-acknowledgement = ReaPack-Spendenhinweis
wizard-step-review = Überprüfung
wizard-step-progress = Fortschritt
wizard-step-done = Fertig

# Mnemonic messages are single-character native access keys. Choose a character
# from the translated label when possible.
wizard-button-back = Zurück
wizard-button-back-mnemonic = Z
wizard-button-next = Weiter
wizard-button-next-mnemonic = W
wizard-button-install = Installieren
wizard-button-install-mnemonic = I
wizard-button-close = Schließen
wizard-button-close-mnemonic = S

wizard-target-heading = REAPER-Installation auswählen
wizard-target-language-label = Sprache
wizard-target-language-restart-note = Beim Wechsel der Sprache wird RAIS neu gestartet, damit die neue Sprache wirksam wird.
wizard-locale-name-en-US = Englisch (Vereinigte Staaten)
wizard-locale-name-de-DE = Deutsch (Deutschland)
wizard-target-choice-label = Installationsziel
wizard-target-details-label = Zieldetails
wizard-target-empty = Es ist kein REAPER-Installationsziel ausgewählt.
wizard-target-portable-choice = Portablen REAPER-Ordner installieren oder aktualisieren
wizard-target-portable-folder-label = Portabler Ordner
wizard-target-portable-folder-message = Wählen Sie einen portablen REAPER-Ordner oder einen leeren Ordner für eine neue portable Einrichtung.
wizard-target-portable-pending-details = Wählen Sie zunächst die Option für ein portables Ziel und anschließend einen portablen REAPER-Ordner oder einen leeren Ordner für eine neue portable Einrichtung.
wizard-target-custom-portable-label = Portabler REAPER-Ordner
wizard-target-custom-portable-app-path-label = Pfad der REAPER-Anwendung
wizard-target-custom-portable-path-label = Portabler Ressourcenpfad
wizard-target-custom-portable-version-label = REAPER-Version
wizard-target-custom-portable-writable-label = Beschreibbar
wizard-target-custom-portable-note = RAIS legt das REAPER-Ressourcenlayout hier an, falls es fehlt.

# $version is the REAPER version or an unknown-version label and $path is the resource path.
wizard-target-row = REAPER { $version } unter { $path }

# $app_path is the REAPER application path, $path is the REAPER resource path,
# $version is the REAPER version or an unknown-version label, and $writable
# is yes/no.
wizard-target-details = REAPER-Anwendung: { $app_path }
    REAPER-Version: { $version }
    Ressourcenpfad: { $path }
    Beschreibbar: { $writable }

wizard-packages-heading = Pakete auswählen
wizard-packages-list-label = Zu installierende oder zu aktualisierende Pakete

wizard-reapack-ack-heading = ReaPack-Spendenhinweis
wizard-reapack-ack-body = ReaPack ist freie Software und steht unter der LGPL. Sein Autor Christian Fillion nimmt Spenden zur Unterstützung der Weiterentwicklung an. Spenden sind vollständig freiwillig und für die Nutzung von ReaPack oder RAIS niemals erforderlich.
wizard-reapack-ack-link-label = ReaPack-Spendenseite öffnen
wizard-reapack-ack-confirm-label = Ich habe den obigen Hinweis gelesen und möchte mit der Installation oder Aktualisierung von ReaPack fortfahren
cli-reapack-ack-prompt-summary = ReaPack ist freie Software (LGPL). Spenden an seinen Autor Christian Fillion unter https://reapack.com/donate sind freiwillig und für die Nutzung von ReaPack oder RAIS niemals erforderlich.
cli-reapack-ack-flag-required = ReaPack ist Teil dieses Plans, der Spendenhinweis wurde aber nicht bestätigt. Führen Sie den Befehl erneut mit --accept-reapack-donation-notice aus, um zu bestätigen, dass Sie https://reapack.com/donate gelesen haben und RAIS ReaPack installieren oder aktualisieren soll.

wizard-version-check-heading = Prüfung auf neueste Versionen
wizard-version-check-status-pending = Versionsprüfung wird vorbereitet…
# $package is the localized package display name.
wizard-version-check-status-checking = { $package } wird geprüft…
# $error_count is the number of failed checks.
wizard-version-check-status-error = { $error_count } Versionsprüfung(en) fehlgeschlagen. Wählen Sie „Zurück“, um ein anderes Ziel zu versuchen, oder schließen Sie RAIS.
wizard-version-check-progress-label = Fortschritt
wizard-version-check-error-heading = Fehlgeschlagene Prüfungen
# $package is the localized package display name; $message is the failure message.
wizard-version-check-error-line = { $package }: { $message }
wizard-package-details-label = Paketdetails
wizard-packages-osara-keymap-heading = OSARA-Tastenzuordnung
wizard-packages-osara-keymap-replace-label = Aktuelle Tastenzuordnung durch OSARA-Tastenzuordnung ersetzen
wizard-packages-osara-keymap-unavailable-note = Wählen Sie OSARA aus, um das Verhalten der Tastenzuordnung zu konfigurieren.
wizard-packages-osara-keymap-preserve-note = Die aktuelle Tastenzuordnung wird als nicht standardmäßige Überschreibung beibehalten. RAIS sollte reaper-kb.ini nicht überschreiben.
wizard-packages-osara-keymap-replace-note = RAIS sichert die Datei reaper-kb.ini und ersetzt sie durch die OSARA-Tastenzuordnung. Dies ist die Standardeinstellung.
wizard-package-details-handling-prefix = Behandlung
wizard-package-handling-automatic = RAIS kann dieses Paket direkt installieren.
wizard-package-handling-unattended = RAIS kann dieses Paket unbeaufsichtigt installieren und bei Bedarf das zugehörige Installationsprogramm starten.
wizard-package-handling-planned = RAIS soll das Installationsprogramm bzw. die Einrichtungsroutine dieses Pakets selbst ausführen und die Installation unbeaufsichtigt abschließen, in dieser Version werden jedoch lediglich die erforderlichen Schritte gemeldet.
wizard-package-handling-manual = RAIS lädt dieses Paket herunter und meldet die manuellen Schritte nach dem Durchlauf.
wizard-package-handling-unavailable = Dieses Paket ist für die ausgewählte Plattform oder Architektur nicht verfügbar.

# $package is the localized package display name, $action is the localized planned action, $installed is the installed version or unknown, and $available is the available version or unknown.
wizard-package-row = { $package }: { $action }. Installiert: { $installed }. Verfügbar: { $available }

wizard-review-heading = Änderungen überprüfen
wizard-review-target-prefix = Ziel
wizard-review-package-heading = Ausgewählte Pakete
wizard-review-osara-keymap-heading = OSARA-Tastenzuordnung
wizard-review-osara-keymap-preserve = Aktuelle Tastenzuordnung beibehalten und die OSARA-Tastenzuordnung nicht anwenden.
wizard-review-osara-keymap-replace = Aktuelle Tastenzuordnung nach Sicherung von reaper-kb.ini ersetzen.
wizard-review-notes-heading = Hinweise
wizard-review-preflight-prefix = Installation derzeit nicht möglich

# $path is the selected REAPER resource path.
wizard-review-target = Ziel: { $path }
wizard-review-no-target = Kein Ziel ausgewählt.
wizard-review-no-package = Kein Paket ausgewählt.

# $package is the localized package display name and $action is the localized planned action.
wizard-review-package = { $package }: { $action }

wizard-progress-heading = Installationsfortschritt
wizard-progress-status-idle = Bereit zur Installation.
wizard-progress-status-running = Ausgewählte Pakete werden installiert. Dies kann mehrere Minuten dauern.
wizard-progress-details-label = Fortschrittsdetails
wizard-progress-details-idle = Es läuft keine Installation.
wizard-progress-details-starting = Einrichtungsvorgang wird gestartet.
wizard-progress-details-cache-prefix = Zwischenspeicher

wizard-done-heading = Fertig
wizard-done-status-idle = Aus diesem Fenster wurde noch keine Installation ausgeführt.
wizard-done-status-success = Installation abgeschlossen. Bitte prüfen Sie die Details unten.
wizard-done-status-error = Installation fehlgeschlagen. Bitte prüfen Sie den Fehler unten.
wizard-done-status-no-packages = Es wurde kein Paket zur Installation oder Aktualisierung ausgewählt.
wizard-done-show-details = Details anzeigen
# Mnemonic messages are single-character native access keys. Choose a character
# from the translated label when possible.
wizard-done-launch-reaper = REAPER starten
wizard-done-launch-reaper-mnemonic = R
wizard-done-open-resource = Ressourcenordner öffnen
wizard-done-open-resource-mnemonic = O
wizard-done-rescan = Ziel erneut prüfen
wizard-done-rescan-mnemonic = E
wizard-done-save-report = Bericht speichern
wizard-done-save-report-mnemonic = B
wizard-done-no-reaper-app = Für dieses Ziel ist keine startbare REAPER-Anwendung bekannt.
wizard-done-no-report = Es ist noch kein Einrichtungsbericht verfügbar.
wizard-done-report-saved-prefix = Bericht gespeichert
wizard-done-report-save-error-prefix = Bericht konnte nicht gespeichert werden
wizard-done-launch-reaper-error-prefix = REAPER konnte nicht gestartet werden
wizard-done-open-resource-error-prefix = Ressourcenordner konnte nicht geöffnet werden
wizard-done-rescan-error-prefix = Ziel konnte nicht erneut geprüft werden
wizard-done-self-update-apply = RAIS-Aktualisierung anwenden
wizard-done-self-update-apply-mnemonic = A
wizard-done-self-update-apply-running = RAIS-Aktualisierung wird angewendet…
wizard-done-self-update-error-prefix = RAIS-Selbstaktualisierung fehlgeschlagen
wizard-done-self-update-relaunch-prefix = RAIS neu gestartet
wizard-self-update-status-checking = Suche nach RAIS-Aktualisierungen…

# $current is the running RAIS version, $latest is the version offered by the
# release manifest, $channel is the release channel id (e.g. "stable").
self-update-status-update-available = RAIS-Aktualisierung verfügbar: { $current } → { $latest } (Kanal { $channel }). Klicken Sie auf „RAIS-Aktualisierung anwenden“ zum Installieren.
self-update-status-up-to-date = RAIS ist auf dem neuesten Stand (aktuell { $current }, Kanal { $channel }).

# $version is the version that the apply pipeline targeted but did not write.
self-update-apply-no-files-replaced = Selbstaktualisierung hat keine Dateien ersetzt (Zielversion { $version }).
# $count is the number of files swapped on disk, $root is the install directory,
# $version is the new RAIS version that is now in place.
self-update-apply-replaced-summary = { $count } Datei(en) unter { $root } ersetzt; starten Sie RAIS neu, um { $version } zu verwenden.

# $signed / $unsigned are counts of binaries that produced each verdict.
self-update-apply-signature-summary-signed-only = Signaturüberprüfung: { $signed } signiert.
self-update-apply-signature-summary-unsigned-only = Signaturüberprüfung: { $unsigned } unsigniert.
self-update-apply-signature-summary-mixed = Signaturüberprüfung: { $signed } signiert, { $unsigned } unsigniert.

# $pid is the OS process id of the other RAIS install holding the lock.
self-update-lock-blocking = Eine andere RAIS-Installation läuft bereits (PID { $pid }). Anwenden ist angehalten, bis sie abgeschlossen ist.

# Summary and report lines shown in the wizard progress/done views and saved outcome reports.
wizard-summary-target = Ziel: { $path }
wizard-summary-portable = Portables Ziel: { $value }
wizard-summary-dry-run = Probelauf: { $value }
wizard-summary-packages-selected = Ausgewählte Pakete: { $packages }
wizard-summary-cache = Zwischenspeicher: { $path }
wizard-summary-planned-app = Geplanter Anwendungspfad: { $path }
wizard-summary-error = Fehler: { $message }
wizard-summary-resource-items-created = Angelegte Ressourceneinträge: { $count }
wizard-summary-packages-installed-or-checked = Installierte oder geprüfte Pakete: { $count }
wizard-summary-packages-current = Bereits aktuelle Pakete: { $count }
wizard-summary-packages-manual = Pakete, die manuelle Aufmerksamkeit benötigen: { $count }
wizard-summary-backup-files-created = Angelegte Sicherungsdateien: { $count }
wizard-summary-backup-file = Sicherungsdatei: { $path }
wizard-summary-receipt-backup = Beleg-Sicherung: { $path }
wizard-summary-backup-manifest = Sicherungsmanifest: { $path }
wizard-summary-package-message = { $package }: { $message }
# $action is one of the localized "action-*" labels (Installieren/Aktualisieren/Beibehalten).
wizard-summary-package-plan-action =   Plan-Aktion: { $action }
# $status is one of the localized "status-*" labels.
wizard-summary-package-status =   Status: { $status }
# $version is the version RAIS just installed (or confirmed already current).
wizard-summary-package-installed-version =   Installierte Version: { $version }
# $architecture is the detected REAPER architecture (x64, arm64, …).
wizard-summary-architecture = Architektur: { $architecture }
status-installed-or-checked = Installiert oder geprüft
status-planned-unattended = Unbeaufsichtigt geplant
status-deferred-unattended = Unbeaufsichtigt verschoben
status-skipped-current = Übersprungen (bereits aktuell)
wizard-summary-planned-execution-title = Geplante unbeaufsichtigte Ausführung:
wizard-summary-planned-execution-runner =   Ausführer: { $runner }
wizard-summary-planned-execution-artifact =   Artefakt: { $artifact }
wizard-summary-planned-execution-program =   Programm: { $program }
wizard-summary-planned-execution-arguments =   Argumente: { $arguments }
wizard-summary-planned-execution-working-directory =   Arbeitsverzeichnis: { $path }
wizard-summary-planned-execution-verify =   Prüfen: { $path }
wizard-summary-manual-title = { $title }:
wizard-summary-manual-step =   { $step }
wizard-summary-manual-note =   Hinweis: { $note }
wizard-summary-status-finished = Abgeschlossen. { $installed } Paketeintrag/Paketeinträge installiert oder geprüft; { $manual } benötigen manuelle Aufmerksamkeit.

wizard-planned-runner-launch-installer = Installationsprogramm ausführen
wizard-planned-runner-extract-archive = Archiv entpacken und enthaltenes Installationsprogramm ausführen
wizard-planned-runner-extract-archive-copy-osara = Archiv entpacken und OSARA-Installationsdateien kopieren
wizard-planned-runner-mount-disk-image = Image einhängen und enthaltenes Installationsprogramm ausführen
wizard-planned-runner-mount-disk-image-copy-app = Image einhängen und enthaltenes Anwendungspaket kopieren
