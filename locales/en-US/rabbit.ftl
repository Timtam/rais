app-title = REAPER Accessibility Bootstrap & Bundle Installation Tool
app-short-name = RABBIT

common-yes = yes
common-no = no

action-install = Will install
action-update = Will update
action-keep = Won't touch

package-reaper = REAPER
package-osara = OSARA
package-sws = SWS Extension
package-reapack = ReaPack
package-reakontrol = ReaKontrol
package-jaws-scripts = Snowman's JAWS Scripts for REAPER
package-ffmpeg = FFmpeg for Improved Video Support

package-reaper-description = REAPER is the digital audio workstation that everything else here builds on. RABBIT can install or update REAPER for you.
package-osara-description = OSARA is the open source accessibility extension that makes REAPER usable with screen readers. NVDA, JAWS and Narrator on Windows, VoiceOver on macOS are all widely adopted, some other Windows screen readers might also work. Install OSARA if you rely on a screen reader to use REAPER.
package-sws-description = The SWS Extension is a long-running community-authored pack of extra actions, scripts, and helpers that extend REAPER's features. For the most accessible REAPER setup, whether you're on Windows or Mac, you should install SWS alongside OSARA.
package-reapack-description = ReaPack is an open source package manager. It can be used to search, install, track and update third-party scripts and extensions from inside REAPER itself. Install this if you want to use scripts shared by the REAPER community.
package-reakontrol-description = ReaKontrol provides open source integration for Native Instruments Komplete Kontrol keyboards. Install this if you have an S series MK2, A series, M-32 or Kontrol MK3 keyboard.
package-jaws-scripts-description = Snowman's scripts improve how JAWS handles various windows throughout REAPER, as well as offering extended Braille support and many other features. Note that these scripts are intended to be used alongside OSARA, they're not an alternative to it. For optimal accessibility with JAWS, you should install both.
package-ffmpeg-description = FFmpeg's shared runtime libraries enable REAPER's video decoder to import and play back common video and audio formats. RABBIT installs the BtbN GPL-shared build's bin folder into UserPlugins; the patch level isn't recoverable from the DLL filenames alone, so externally-installed FFmpegs are reported with a `<major>.0.0` placeholder.

# $reason is one of the localized "wizard-package-row-unavailable-*" strings
# explaining *why* the row is unavailable. Appended to the row's main summary
# in the package CheckListBox.
wizard-package-row-unavailable-suffix = (not available: { $reason })
wizard-package-row-unavailable-portable = portable REAPER target

detect-installed = Installed
detect-not-installed = Not installed
detect-version-unknown = Unknown version
detect-source-receipt = RABBIT receipt
detect-source-files = UserPlugins file presence
detect-source-reapack-registry = ReaPack registry

# $package is the localized package display name.
status-package-installed = { $package } installed

wizard-step-target = Target
wizard-step-version-check = Version check
wizard-step-packages = Packages
wizard-step-reapack-acknowledgement = ReaPack donation
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

wizard-target-heading = Choose a task
wizard-target-language-label = Language
wizard-target-language-restart-note = Changing the language restarts RABBIT so the new language can take effect.
wizard-locale-name-en-US = English (United States)
wizard-locale-name-de-DE = German (Germany)
wizard-target-choice-label = Installation Path
wizard-target-details-label = Target details
wizard-target-empty = No REAPER installation target is selected.
wizard-target-portable-choice = Make or update a portable version of REAPER
wizard-target-portable-folder-label = Portable folder
wizard-target-portable-folder-message = Choose a portable REAPER folder if you already have one, or an empty folder if making a new portable version.
wizard-target-portable-folder-browse-label = Browse…
wizard-target-portable-pending-details = Use the Browse button to set the location of an existing portable version if you have one, or to choose an empty folder if you want to make a new portable version of REAPER.
wizard-target-custom-portable-label = Portable REAPER folder
wizard-target-custom-portable-app-path-label = REAPER application path
wizard-target-custom-portable-path-label = Portable resource path
wizard-target-custom-portable-version-label = REAPER version
wizard-target-custom-portable-writable-label = Writable
wizard-target-custom-portable-note = RABBIT will create the REAPER resource path here if it is missing.

