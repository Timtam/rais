# RAIS Design

Status: initial design, 2026-04-23

RAIS means "REAPER Accessibility Installation Software" and is pronounced like
"rice". Its job is to install and update REAPER, OSARA, SWS, ReaPack, and later
additional packages, while keeping the workflow usable with screen readers on
Windows and macOS.

## Product Goals

- Install into an existing standard REAPER installation.
- Install into an existing portable REAPER installation.
- Install REAPER and all selected accessibility packages from scratch.
- Update REAPER and selected packages when newer versions are available.
- Detect installed versions where technically possible and clearly report
  "installed, version unknown" where it is not reliable.
- Prefer user-level installation paths for extensions so admin rights are not
  needed unless installing REAPER itself into a protected location.
- Preserve user configuration by default, especially key maps and ReaPack data.
- Make every user-visible string localizable from the beginning.
- Make RAIS itself as portable as possible: the preferred distribution is one
  executable file that can be downloaded, launched, used, and deleted without a
  RAIS installer, companion resource folder, or permanent RAIS installation.

## Source-Backed Facts

- REAPER's own download page exposes the current version and platform-specific
  downloads. On 2026-04-23 it listed REAPER 7.69, released 2026-04-12.
  Source: <https://www.reaper.fm/download.php>
- REAPER's user guide says the resource path is shown from "Options > Show
  REAPER resource path in explorer/finder" and that Windows settings are not
  stored in the Windows Registry. Source:
  <https://dlz.reaper.fm/userguide/ReaperUserGuide728c.pdf>
- REAPER portable installs are based on a folder containing `reaper.ini`.
  On Windows the installer has a "Portable install" option; on macOS the guide
  describes creating a folder with `reaper.ini` and copying the app there.
  Source: <https://www.reaper.fm/userguide/ReaperUserGuide681c.pdf>
- OSARA installs into the REAPER user configuration/resource path, not the
  REAPER program directory. Its Windows installer writes files into
  `UserPlugins`, `KeyMaps`, and `osara/locale`; the standard Windows installer
  also writes an uninstall registry `DisplayVersion`. Source:
  <https://raw.githubusercontent.com/jcsteh/osara/master/installer/osara.nsi>
- OSARA's macOS installer uses
  `~/Library/Application Support/REAPER` for a standard installation and asks
  for the folder containing portable REAPER for portable installs. Source:
  <https://raw.githubusercontent.com/jcsteh/osara/master/installer/mac/Install%20OSARA.js>
- OSARA's update code embeds `OSARA_VERSION` into the extension binary and
  compares it with the snapshot update JSON. Source:
  <https://raw.githubusercontent.com/jcsteh/osara/master/src/updateCheck.cpp>
- SWS 2.14.0.7 is the latest stable version shown on the SWS site at the time
  of this design. The SWS changelog says SWS can be installed either by the
  traditional installer or via ReaPack 1.2.4.4 or newer. Sources:
  <https://sws-extension.org/> and <https://sws-extension.org/whatsnew.php>
- SWS embeds Windows version resources using `SWS_VERSION` and
  `SWS_VERSION_STR`. Source:
  <https://raw.githubusercontent.com/reaper-oss/sws/master/version.rc2.in>
- ReaPack's user guide says to install by placing the downloaded platform file
  into REAPER's `UserPlugins` directory and restarting REAPER. Source:
  <https://reapack.com/user-guide>
- ReaPack 1.2.6 is the latest stable release shown by ReaPack at the time of
  this design. Source: <https://reapack.com/>
- ReaPack builds its version from `Extensions/ReaPack.ext`, embeds Windows file
  version resources, and registers installed packages in
  `ReaPack/registry.db`. Sources:
  <https://raw.githubusercontent.com/cfillion/reapack/master/Extensions/ReaPack.ext>,
  <https://raw.githubusercontent.com/cfillion/reapack/master/src/buildinfo.rc>,
  and <https://raw.githubusercontent.com/cfillion/reapack/master/src/registry.cpp>
- wxWidgets can use native controls on Windows and has MSAA accessibility
  support through `wxAccessible`, but that documented custom accessibility class
  is Windows/MSAA-specific. Sources:
  <https://wxwidgets.org/docs/faq/windows/> and
  <https://wxd.sourceforge.net/wxWidgets-2.6/docs/html/wx/wx_wxaccessible.html>
- wxDragon is a Rust wxWidgets toolkit/wrapper. Its docs describe native
  Windows, macOS, and Linux support, a Rust widget API, and XRC support through
  `include_xrc!`. Source: <https://docs.rs/wxdragon/latest/wxdragon/>
- AccessKit is Rust accessibility infrastructure for custom-rendered UI
  toolkits and supports Windows/macOS adapters. Source: <https://accesskit.dev/>

