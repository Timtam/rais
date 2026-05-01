# RAIS Design

Status: revised 2026-05-01

RAIS means "REAPER Accessibility Installation Software" and is pronounced like
"rice". Its job is to install and update REAPER, OSARA, SWS, ReaPack,
ReaKontrol, JAWS-for-REAPER scripts (Windows only), and later additional
packages, while keeping the workflow usable with screen readers on Windows and
macOS.

The audience is REAPER users — including users who are not Rust developers,
accessibility experts, or installer engineers. RAIS must therefore choose
sensible defaults, hide implementation detail, and avoid asking the user
questions they cannot reasonably answer.

## Product Goals

- One executable. RAIS ships as a single self-contained binary per platform.
  Run with no arguments → graphical wizard; run with any argument or `--help` →
  CLI. There is no separate `rais-cli` binary, no helper executable, no
  installer, no companion resource folder.
- On Windows the executable does not pop up a console window when launched as
  a GUI; the same binary still attaches to the parent console when run from a
  command prompt with arguments.
- Release builds are optimized for file size first, then runtime speed
  (`opt-level = "z"`, fat LTO, single codegen unit, stripped symbols, panic
  abort) so a download stays small for users on metered or slow connections.
- The wizard is short, opinionated, and free of jargon. RAIS picks the
  defaults a non-technical user would otherwise have to research, and presents
  results in plain terms ("REAPER 7.69 installed", "OSARA up to date") rather
  than internal mechanics ("detector: rais-receipt", "confidence: High",
  "PlannedAutomationKind::VendorInstaller").
- Install into an existing standard REAPER installation, into an existing
  portable REAPER installation, or set up REAPER plus the selected packages
  from scratch.
- Fully automate installation and update of REAPER, OSARA, SWS, ReaPack,
  ReaKontrol, and the Windows-only JAWS-for-REAPER scripts without asking the
  user to run vendor installers or copy files manually in the normal flow.
- Update REAPER and selected packages when newer versions are available.
- Detect installed versions where technically possible and clearly report
  "installed, version unknown" where it is not reliable.
- Prefer user-level installation paths for extensions so admin rights are not
  needed unless installing REAPER itself into a protected location.
- Preserve user configuration by default where possible, but when OSARA is
  selected RAIS replaces the active key map with the OSARA key map after
  backing up `reaper-kb.ini`. This is the default and is not exposed as a
  user-facing question in the GUI; the CLI keeps an explicit opt-out for power
  users.
- Make every user-visible string localizable from the beginning. Embedded
  locales today: en-US and de-DE.
- Build Windows and macOS artifacts automatically for every push in GitHub
  Actions so every commit can be tested from real binaries. Artifacts are the
  raw single-file binaries; no zipping.
- Publish signed release artifacts through a GitHub release pipeline so tagged
  versions become downloadable binaries with checksums and update metadata.
- Let RAIS detect when a newer RAIS version has been released and update
  itself with as little user interaction as practical while preserving
  accessibility and platform trust requirements.

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
- ReaKontrol is a REAPER extension for Native Instruments Komplete Kontrol
  keyboards. Its site says the current version is 2026.2.16.100, it requires
  REAPER 6.37 or later, and there is no installer: on Windows and macOS the
  user installs it by copying the downloaded file into REAPER's `UserPlugins`
  folder. The site also documents `reaKontrol/fxMaps` under the REAPER resource
  path for additional mapping files. Sources:
  <https://reakontrol.jantrid.net/> and
  <https://github.com/jcsteh/reaKontrol>
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
- The REAPER Accessibility Hoard is a public file server that hosts the
  JAWS-for-REAPER scripts under
  <https://hoard.reaperaccessibility.com/Custom%20actions,%20Scripts%20and%20jsfx/Windows%20Scripts/JAWS%20Scripts%20by%20Snowman/>.
  It is the rejetto/HFS server (`https://github.com/rejetto/hfs`), which exposes
  a documented public REST API: `POST /~/api/get_file_list` returns a JSON
  directory listing including each entry's name, size, and modified time, which
  RAIS can use as the latest-version provider for the JAWS scripts.