# $version is the REAPER version or an unknown-version label and $path is the resource path.
wizard-target-row = REAPER { $version } in { $path }

# $app_path is the REAPER application path, $path is the REAPER resource path,
# $version is the REAPER version or an unknown-version label, and $writable
# is yes/no.
wizard-target-details = REAPER installation path: { $app_path }
    Version: { $version }
    Resource path: { $path }
    Writable: { $writable }

wizard-packages-heading = Choose packages
wizard-packages-list-label = Packages to install or update
wizard-packages-tree-group-label = Packages
wizard-configuration-tree-group-label = Configuration
# $package is the localized package name the configuration step depends on.
wizard-configuration-row-unavailable = Unavailable: requires { $package } to be installed.
wizard-configuration-row-already-applied = Already applied on this REAPER target.
# Short status tag appended in parentheses to a configuration row's tree label
# when the row isn't actionable. Kept terse so the tree label stays readable;
# the longer sentence in `wizard-configuration-row-unavailable` /
# `wizard-configuration-row-already-applied` is still surfaced in the details
# pane and as the row's accessible reason.
# $reason is one of the "wizard-configuration-row-status-*" strings below.
wizard-configuration-row-summary-suffix = ({ $reason })
# $package is the localized name of the dependency package.
wizard-configuration-row-status-requires = requires { $package }
wizard-configuration-row-status-already-applied = already applied
config-reapack-reaper-accessibility-name = Add Toni's REAPER Accessibility ReaPack repository
config-reapack-reaper-accessibility-description = Adds Ttoni Barth's REAPER Accessibility ReaPack repository (https://github.com/Timtam/reapack/raw/master/index.xml). Once added, go to the Extensions menu, ReaPack, Browse Packages to get extra accessible scripts and plug-ins.

wizard-reapack-ack-heading = ReaPack donation notice
wizard-reapack-ack-body = ReaPack is free software released under the LGPL. Its author Christian Fillion accepts optional donations to support continued development. Christian also maintains the SWS extensions and has landed code specifically to improve compatibility with OSARA in the past. Any support you can send has been well earned.
wizard-reapack-ack-link-label = Open the ReaPack donation page
wizard-reapack-ack-confirm-label = Skip donation this time, just install or update ReaPack
cli-reapack-ack-prompt-summary = ReaPack is free software (LGPL). Donations to its author Christian Fillion accepts optional donations at https://reapack.com/donate to support ongoing development.
cli-reapack-ack-flag-required = ReaPack is in this run's plan but the donation acknowledgement is missing. Re-run with --accept-reapack-donation-notice to confirm you have read https://reapack.com/donate and want RABBIT to install or update ReaPack.

wizard-version-check-heading = Checking latest versions
wizard-version-check-status-pending = Preparing latest-version check…
# $package is the localized package display name.
wizard-version-check-status-checking = Checking { $package }…
# $error_count is the number of failed checks.
wizard-version-check-status-error = { $error_count } version check(s) failed. Use Back to try a different target, or close RABBIT.
wizard-version-check-progress-label = Progress
wizard-version-check-error-heading = Failed checks
# $package is the localized package display name; $message is the failure message.
wizard-version-check-error-line = { $package }: { $message }
wizard-package-details-label = Package details
wizard-packages-osara-keymap-heading = OSARA key map
wizard-packages-osara-keymap-replace-label = Replace your current key map with latest OSARA key map
wizard-packages-osara-keymap-unavailable-note = Select OSARA to configure its key map behavior.
wizard-packages-osara-keymap-preserve-note = For advanced users: your current key map will be preserved. Rabbit won't touch reaper-kb.ini, you will need to manage staying up to date with the latest OSARA key map additions manually.
wizard-packages-osara-keymap-replace-note = Recommended for new through intermediate users: RABBIT will backup a copy of your current reaper-kb.ini file, then replace it with the latest version of the OSARA key map.
wizard-package-details-handling-prefix = Handling
wizard-package-handling-automatic = RABBIT can install this package directly.
wizard-package-handling-unattended = RABBIT can install this package unattended, including launching its installer when required.
wizard-package-handling-planned = RABBIT is designed to run this package's installer or setup routine itself and finish the installation unattended, but this build still reports the steps instead of executing them.
wizard-package-handling-manual = RABBIT will download this package and report the manual steps after the run.
wizard-package-handling-unavailable = This package is not available for the selected platform or architecture.