## Recommended Technical Direction

Use Rust for the core application, package engine, and primary UI. The UI should
prefer wxDragon so RAIS can stay in one Rust codebase while still using mature
wxWidgets-backed native controls.

Recommended first implementation:

- `rais-core` in Rust: detection, manifests, downloads, verification, install
  planning, backups, receipts, localization lookup, and logging.
- `rais-cli` in Rust: a command-line entry point used for diagnostics, tests,
  unattended installs, and future automation.
- `rais-ui-wxdragon` in Rust: a wxDragon UI crate calling `rais-core` directly.
  Use wxDragon's native widgets and sizers for the main wizard. XRC may be used
  for screen layout if it improves maintainability, but application logic should
  remain in Rust modules and view models.
- Build release artifacts as self-contained executables wherever the platform
  allows. Embed required UI text, default localization resources, package
  metadata, and small static assets into the binary. Do not require a RAIS
  installer for normal use.

This keeps the important logic and UI integration in Rust and avoids maintaining
a separate C++/Objective-C/C ABI UI shell. The tradeoff is that wxDragon is a
younger Rust layer over a mature toolkit, so RAIS should keep the GUI thin and
well tested. If wxDragon blocks required accessibility behavior, the fallback
should be a direct wxWidgets shell with the same view-model boundary rather than
rewriting the installer engine.

## RAIS Portability

RAIS should behave like a portable utility. A user should be able to download a
single executable, run it from any writable folder, complete the REAPER setup or
update, and remove the executable afterward. The executable may create cache,
log, backup, report, and receipt files in explicit RAIS locations under the
selected REAPER resource path or user cache directory, but those files are
operation data, not files required to start RAIS.

Distribution goals:

- Windows: prefer `RAIS.exe` as a single signed executable. Avoid an MSI or
  setup program for the normal download. If a future installer is offered, it
  must be optional and not the primary accessibility path.
- macOS: prefer a signed and notarized standalone app bundle if macOS platform
  policy makes a literal single Mach-O executable impractical for GUI launch,
  but keep the bundle self-contained with no separate RAIS installer. A CLI-only
  build may still be a single executable.
- Do not require separate locale files, XRC files, icons, package manifests, or
  certificates beside the executable for the default experience.
- Store downloads in the normal RAIS cache directory and allow the cache to be
  deleted safely.
- Store install receipts, backups, and reports in the selected REAPER resource
  path so they travel with portable REAPER installations when possible.
- Do not write RAIS program settings to the Windows Registry. If user
  preferences become necessary, keep them optional and user-scoped.

Implementation rules:

- Use `include_str!`, `include_bytes!`, or generated Rust data for required
  built-in resources.
- Treat external locale/resource directories as optional developer or advanced
  override paths, not runtime requirements.
- Keep wxDragon/wxWidgets deployment self-contained for release builds. The
  release check must verify that launching RAIS does not depend on DLLs or
  dylibs sitting beside the executable unless the platform's GUI framework
  requires a signed app bundle layout.
- Any temporary extraction of embedded helpers must go to a temporary directory,
  be integrity-checked, and be cleaned up best-effort.

## Accessibility Rules

- Use native controls only: buttons, check boxes, radio buttons, list views,
  tree views, text fields, progress controls, and standard dialogs.
- Avoid custom drawing, canvas controls, owner-drawn lists, and grid controls in
  the main workflow.
- Every control needs an accessible name, role, state, keyboard focus, and
  visible label.
- Every screen must work by keyboard only, with a predictable tab order and
  mnemonic accelerators.
- Progress must be exposed as both a progress bar and a text status line.
- Errors must be plain text, selectable/copyable, and available in a final
  report.
- The test matrix for every release must include NVDA and Narrator on Windows,
  plus VoiceOver on macOS. JAWS should be included before public beta.

## User Workflow

The UI should be a short wizard with no hidden advanced requirements.

1. Welcome and target choice
   - Use detected REAPER installation
   - Install new standard REAPER
   - Install new portable REAPER
   - Choose a portable folder

2. Installation selection
   - Show detected candidates with type, path, REAPER version, architecture,
     resource path, and confidence.
   - Let the user choose one target.

3. Packages
   - Check boxes for REAPER, OSARA, SWS, ReaPack, and later packages.
   - Each row shows installed version, available version, action, and notes.
   - Defaults: install or update missing/outdated recommended accessibility
     packages.
   - OSARA key map option:
     - Install OSARA key map file.
     - Preserve current key map by default.
     - Offer "Replace current key map with OSARA key map" only with an explicit
       backup explanation.

4. Review
   - Plain text summary of files to be changed, backups to be created, and any
     admin prompts expected.