## Recommended Technical Direction

Use Rust for the core application, package engine, and primary UI. The UI
should prefer wxDragon so RAIS can stay in one Rust codebase while still using
mature wxWidgets-backed native controls.

Recommended implementation:

- `rais-core` in Rust: detection, manifests, downloads, verification, install
  planning, backups, receipts, localization lookup, and logging.
- `rais-platform` in Rust: Windows/macOS native API isolation (file-version
  probes, registry probes, keychain/codesign, locale probe, plist parsing,
  disk-image mounting). One-way dependency from `rais-core` to
  `rais-platform`.
- `rais` in Rust (single binary, formerly `rais-cli` + `rais-ui-wxdragon`): the
  user-facing entry point. Built with the GUI feature on by default so a release
  binary can launch the wizard. The `main` function dispatches by argv:
  - no arguments → run the wxDragon wizard
  - any arguments or `--help` → run the CLI subcommand parser
  This single-binary model removes the duplicate distribution shape and makes
  it easy for users to memorize "the file is `RAIS.exe`/`RAIS`".
- Build release artifacts as self-contained executables wherever the platform
  allows. Embed required UI text, default localization resources, package
  metadata, and small static assets into the binary. Do not require a RAIS
  installer for normal use.

### Single-Binary Argv Dispatch

Top-level `main`:

1. Read `std::env::args_os()`.
2. If exactly one argument is present (the program name itself), launch the
   GUI. On Windows, the binary is built with `#![windows_subsystem = "windows"]`
   so no console pops up; the GUI path uses `AttachConsole(ATTACH_PARENT_PROCESS)`
   only when the user explicitly asks for a banner/version on stdout.
3. Otherwise, hand the full argv to the CLI parser, which on Windows attaches
   to the parent console (or allocates one) so help output and command
   results are visible.

The CLI subcommand surface stays roughly what `rais-cli` exposes today;
moving it under the same crate is a packaging change, not a feature change.

### Release Build Profile

Release builds are tuned for binary size:

```toml
[profile.release]
opt-level = "z"     # optimize for size
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

`debug-assertions` stays off in release. The CI release-mode artifact check
ensures the produced binary stays a single file with no neighboring DLL/dylib
dependency for the normal launch path.

This keeps the important logic and UI integration in Rust and avoids maintaining
a separate C++/Objective-C/C ABI UI shell. The tradeoff is that wxDragon is a
younger Rust layer over a mature toolkit, so RAIS should keep the GUI thin and
well tested. If wxDragon blocks required accessibility behavior, the fallback
should be a direct wxWidgets shell with the same view-model boundary rather than
rewriting the installer engine.

## Primary Automation Requirement

Full unattended installation is part of the product definition, not a stretch
goal. For the first-class supported package set of REAPER, OSARA, SWS,
ReaPack, and ReaKontrol, RAIS should converge on one shared unattended
execution path used by both the GUI and CLI.

Design rules:

- The normal supported path must not stop at "download and tell the user what to
  do next" for REAPER, OSARA, SWS, ReaPack, or ReaKontrol.
- For executable installers, RAIS itself must download, verify, launch, wait
  for completion, evaluate exit status, and validate the installed result in the
  same run.
- The supported flow must not require the user to manually open an `.exe`,
  `.pkg`, `.app`, disk image, or extracted archive and click through its setup
  UI on their own.
- Any manual-attention flow for those packages is a temporary implementation
  gap, not acceptable steady-state product behavior.
- The GUI wizard and CLI must call the same package execution engine so
  unattended behavior is consistent and testable.
- RAIS should prefer direct verified file installation for extensions when that
  is technically reliable, because it is more deterministic and accessible than
  driving third-party interactive installers.
- When RAIS must use a vendor installer, it should do so with documented or
  validated silent arguments, explicit exit-code handling, integrity checks, and
  a post-install verification pass.
- "Run upstream installer" in the package model means RAIS invokes the installer
  itself as part of the installation operation. It does not mean "download the
  installer and ask the user to run it manually later".
- If a package cannot currently be installed unattended on a platform, RAIS
  should mark that as unsupported for that build/platform combination, not treat
  permanent manual installation as the finished design.

## RAIS Portability

RAIS should behave like a portable utility. A user should be able to download a
single executable, run it from any writable folder, complete the REAPER setup or
update, and remove the executable afterward. The executable may create cache,
log, backup, report, and receipt files in explicit RAIS locations under the
selected REAPER resource path or user cache directory, but those files are
operation data, not files required to start RAIS.

Distribution goals:

- Windows: a single signed executable. No MSI, no setup program, no companion
  DLLs in the default launch path. The same executable serves both UI and CLI
  users via argv dispatch.
- macOS: a single signed and notarized executable. Use an `.app` bundle
  layout only if macOS GUI launch policy forces it for a given
  wxWidgets/wxDragon shape; in that case the bundle stays self-contained with
  no separate RAIS installer, and the CLI command-line surface still works
  against the binary inside the bundle.
- Release artifact names follow `rais-<version>-<os>-<arch>[.exe]`
  (e.g. `rais-0.2.0-windows-x86_64.exe`, `rais-0.2.0-macos-aarch64`). This
  makes successive downloads distinguishable on disk, calls out the build's
  architecture explicitly, and works around the macOS-binary-without-extension
  ambiguity. Users may rename the file after downloading; the self-update
  apply step swaps in place under whatever filename the running binary has,
  not under the downloaded file's name.
- CI artifacts and GitHub release assets are the raw single-file binaries plus
  per-file SHA-256 sums. No `.zip` wrapper is required for single-file
  artifacts; release notes link directly to the per-platform binaries.
- Do not require separate locale files, XRC files, icons, package manifests,
  or certificates beside the executable for the default experience.
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
- The same portability rule applies to self-update: updated binaries should
  replace the old RAIS binary or app bundle in place, not install a separate
  long-lived updater application.

## CI/CD And Release Delivery

RAIS should have first-class delivery automation from the beginning. The design
target is that every push produces testable platform artifacts and every tagged
release produces end-user release assets plus update metadata.

GitHub Actions build pipeline for every push:

- Trigger on every push and pull request.
- Build RAIS on at least:
  - `windows-latest` for the Windows executable
  - `macos-latest` for the macOS app or executable artifact
- Run the normal Rust checks on both platforms:
  - formatting
  - unit/integration tests
  - release-mode build
- Build the distributable artifact shape, not only debug binaries:
  - Windows: a single `RAIS.exe`
  - macOS: a single `RAIS` executable (or a self-contained signed `.app`
    bundle if macOS GUI launch policy requires it for the chosen
    wxWidgets/wxDragon shape)
- Upload build artifacts to the workflow run so every push has downloadable
  test binaries. Single-file artifacts upload as the bare file; no `.zip`
  wrapper is added on top of an already-single-file binary.
- Publish per-file SHA-256 sums alongside the artifacts so testers can
  verify what they ran.

GitHub release pipeline:

- Trigger on a version tag such as `vX.Y.Z`, or on an explicit release workflow
  dispatch that creates the tag as part of the release process.
- Rebuild Windows and macOS release artifacts from the tagged commit in a clean
  GitHub Actions environment.
- Produce release attachments:
  - Windows artifact
  - macOS artifact
  - SHA-256 checksums
  - machine-readable release/update manifest
- Apply code signing where available:
  - Windows Authenticode signing for `RAIS.exe`
  - macOS code signing and notarization for the app bundle or executable
- Generate release notes from a changelog or tag diff, with manual override for
  accessibility-relevant release notes.
- Publish the GitHub Release only after artifacts, checksums, signing, and
  update metadata are complete.

Release metadata:

- Each published release should expose enough machine-readable metadata for
  RAIS self-update, including:
  - semantic version
  - release channel (`stable`, later optionally `beta`)
  - publish timestamp
  - per-platform download URL
  - expected SHA-256
  - minimum supported previous RAIS version if a breaking updater transition is
    ever needed
- The release workflow should emit a stable JSON manifest asset or equivalent
  update feed derived from the GitHub Release so the updater does not need to
  scrape human-written release notes.

Suggested workflow layout:

```text
.github/
  workflows/
    ci.yml
    release.yml