# $package is the localized package display name, $action is the localized planned action, $installed is the installed version or unknown, and $available is the available version or unknown.
wizard-package-row = { $package }: { $action }. You have { $installed }. Latest is { $available }

wizard-review-heading = Review what you've asked RABBIT to do
wizard-review-target-prefix = Target
wizard-review-package-heading = Selected packages
wizard-review-osara-keymap-heading = OSARA key map
wizard-review-osara-keymap-preserve = Preserve your current key map.
wizard-review-osara-keymap-replace = Backup your current key map then replace with the latest from OSARA.
wizard-review-notes-heading = Notes
wizard-review-preflight-prefix = Cannot install yet

# $path is the selected REAPER resource path.
wizard-review-target = Target: { $path }
wizard-review-no-target = No target selected.
wizard-review-no-package = No package selected.

# $package is the localized package display name and $action is the localized planned action.
wizard-review-package = { $package }: { $action }

wizard-progress-heading = Installation progress
wizard-progress-status-idle = Ready to install.
wizard-progress-status-running = Installing selected packages. This might take a few minutes.
wizard-progress-details-label = Progress details
wizard-progress-details-idle = No installation is running.
wizard-progress-details-starting = Starting setup operation.
wizard-progress-details-cache-prefix = Cache

wizard-done-heading = Done
wizard-done-status-idle = No installation has been run from this window yet.
wizard-done-status-success = RABBIT finished working its magic! Review the details below.
wizard-done-status-error = Installation failed. Review the error below.
wizard-done-status-no-packages = No package was selected for installation or update.
wizard-done-show-details = Show details
# Mnemonic messages are single-character native access keys. Choose a character
# from the translated label when possible.
wizard-done-launch-reaper = Open REAPER and close RABBIT
wizard-done-launch-reaper-mnemonic = O
wizard-done-open-resource = Open resource folder (only for advanced manual maintenance)
wizard-done-open-resource-mnemonic = R
wizard-done-no-reaper-app = No launchable REAPER application is known for this target.
wizard-done-launch-reaper-error-prefix = REAPER could not be launched
wizard-done-open-resource-error-prefix = Resource folder could not be opened
wizard-done-self-update-apply = Apply RABBIT update
wizard-done-self-update-apply-mnemonic = A
wizard-done-self-update-apply-running = Applying RABBIT update…
wizard-done-self-update-error-prefix = RABBIT self-update failed
wizard-done-self-update-relaunch-prefix = Relaunched RABBIT
wizard-self-update-status-checking = Checking for RABBIT updates…

# $current is the running RABBIT version, $latest is the version offered by the
# release manifest, $channel is the release channel id (e.g. "stable").
self-update-status-update-available = RABBIT update available: { $current } → { $latest } (channel { $channel }). Click 'Apply RABBIT update' to install.
self-update-status-up-to-date = RABBIT is up to date (current { $current }, channel { $channel }).

# $version is the version that the apply pipeline targeted but did not write.
self-update-apply-no-files-replaced = Self-update did not replace any files (target version { $version }).
# $count is the number of files swapped on disk, $root is the install directory,
# $version is the new RABBIT version that is now in place.
self-update-apply-replaced-summary = Replaced { $count } file(s) under { $root }; relaunch RABBIT to use { $version }.

# $signed / $unsigned are counts of binaries that produced each verdict.
self-update-apply-signature-summary-signed-only = Signature verification: { $signed } signed.
self-update-apply-signature-summary-unsigned-only = Signature verification: { $unsigned } unsigned.
self-update-apply-signature-summary-mixed = Signature verification: { $signed } signed, { $unsigned } unsigned.