5. Install/update progress
   - Current step.
   - Package progress.
   - Details button for log.

6. Done
   - Success/failure summary.
   - Buttons: Launch REAPER, Open resource folder, Save report.

## Installation Discovery

Represent every candidate as:

```text
Installation {
  kind: Standard | Portable,
  app_path: PathBuf,
  resource_path: PathBuf,
  version: Option<Version>,
  architecture: Option<Architecture>,
  writable: bool,
  confidence: High | Medium | Low,
  evidence: Vec<Evidence>
}
```

Windows standard detection:

- Resource path: `%APPDATA%\REAPER`.
- App candidates:
  - `%ProgramFiles%\REAPER\reaper.exe`
  - `%ProgramFiles(x86)%\REAPER\reaper.exe`
  - uninstall registry entries, treated only as hints.
- Version: Windows file version metadata where available; otherwise optional
  runtime probe.

Windows portable detection:

- User-selected folder is authoritative.
- Auto-detected portable candidates must contain `reaper.exe` and `reaper.ini`.
- Resource path is the portable folder.

macOS standard detection:

- App candidates:
  - `/Applications/REAPER.app`
  - `/Applications/REAPER64.app`
  - `/Applications/REAPER-ARM.app`
- Resource path: `~/Library/Application Support/REAPER`.
- Version: app bundle `Info.plist` where available; otherwise optional runtime
  probe.

macOS portable detection:

- User-selected folder is authoritative.
- Candidate folder should contain a REAPER app bundle and normally `reaper.ini`.
- Resource path is the selected folder.

REAPER runtime probe:

- Optional later enhancement: launch REAPER in a controlled way with a temporary
  ReaScript that calls `reaper.GetAppVersion()`. This is more accurate than
  binary metadata but must never be used silently if it would disturb the user
  or open a visible DAW session unexpectedly.

## Version Detection Strategy

There is no single generic, external "REAPER extension version" API that RAIS
should rely on. Detection must be package-specific and confidence-scored.

| Component | Primary detector | Fallback detector | Confidence notes |
| --- | --- | --- | --- |
| REAPER | executable/app metadata or optional `GetAppVersion()` runtime probe | presence of app only | Runtime probe is most accurate, metadata is good enough for normal installs. |
| OSARA | RAIS receipt after RAIS-managed install; Windows standard uninstall `DisplayVersion` | binary string scan for embedded `OSARA_VERSION`; presence of `reaper_osara*` | Portable/mac installs do not have a universal external version registry. Binary scan is useful but should be marked best-effort. |
| SWS | RAIS receipt; Windows PE `ProductVersion`; ReaPack registry if installed by ReaPack | binary metadata/string scan; presence of `reaper_sws*` | Prefer ReaPack registry for ReaPack-managed SWS. |
| ReaPack | RAIS receipt; Windows PE `ProductVersion`; ReaPack self-entry in `ReaPack/registry.db` after first launch | presence of `reaper_reapack*` | The registry DB may not exist until ReaPack has run inside REAPER. |
| ReaPack packages | `ReaPack/registry.db` table `entries.version` | none | This is the best source for packages ReaPack knows about. |

RAIS should keep its own receipt in each REAPER resource path:

```text
<resource_path>/RAIS/install-state.json
```

The receipt should record package id, installed version, source URL, SHA-256,
installed files, backup files, install time, RAIS version, and target
architecture. This is authoritative only for files RAIS installed. If the user
later changes files manually, RAIS should show that the receipt and disk state
do not match.

## Package Model

Use a manifest-driven package system so future packages can be added without
rewriting the installer engine.

```text
PackageSpec {
  id,
  display_name_key,
  package_kind,
  required,
  recommended,
  min_reaper_version,
  supported_platforms,
  supported_architectures,
  latest_version_provider,
  artifact_provider,
  detectors,
  install_steps,
  uninstall_steps,
  backup_policy
}
```

Initial package kinds:

- `reaper_app`: vendor installer, dmg/app copy, or portable creation.
- `user_plugin_binary`: copy one or more extension binaries into `UserPlugins`.
- `keymap`: copy into `KeyMaps`, optionally replace `reaper-kb.ini` with
  backup.
- `reapack_package`: install/update through ReaPack later, once ReaPack is
  present and REAPER has been launched.

## Install Targets

Resource path layout:

```text
<resource_path>/
  reaper.ini
  reaper-kb.ini
  UserPlugins/
  KeyMaps/
  ReaPack/
    registry.db
  osara/
    locale/
  RAIS/
    install-state.json
    logs/
    backups/
```

Extension files:

- OSARA Windows: install all available supported OSARA DLLs, matching the
  upstream installer behavior:
  - `UserPlugins/reaper_osara32.dll`
  - `UserPlugins/reaper_osara64.dll`
  - `UserPlugins/reaper_osara_arm64ec.dll` where supported