```

## RAIS Self-Update

RAIS should be able to update itself from GitHub Releases with minimal user
interaction while staying accessible and verifiable.

Updater design goals:

- Check for RAIS updates separately from package updates for REAPER and its
  extensions.
- Use the GitHub release/update manifest as the authoritative source for the
  latest RAIS version and platform artifact URL.
- Compare versions using strict semantic versioning for RAIS itself.
- Default behavior should be:
  - detect newer RAIS release
  - present a short accessible prompt
  - download in the background after confirmation
  - apply the update with one restart/replace step
- Advanced later option:
  - support a user preference for automatically downloading stable RAIS updates
    and applying them on the next restart

Updater flow:

1. On startup, or on explicit `Check for RAIS updates`, fetch the signed or
   checksum-validated release manifest from the configured GitHub release
   channel.
2. If the current RAIS version is already current, report that plainly.
3. If a newer version exists, show:
   - current version
   - available version
   - release channel
   - short release notes or a link to them
4. After confirmation, download the platform artifact to a temporary update
   staging directory.
5. Verify:
   - HTTPS transport
   - expected SHA-256 from release metadata
   - Windows signature or macOS code signing/notarization where applicable
6. Stage the replacement.
7. Replace RAIS with the new version using the smallest platform-appropriate
   restart flow.
8. Relaunch the updated RAIS instance and confirm the new version.

Platform update strategy:

- Windows single-executable build:
  - RAIS cannot replace its own running `.exe` in place.
  - Stage the new executable beside the current one or in a temporary
    directory.
  - Launch a very small temporary updater helper process or script whose only
    job is to wait for RAIS to exit, swap the executable, and relaunch RAIS.
  - The helper must be ephemeral, integrity-checked, and cleaned up best-effort.
- macOS app bundle build:
  - Stage the new signed/notarized app bundle in a temporary directory.
  - After RAIS exits, replace the existing bundle atomically where possible and
    relaunch it.
  - Preserve the app bundle path so Dock aliases and user expectations do not
    break.

Updater safety rules:

- Never apply a RAIS self-update while a package installation/update operation
  is running.
- Never replace RAIS with an unsigned or checksum-mismatched artifact.
- Keep one rollback copy of the previously running RAIS binary or bundle until
  the first successful restart of the updated version.
- Log the update attempt and result in a plain text and machine-readable report.
- If self-update fails, RAIS must continue running the existing version and
  report the error without leaving itself half-replaced.

CLI and UI expectations:

- CLI should expose explicit commands later such as:
  - `rais self-update check`
  - `rais self-update apply`
- GUI should expose:
  - automatic startup check
  - manual `Check for RAIS updates`
  - accessible progress/status during download and replacement

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

The UI is a short, jargon-free wizard. Defaults are chosen so a non-technical
user can finish the install by pressing Next a few times.

1. Target
   - One concise control: pick a detected REAPER, or pick "Install or update a
     portable REAPER folder" and choose the folder.
   - Detected installations show simply as "REAPER 7.69 in `<path>`" — no
     architecture, no detection-confidence label, no evidence list. Those
     remain available in the saved report and from the CLI for advanced
     users.

2. Version check
   - A dedicated progress page with a progress bar, one status line ("Checking
     OSARA…") and a hidden error region that only appears if a fetch failed.
   - On success the wizard auto-advances to the package list. On failure the
     page surfaces the error lines and lets the user go back or quit.

3. Packages
   - Check boxes for REAPER, OSARA, SWS, ReaPack, ReaKontrol, JAWS scripts
     (Windows only), and later packages. Each row reads as plain text:
     "OSARA — installed 2024.3.6, latest 2026.2.16, will update".
   - The selected row's details pane shows the package's localized
     description (one or two plain-language sentences explaining what the
     package is and why a user might want it). The same description is
     exposed by the CLI via `rais packages`, so users can read about every
     package before deciding to install it.
   - Defaults: install or update missing/outdated recommended accessibility
     packages.
   - OSARA key map: RAIS replaces `reaper-kb.ini` with the OSARA key map after
     backing it up. The GUI does not ask the user to confirm; the backup is
     mentioned in the final report. The CLI keeps `--preserve-osara-keymap`
     for power users who want the opt-out.

4. ReaPack donation acknowledgement (only when ReaPack is being installed
   or updated this run)
   - Dedicated wizard page that mirrors the donation hint from
     <https://reapack.com/>. Plain language, in the active locale:
     ReaPack is free software, donations are optional, here is the
     donation URL.
   - A focusable link to <https://reapack.com/donate> and an explicit
     checkbox or "I understand, continue" control. Continue is disabled
     until the acknowledgement is set. Going back to step 3 clears the
     acknowledgement so the user always re-confirms after changing the
     selection.
   - In the CLI, the same acknowledgement is surfaced as an interactive
     prompt before staging. For unattended use the user passes
     `--accept-reapack-donation-notice`; without it, `apply-packages` /
     `setup` refuses to stage ReaPack and exits non-zero with a clear
     message that points at the flag.
   - Skipped automatically on runs where ReaPack is neither installed nor
     updated.

5. Review
   - Short plain-language summary: target path, packages to be installed or
     updated, an indication that backups will be made if any existing files
     will be replaced. No backup-file paths, no admin-prompt enumeration, no
     planned-execution metadata in the GUI summary; all of that is still
     written to the saved report and exposed by the CLI.

6. Install/update progress
   - Single progress bar plus one current-step line. A "Show details" toggle
     reveals the underlying log for users who want it; collapsed by default.

7. Done
   - One sentence summarizing success or failure.
   - Buttons: Launch REAPER, Open resource folder, Save report. The
     signature-verification counts, lock-file paths, and similar diagnostics
     stay in the saved report.

Streamlining rules (apply when adding new wizard text):

- Default to one sentence per element. If a control needs more, the second
  sentence belongs in the saved report, not the wizard.
- Never expose internal identifiers (detector names, automation kinds, plan
  action enums, lock-file paths, SHA-256 prefixes) to the GUI user.
- Power-user output (full plan, full execution log, verdicts) stays
  reachable through the CLI and the saved report.

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
| ReaKontrol | RAIS receipt after RAIS-managed install | best-effort binary metadata if available; presence of `reaper_kontrol*` | No installer or registry-based detector is expected; validate binary metadata quality during implementation. |
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
  display_description_key,
  package_kind,
  required,
  recommended,
  requires_user_acknowledgement,
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

Every package ships a `display_description_key` that resolves to a one- or
two-sentence localized description of what the package is and why a user
might want it. The wizard surfaces the description in the package details
pane (the same spot that today shows the handling/version block); the CLI
exposes it via `rais packages` so non-technical users can read about
REAPER, OSARA, SWS, ReaPack, ReaKontrol, and the JAWS scripts before
deciding what to install.

`requires_user_acknowledgement` is set on packages whose upstream policy or
licensing posture asks for an explicit acknowledgement before install — see
the ReaPack donation rule below. Default is `false`. When set, RAIS must
not start the install for that package until the user has confirmed the
package-specific acknowledgement message in the GUI (a dedicated wizard
page) or in the CLI (an interactive prompt or an explicit
`--accept-<package>-notice` flag for unattended runs).

Initial package kinds:

- `reaper_app`: vendor installer, dmg/app copy, or portable creation.
- `user_plugin_binary`: copy one or more extension binaries into `UserPlugins`.
- `keymap`: copy into `KeyMaps`, optionally replace `reaper-kb.ini` with
  backup.
- `reapack_package`: install/update through ReaPack later, once ReaPack is
  present and REAPER has been launched.
- `screen_reader_scripts`: copy a screen-reader-specific script bundle into
  the user's screen-reader profile (e.g. JAWS scripts under `%APPDATA%\Freedom
  Scientific\JAWS\<version>\Settings\enu`). Platform-gated: a package of this
  kind only appears in the wizard when the relevant screen reader is
  available on the host.

For the initial supported package set, RAIS should implement these unattended
strategies:

- REAPER Windows standard install:
  - download the official installer,
  - verify signature and version,
  - invoke it itself with unattended arguments for standard installation,
  - wait for completion and treat non-zero or unexpected exit codes as failure,
  - verify `reaper.exe` and the target resource path after completion.
- REAPER Windows portable install:
  - either invoke the official installer itself with unattended portable-install
    arguments or use another validated vendor-supported unattended method,
  - wait for completion and treat non-zero or unexpected exit codes as failure,
  - verify both `reaper.exe` and `reaper.ini` in the selected portable folder.
- REAPER macOS standard install:
  - download the official disk image or app distribution,
  - verify signature/notarization,
  - mount or extract it non-interactively,
  - copy/install REAPER unattended into `/Applications` or the chosen target,
  - verify the final app bundle and version.
- REAPER macOS portable install:
  - create the portable folder layout unattended,
  - place the REAPER app bundle there using a verified unattended copy flow,
  - create or preserve `reaper.ini` as required for portable mode.
- OSARA:
  - install unattended by either invoking a validated silent installer path or
    reproducing the upstream file layout directly into the selected REAPER
    resource path,
  - manage the keymap behavior as a RAIS choice with default replacement plus
    backup, and an explicit preserve-current opt-out.
- SWS:
  - install unattended by placing the correct verified binary into
    `UserPlugins` for the selected REAPER architecture.
- ReaPack:
  - install unattended by placing the correct verified binary into
    `UserPlugins` for the selected REAPER architecture.
  - mark the package with `requires_user_acknowledgement = true`. Whenever
    a RAIS run would install or update ReaPack, the wizard must show a
    dedicated confirmation page before the Review step, and the CLI must
    prompt before staging the artifact. The page reproduces the donation
    hint visible on <https://reapack.com/> in the active locale: ReaPack is
    free software released under the terms of the LGPL, but its author
    Christian Fillion accepts donations at <https://reapack.com/donate> to
    support continued development. The page links to the donation URL,
    states clearly that donating is optional and that no donation is
    required to use ReaPack or RAIS, and only enables Continue once the
    user has explicitly acknowledged the notice (a checkbox or focused
    button in the GUI; an interactive prompt or
    `--accept-reapack-donation-notice` flag in the CLI for unattended
    runs). Skip the page entirely on runs where ReaPack is neither being
    installed nor updated.
- ReaKontrol:
  - install unattended by placing the correct verified binary into
    `UserPlugins` for the selected REAPER architecture or platform,
  - preserve existing `reaKontrol/fxMaps` user mapping data and treat it as
    user content, not package-owned files.
- JAWS scripts (Windows only):
  - latest-version + artifact provider: HFS REST API at
    `hoard.reaperaccessibility.com`. RAIS calls
    `POST /~/api/get_file_list` with the JAWS-Scripts directory as the path
    and parses the returned JSON to pick the newest archive (date + filename
    contain the version anchor). Source archive URLs come from the same API.
  - install unattended by extracting the archive and placing the script set
    into the user's JAWS settings folder (`%APPDATA%\Freedom Scientific\JAWS\
    <version>\Settings\enu` for the highest installed JAWS version, falling
    back to a clearly reported error if no JAWS install is detected).
  - the package only appears in the wizard package list on Windows when JAWS
    is detected on the host. macOS users never see the JAWS scripts row.
  - back up any existing same-named script files before overwriting; track
    the install through the standard RAIS receipt mechanism.

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
  reaKontrol/
    fxMaps/
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
- ReaKontrol Windows:
  - `UserPlugins/reaper_kontrol.dll`
- ReaKontrol macOS:
  - `UserPlugins/reaper_kontrol.dylib`
- ReaKontrol support data:
  - preserve `reaKontrol/fxMaps/` and any user-created map files during
    install and update.

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
10. Apply changes unattended:
   - invoke verified silent REAPER install steps where required,
   - launch installer executables or equivalent package routines directly from
     RAIS where the package model says so,
   - copy verified extension files directly where possible,
   - use temp files and atomic rename where possible.
11. Write receipt and report.
12. Verify the final installed state against the plan and report any mismatch as
    an installation failure, not a silent warning.

## Safety Behavior

- If REAPER is running, stop before changing extension files and ask the user to
  close it.
- When OSARA is selected, back up `reaper-kb.ini` and replace it with the OSARA
  key map by default unless the user explicitly asks to preserve the current key
  map instead.
- Do not overwrite or delete user-created `reaKontrol/fxMaps` content during a
  ReaKontrol install or update.
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
  rais/                # the single user-facing binary (CLI + GUI dispatch)
ui/
  wxdragon/
    xrc/
locales/
  en-US/
  de-DE/
docs/
  architecture/
tests/
```

