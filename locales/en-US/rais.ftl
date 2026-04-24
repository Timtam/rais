app-title = REAPER Accessibility Installation Software
app-short-name = RAIS

common-yes = yes
common-no = no

action-install = Install
action-update = Update
action-keep = Keep
action-review = Review manually

package-reaper = REAPER
package-osara = OSARA
package-sws = SWS Extension
package-reapack = ReaPack

detect-installed = Installed
detect-not-installed = Not installed
detect-version-unknown = Version unknown
detect-architecture-unknown = Architecture unknown
detect-source-receipt = RAIS receipt
detect-source-files = UserPlugins file presence
detect-source-reapack-registry = ReaPack registry

# $package is the localized package display name.
status-package-installed = { $package } installed

wizard-step-target = Target
wizard-step-packages = Packages
wizard-step-review = Review
wizard-step-progress = Progress
wizard-step-done = Done

# Mnemonic messages are single-character native access keys. Choose a character
# from the translated label when possible.
wizard-button-back = Back
wizard-button-back-mnemonic = B
wizard-button-next = Next
wizard-button-next-mnemonic = N
wizard-button-install = Install
wizard-button-install-mnemonic = I
wizard-button-close = Close
wizard-button-close-mnemonic = C

wizard-target-heading = Choose REAPER installation
wizard-target-choice-label = Installation target
wizard-target-details-label = Target details
wizard-target-empty = No REAPER installation target is selected.
wizard-target-portable-choice = Install or update a portable REAPER folder
wizard-target-portable-folder-label = Portable folder
wizard-target-portable-folder-message = Choose a portable REAPER folder, or an empty folder for a new portable setup.
wizard-target-portable-pending-details = Choose the portable target option, then choose a portable REAPER folder or an empty folder for a new portable setup.
wizard-target-custom-portable-label = Portable REAPER folder
wizard-target-custom-portable-app-path-label = REAPER application path
wizard-target-custom-portable-path-label = Portable resource path
wizard-target-custom-portable-version-label = REAPER version
wizard-target-custom-portable-architecture-label = Architecture
wizard-target-custom-portable-writable-label = Writable
wizard-target-custom-portable-note = RAIS will create the REAPER resource layout here if it is missing.

# $kind is the detected installation kind, $version is the REAPER version or an unknown-version label, and $path is the resource path.
wizard-target-row = { $kind } REAPER { $version } at { $path }

# $app_path is the REAPER application path, $path is the REAPER resource path,
# $version is the REAPER version or an unknown-version label, $architecture is the
# REAPER architecture or an unknown-architecture label, $writable is yes/no, and
# $confidence is the detection confidence.
wizard-target-details = REAPER application path: { $app_path }
    REAPER version: { $version }
    Architecture: { $architecture }
    Resource path: { $path }
    Writable: { $writable }
    Detection confidence: { $confidence }

wizard-packages-heading = Choose packages
wizard-packages-list-label = Packages to install or update
wizard-package-details-label = Package details
wizard-packages-osara-keymap-heading = OSARA key map
wizard-packages-osara-keymap-replace-label = Replace current key map with OSARA key map
wizard-packages-osara-keymap-unavailable-note = Select OSARA to configure its key map behavior.
wizard-packages-osara-keymap-preserve-note = The current key map will be preserved as a non-default override. RAIS should not overwrite reaper-kb.ini.
wizard-packages-osara-keymap-replace-note = RAIS will back up and replace reaper-kb.ini with the OSARA key map. This is the default.
wizard-package-details-handling-prefix = Handling
wizard-package-handling-automatic = RAIS can install this package directly.
wizard-package-handling-unattended = RAIS can install this package unattended, including launching its installer when required.
wizard-package-handling-planned = RAIS is designed to run this package's installer or setup routine itself and finish the installation unattended, but this build still reports the steps instead of executing them.
wizard-package-handling-manual = RAIS will download this package and report the manual steps after the run.
wizard-package-handling-unavailable = This package is not available for the selected platform or architecture.

# $package is the localized package display name, $action is the localized planned action, $installed is the installed version or unknown, and $available is the available version or unknown.
wizard-package-row = { $package }: { $action }. Installed: { $installed }. Available: { $available }

