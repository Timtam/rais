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

wizard-button-back = Back
wizard-button-next = Next
wizard-button-install = Install
wizard-button-close = Close

wizard-target-heading = Choose REAPER installation
wizard-target-choice-label = Detected installations
wizard-target-details-label = Target details
wizard-target-empty = No REAPER installation was detected. Use the CLI setup command for a new portable resource path until the GUI creation workflow is implemented.

# $kind is the detected installation kind, $version is the REAPER version or an unknown-version label, and $path is the resource path.
wizard-target-row = { $kind } REAPER { $version } at { $path }

# $path is the REAPER resource path, $writable is yes/no, and $confidence is the detection confidence.
wizard-target-details = Resource path: { $path }
    Writable: { $writable }
    Detection confidence: { $confidence }

wizard-packages-heading = Choose packages
wizard-packages-list-label = Packages to install or update
wizard-package-details-label = Package details

# $package is the localized package display name, $action is the localized planned action, $installed is the installed version or unknown, and $available is the available version or unknown.
wizard-package-row = { $package }: { $action }. Installed: { $installed }. Available: { $available }

wizard-review-heading = Review changes
wizard-review-target-prefix = Target

# $path is the selected REAPER resource path.
wizard-review-target = Target: { $path }
wizard-review-no-target = No target selected.
wizard-review-no-package = No package selected.

# $package is the localized package display name and $action is the localized planned action.
wizard-review-package = { $package }: { $action }

wizard-progress-heading = Installation progress
wizard-progress-status-idle = Ready to install.
wizard-progress-status-running = Installing selected packages. This can take several minutes.

wizard-done-heading = Done
wizard-done-status-idle = No installation has been run from this window yet.
wizard-done-status-success = Installation finished. Review the details below.
wizard-done-status-error = Installation failed. Review the error below.
wizard-done-status-no-packages = No package was selected for installation or update.