- OSARA macOS:
  - `UserPlugins/reaper_osara.dylib`
- SWS:
  - install the binary matching the selected REAPER architecture, unless the
    upstream package explicitly supports installing multiple side-by-side
    architecture files.
- ReaPack:
  - install the binary matching REAPER architecture, not merely the operating
    system architecture.

## Update Flow

1. Discover installation candidates.
2. Detect current component state.
3. Refresh latest-version metadata from providers.
4. Build an install plan.
5. Show the plan and require confirmation.
6. Download artifacts into the RAIS cache:
   - Windows: `%LOCALAPPDATA%\RAIS\cache`
   - macOS: `~/Library/Caches/RAIS`
7. Verify artifacts:
   - HTTPS only.
   - SHA-256 when known.
   - Authenticode signature for Windows executables and DLLs where available.
   - macOS code signing/notarization checks where available.
8. Ensure REAPER is not running.
9. Create backups.
10. Apply changes using temp files and atomic rename where possible.
11. Write receipt and report.

## Safety Behavior

- If REAPER is running, stop before changing extension files and ask the user to
  close it.
- Never overwrite `reaper-kb.ini` unless the user explicitly asks to replace the
  key map.
- Back up every overwritten file under `RAIS/backups/<timestamp>/`.
- Keep a machine-readable operation report and a plain text report.
- Treat non-writable targets as a planning error before downloading anything.
- Do not request elevation unless the selected REAPER app install target
  requires it.
- Do not delete unknown files during update. Only remove files listed in a RAIS
  receipt or explicitly owned by the package manifest.

## Localization

Use message IDs from the first commit. Recommended Rust-side choice: Fluent via
`fluent-rs`. Required built-in locales should be embedded into the executable.
During development and for advanced overrides, RAIS may also read locale files
like:

```text
locales/
  en-US/
    rais.ftl
```

Rules:

- No string concatenation for user-visible messages.
- The default release must work without a `locales/` directory.
- Include translator comments for placeholders.
- Localize logs that are shown to users, but keep an internal structured event
  code for support.
- Include accessible names, descriptions, dialog titles, and button labels in
  localization.
- Keep package display names localizable, but package IDs stable and
  untranslated.

## Suggested Repository Structure

```text
Cargo.toml
crates/
  rais-core/
  rais-platform/
  rais-cli/
  rais-ui-wxdragon/
ui/
  wxdragon/
    xrc/
locales/
  en-US/
docs/
  architecture/
tests/
```

`rais-core` should have no GUI dependency. `rais-platform` should isolate
Windows/macOS APIs. The wxDragon UI should depend on `rais-core`, not the other
way around, so another native UI shell could replace it without changing the
package engine. Files under `locales/` and `ui/` are source/development assets;
release builds should embed required resources instead of shipping those
directories beside the executable.

## Testing Strategy

Automated tests:

- manifest parsing
- version comparison
- path discovery from fake filesystem fixtures
- ReaPack SQLite registry parsing
- receipt read/write
- install planning
- backup and rollback
- embedded-resource availability without external files
- release packaging checks for accidental runtime file dependencies

Manual accessibility tests:

- Windows 11 with NVDA
- Windows 11 with Narrator
- Windows 11 with JAWS before beta
- macOS current release with VoiceOver

Install tests:

- clean Windows standard install
- clean Windows portable install
- clean macOS standard install
- clean macOS portable install
- update from older REAPER plus older extensions
- launch RAIS from a temporary folder with no neighboring resource files
- existing user key map preserved
- OSARA key map replacement with backup
- ReaPack already installed with populated registry
- extension installed manually with unknown version

## Open Questions

- Verify whether the REAPER Windows installer has documented silent install
  arguments suitable for accessible unattended standard and portable installs.
- Confirm SWS and ReaPack macOS binaries expose reliable version metadata
  outside ReaPack's registry DB. If not, RAIS receipts and ReaPack DB should be
  treated as the reliable sources.
- Decide whether first-version RAIS should install SWS directly from the SWS
  installer/assets or use ReaPack. Direct file install is simpler before REAPER
  has launched; ReaPack gives better future package management after it is
  initialized.
- Build a small wxDragon proof of concept and test it with NVDA, Narrator, and
  VoiceOver before expanding it into the full wizard.
- Verify whether wxDragon exposes the wxWidgets accessibility hooks RAIS needs
  directly. If not, document the smallest upstream contribution or local wrapper
  needed for accessible names, descriptions, roles, and state.
- Verify wxDragon/wxWidgets release packaging on Windows and macOS can meet the
  one-download, no-RAIS-installer goal without sacrificing code signing,
  notarization, or screen-reader behavior.