`rais-core` has no GUI dependency. `rais-platform` isolates Windows/macOS
APIs. The `rais` binary crate depends on both and contains:

- `cli/` — the clap-based subcommand parser (former `rais-cli` content).
- `ui/` — the wxDragon wizard (former `rais-ui-wxdragon` content), behind a
  `gui` Cargo feature for dev-loop builds that skip native deps.
- `main.rs` — the argv dispatcher that picks GUI or CLI mode.

This layout lets another native UI shell replace the wxDragon module without
touching the package engine. Files under `locales/` and `ui/` are
source/development assets; release builds embed required resources instead
of shipping those directories beside the executable.

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
- GitHub Actions workflow validation for push builds and tagged releases
- release-manifest generation and checksum publication
- RAIS self-update version comparison and channel selection
- self-update staging, verification, rollback, and restart handoff logic

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
- unattended REAPER Windows standard install end-to-end
- unattended REAPER Windows portable install end-to-end
- unattended REAPER macOS standard install end-to-end
- unattended REAPER macOS portable install end-to-end
- unattended OSARA install end-to-end
- unattended SWS install end-to-end
- unattended ReaPack install end-to-end
- unattended ReaKontrol install end-to-end
- GitHub Actions push build produces downloadable Windows and macOS artifacts
- GitHub release workflow publishes release assets, checksums, and update
  metadata