# $pid is the OS process id of the other RABBIT install holding the lock.
self-update-lock-blocking = Another RABBIT install is in progress (PID { $pid }). Apply is paused until it finishes.

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
# $action is one of the localized "action-*" labels (Install/Update/Keep).
wizard-summary-package-plan-action =   Plan action: { $action }
# $status is one of the localized "status-*" labels.
wizard-summary-package-status =   Status: { $status }
# $version is the version RABBIT just installed (or confirmed already current).
wizard-summary-package-installed-version =   Installed version: { $version }
# $architecture is the detected REAPER architecture (x64, arm64, …).
wizard-summary-architecture = Architecture: { $architecture }
status-installed-or-checked = Installed or checked
status-planned-unattended = Planned unattended
status-deferred-unattended = Deferred unattended
status-skipped-current = Skipped (already current)

# Per-package status messages surfaced on the wizard's Done page next to the
# package name (e.g. "OSARA: <message>"). The wrapper template
# `wizard-summary-package-message` already prefixes the package name, so each
# of these strings is just the message body.
package-status-extension-binary-installed = Single extension binary handled by RABBIT installer.
# $installed is the on-disk version; $available is the latest upstream version.
package-status-skipped-current = Installed version { $installed } is current or newer than available version { $available }.
# $automation is one of the "package-automation-*" labels (vendor installer / archive extraction / ...).
package-status-dry-run-would-run-unattended = Dry run: RABBIT would download and run this { $automation } unattended.
# $automation is one of the "package-automation-*" labels.
package-status-deferred-unattended-staged = This build has not implemented the planned unattended { $automation } execution path yet. RABBIT staged the artifact in the cache but did not run it.
# $automation is one of the "package-automation-*" labels.
package-status-deferred-unattended-not-staged = This build has not implemented the planned unattended { $automation } execution path yet. RABBIT did not download or run the artifact.
package-status-unattended-installed = RABBIT ran the upstream installer unattended, verified the expected target paths, and updated the RABBIT receipt.
package-status-osara-unattended-keymap-backed-up = RABBIT ran the upstream installer unattended, backed up reaper-kb.ini, applied the OSARA key map replacement, and updated the RABBIT receipt.
package-status-osara-unattended-keymap-replaced = RABBIT ran the upstream installer unattended, applied the OSARA key map replacement, and updated the RABBIT receipt.

# Short automation-kind labels interpolated into the per-package status
# messages above.
package-automation-installer = vendor installer
package-automation-archive = archive extraction
package-automation-disk-image = disk image install
package-automation-extension-binary = direct file install

# Per-configuration-step status messages surfaced on the wizard's Done page.
# `wizard-summary-configuration-message = { $step }: { $message }` is the
# wrapper template — the `*-message` keys below are the message body only.
# $name is the human-readable remote name; $url is the index XML URL.
config-message-reapack-remote-already-present = ReaPack remote { $name } ({ $url }) is already configured in reapack.ini.
config-message-reapack-remote-added = Added ReaPack remote { $name } ({ $url }) to reapack.ini.
config-message-reapack-remote-created-file = Created reapack.ini with ReaPack remote { $name } ({ $url }). ReaPack will add its default repositories on the next REAPER launch.
config-message-reapack-remote-dry-run = Would add ReaPack remote { $name } ({ $url }) to reapack.ini.
# $step is the configuration step id (e.g. `reapack-add-reaper-accessibility-remote`).
config-message-skipped = Configuration step { $step } was not selected.
# $step is the configuration step id; $dependency is the dependency package id.
config-message-skipped-dependency-missing = Configuration step { $step } skipped because its dependency package { $dependency } was not installed and is not part of this plan.
config-message-applied-no-op = Configuration step applied without changes.

# Per-configuration-step status sub-line on the Done page. Sibling to
# `wizard-summary-package-status` which handles per-package items.
wizard-summary-configuration-message = { $step }: { $message }
wizard-summary-configuration-status =   Status: { $status }

# Configuration step status labels used in the summary's "  Status: …" line.
config-status-applied = Applied
config-status-skipped = Skipped
config-status-skipped-dependency-missing = Skipped (dependency missing)
config-status-dry-run = Dry run
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
wizard-planned-runner-extract-archive-copy-osara = Extract archive and copy OSARA installer assets
wizard-planned-runner-mount-disk-image = Mount disk image and run contained installer
wizard-planned-runner-mount-disk-image-copy-app = Mount disk image and copy contained app bundle