wizard-review-heading = Review changes
wizard-review-target-prefix = Target
wizard-review-cache-prefix = Cache
wizard-review-resource-heading = Resource setup
wizard-review-resource-create-directory-prefix = Create directory
wizard-review-resource-create-file-prefix = Create file
wizard-review-resource-no-changes = No resource path changes are needed.
wizard-review-backup-heading = Backups expected
wizard-review-backup-file-prefix = Back up file
wizard-review-backup-no-changes = No backup files are currently expected.
wizard-review-admin-heading = Administrator prompts expected
wizard-review-admin-no-prompts = No administrator prompt is currently expected.
wizard-review-admin-app-prefix = Administrator approval may be required for the REAPER application path
wizard-review-admin-resource-prefix = Administrator approval may be required for the selected resource path
wizard-review-package-heading = Selected packages
wizard-review-osara-keymap-heading = OSARA key map
wizard-review-osara-keymap-preserve = Preserve the current key map instead of applying the OSARA key map.
wizard-review-osara-keymap-replace = Replace the current key map after backing up reaper-kb.ini.
wizard-review-notes-heading = Notes
wizard-review-preflight-prefix = Cannot install yet
wizard-review-manual-heading = Manual attention expected

# $path is the selected REAPER resource path.
wizard-review-target = Target: { $path }
wizard-review-no-target = No target selected.
wizard-review-no-package = No package selected.

# $package is the localized package display name and $action is the localized planned action.
wizard-review-package = { $package }: { $action }

wizard-progress-heading = Installation progress
wizard-progress-status-idle = Ready to install.
wizard-progress-status-running = Installing selected packages. This can take several minutes.
wizard-progress-details-label = Progress details
wizard-progress-details-idle = No installation is running.
wizard-progress-details-starting = Starting setup operation.
wizard-progress-details-cache-prefix = Cache

wizard-done-heading = Done
wizard-done-status-idle = No installation has been run from this window yet.
wizard-done-status-success = Installation finished. Review the details below.
wizard-done-status-error = Installation failed. Review the error below.
wizard-done-status-no-packages = No package was selected for installation or update.
# Mnemonic messages are single-character native access keys. Choose a character
# from the translated label when possible.
wizard-done-launch-reaper = Launch REAPER
wizard-done-launch-reaper-mnemonic = L
wizard-done-open-resource = Open resource folder
wizard-done-open-resource-mnemonic = O
wizard-done-rescan = Rescan target
wizard-done-rescan-mnemonic = R
wizard-done-save-report = Save report
wizard-done-save-report-mnemonic = S
wizard-done-no-reaper-app = No launchable REAPER application is known for this target.
wizard-done-no-report = No setup report is available yet.
wizard-done-report-saved-prefix = Report saved
wizard-done-report-save-error-prefix = Report could not be saved
wizard-done-launch-reaper-error-prefix = REAPER could not be launched
wizard-done-open-resource-error-prefix = Resource folder could not be opened
wizard-done-rescan-error-prefix = Target could not be rescanned

# Summary and report lines shown in the wizard progress/done views and saved outcome reports.
wizard-summary-target = Target: { $path }
wizard-summary-portable = Portable target: { $value }
wizard-summary-dry-run = Dry run: { $value }
wizard-summary-packages-selected = Packages selected: { $packages }
wizard-summary-cache = Cache: { $path }
wizard-summary-planned-app = Planned app path: { $path }
wizard-summary-error = Error: { $message }
wizard-summary-resource-items-created = Resource items created: { $count }
wizard-summary-packages-installed-or-checked = Packages installed or checked: { $count }
wizard-summary-packages-current = Packages already current: { $count }
wizard-summary-packages-manual = Packages requiring manual attention: { $count }
wizard-summary-backup-files-created = Backup files created: { $count }
wizard-summary-backup-file = Backup file: { $path }
wizard-summary-receipt-backup = Receipt backup: { $path }
wizard-summary-backup-manifest = Backup manifest: { $path }
wizard-summary-package-message = { $package }: { $message }
wizard-summary-planned-execution-title = Planned unattended execution:
wizard-summary-planned-execution-runner =   Runner: { $runner }
wizard-summary-planned-execution-artifact =   Artifact: { $artifact }
wizard-summary-planned-execution-program =   Program: { $program }
wizard-summary-planned-execution-arguments =   Arguments: { $arguments }
wizard-summary-planned-execution-working-directory =   Working directory: { $path }
wizard-summary-planned-execution-verify =   Verify: { $path }
wizard-summary-manual-title = { $title }:
wizard-summary-manual-step =   { $step }
wizard-summary-manual-note =   Note: { $note }
wizard-summary-status-finished = Finished. { $installed } package item(s) installed or checked; { $manual } require manual attention.

wizard-planned-runner-launch-installer = Launch installer executable
wizard-planned-runner-extract-archive = Extract archive and run contained installer
wizard-planned-runner-mount-disk-image = Mount disk image and run contained installer