- launch RAIS from a temporary folder with no neighboring resource files
- existing user key map preserved
- OSARA key map replacement with backup
- ReaPack already installed with populated registry
- existing `reaKontrol/fxMaps` user maps preserved
- extension installed manually with unknown version
- RAIS self-update from one released version to the next on Windows
- RAIS self-update from one released version to the next on macOS

## Open Questions

- Select and validate the exact unattended invocation strategy for the REAPER
  Windows installer for both standard and portable targets, including exit
  codes, logging, and upgrade behavior.
- Select and validate the exact unattended install strategy for REAPER on macOS:
  mounted DMG copy flow, packaged installer flow, or another vendor-supported
  non-interactive path.
- Confirm SWS and ReaPack macOS binaries expose reliable version metadata
  outside ReaPack's registry DB. If not, RAIS receipts and ReaPack DB should be
  treated as the reliable sources.
- Confirm whether ReaKontrol release binaries expose reliable version metadata
  on Windows and macOS. If not, RAIS receipts plus package-file presence should
  be treated as the reliable sources.
- Decide whether the RAIS update feed should be a GitHub release asset JSON
  generated by `release.yml`, a repository-hosted appcast/manifest file, or
  both.
- Validate the exact Windows self-update replacement mechanism for a running
  single executable: temporary helper executable, script, or another minimal
  relaunch approach.
- Validate the exact macOS self-update replacement mechanism for a signed and
  notarized app bundle without breaking code signing, quarantine, or app path
  stability.
- Decide how stable and beta RAIS release channels should be represented in the
  GitHub release/update metadata and in the UI.
- Decide whether first-version RAIS should install SWS directly from SWS
  release assets or through an unattended ReaPack-driven path after ReaPack is
  present. The design target remains unattended either way.
- Build a small wxDragon proof of concept and test it with NVDA, Narrator, and
  VoiceOver before expanding it into the full wizard.
- Verify whether wxDragon exposes the wxWidgets accessibility hooks RAIS needs
  directly. If not, document the smallest upstream contribution or local wrapper
  needed for accessible names, descriptions, roles, and state.
- Verify wxDragon/wxWidgets release packaging on Windows and macOS can meet the
  one-download, no-RAIS-installer goal without sacrificing code signing,
  notarization, or screen-reader behavior.
