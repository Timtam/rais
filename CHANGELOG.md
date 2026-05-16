# Changelog

All notable changes to RABBIT are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## How to update

Add entries under `## [Unreleased]` as part of each change. Use these
subsection headings (omit any that are empty):

- `### Added` — new features
- `### Changed` — changes to existing behavior
- `### Deprecated` — soon-to-be-removed features
- `### Removed` — now-removed features
- `### Fixed` — bug fixes
- `### Security` — security fixes

To cut a release, run the **prepare-release** workflow from the GitHub
Actions tab (workflow_dispatch) with the version `X.Y.Z` as input. It will:

1. Validate that `[Unreleased]` is non-empty and the tag is unused.
2. Bump the workspace `version` in `Cargo.toml` (and refresh `Cargo.lock`).
3. Insert `## [X.Y.Z] - YYYY-MM-DD` below `## [Unreleased]`, leaving a
   fresh empty `## [Unreleased]` on top.
4. Commit, tag `vX.Y.Z`, push both, and dispatch the release workflow.

The release workflow then extracts the section matching the tag's version
from this file and posts it as the GitHub release body.

## [Unreleased]

### Fixed

- Self-update apply in the GUI now exits the old RABBIT process after
  spawning the relaunched copy. Previously the swap completed and the
  new version launched, but the pre-update window kept running next to
  it because the apply callback never asked the wx event loop to shut
  down. The relaunch path now mirrors the language-switch relaunch and
  calls `std::process::exit(0)` once the new process has been spawned;
  the error path is unchanged so a failed spawn still leaves the
  original window open. CLI `apply --restart` was already correct.

## [0.2.0] - 2026-05-15

### Added

- New package: **Surge XT**, the free open-source hybrid synthesizer from
  the [Surge Synth Team](https://surge-synthesizer.github.io/). Opt-in,
  Windows + macOS, standard REAPER installations only. RABBIT runs the
  vendor installer (Inno Setup on Windows, productbuild-wrapped `.pkg`
  on macOS) under elevation so the VST3, CLAP, AU (macOS) and standalone
  formats land system-wide for REAPER and other DAWs to pick up. Tracks
  the rolling nightly channel at
  `surge-synthesizer/surge` releases tag `Nightly` rather than the
  stable 1.3.4 release (2024-08-11) — the project effectively ships
  through nightlies now. Version detection layers a `NIGHTLY-<date>-<sha>`
  token from the receipt over a Medium-confidence semver fallback read
  from the vendor-installed VST3 bundle's file metadata.
- macOS elevation primitive in `rabbit-platform::elevation`: wraps the
  elevated command in `osascript -e 'do shell script "…" with
  administrator privileges'` so the system raises its native
  AuthorizationServices dialog. First (and currently only) consumer is
  the Surge XT `MountDiskImageAndRunPkgInstaller` runner.
- New `PlannedExecutionKind::MountDiskImageAndRunPkgInstaller` runner:
  mounts a `.dmg`, locates the inner `.pkg` via a filename-suffix glob
  matched against the mounted volume root, invokes `/usr/sbin/installer
  -pkg <path> -target /` under admin authorization, and detaches the
  image whether the install succeeded or failed.

- Automatic ReaPack script-action preservation across OSARA key-map
  replacement. Replacing `reaper-kb.ini` with `OSARA.ReaperKeyMap` —
  the default OSARA-recommended flow — drops every `SCR` line ReaPack
  had registered through REAPER's `AddRemoveReaScript` API, so installed
  ReaScripts disappear from REAPER's actions list until the user runs
  "ReaPack: Synchronize packages" inside REAPER (or re-installs every
  package). The unattended replacement path now reads the existing
  `reaper-kb.ini` first, captures all of its `SCR` records, lets the
  OSARA key map overwrite the file as before, then re-appends the
  preserved lines using the written file's newline convention. Any
  user `KEY` binding that targeted one of those scripts keeps working —
  REAPER derives the `_RS<hex>` action command ID deterministically
  from the script path, so the re-appended SCR lines bind to the same
  IDs the user already has in their key map. No opt-out: the
  preservation always runs when the OSARA key map is replaced.

- Live per-package progress reporting on the wizard's Installation
  progress page. The previous progress page set the gauge to 10 % when
  install kicked off, then jumped straight to 100 % at the end —
  everything in between was a black box even though packages like
  REAPER's macOS dmg take ~30 MB of network transfer per install.
  The setup pipeline now emits structured `ProgressEvent`s
  (`DownloadStarted` / `DownloadProgress` / `DownloadCompleted` /
  `InstallStarted` / `InstallCompleted` / `ConfigurationStarted` /
  `ConfigurationCompleted`) through an optional `ProgressReporter`
  threaded down from `execute_setup_operation_with_progress` into
  `download_artifacts_with_progress` and
  `install_cached_artifacts_with_progress`. The artifact downloader
  swapped `std::io::copy` for a chunked read/write loop that emits a
  byte-progress event every ~256 KiB or ~200 ms (whichever is rarer),
  so the gauge moves smoothly during the REAPER dmg pull instead of
  stalling. The wxdragon wizard forwards each event to the UI thread
  via `wxdragon::call_after`: the gauge advances by a per-phase
  fraction (weighted by completed downloads/installs plus the
  in-flight byte fraction), the status label updates to "Downloading
  REAPER… 12.4 MB / 30.0 MB", and a running log of completed
  transitions appends to the progress details TextCtrl (screen
  readers announce each new line as it lands). The no-progress
  entry points (`execute_setup_operation`,
  `execute_resolved_setup_operation`, `download_artifacts`,
  `install_cached_artifacts`, `execute_wizard_install`) stay on
  their existing signatures and delegate via `ProgressReporter::noop`,
  so the CLI and existing tests are unaffected.

### Fixed

- Wizard startup detection no longer SHA-256-hashes every receipted
  install file. `verify_package_receipt` used to verify each entry in
  a package's receipt by hashing the on-disk file, which on Windows
  meant 14 seconds of stalled UI just for FFmpeg (~200 MB of DLLs)
  and ~1 second for REAPER on every wizard launch — plus another
  round of the same after every install via the post-install rescan
  hook. The receipt verifier now checks file existence and size only;
  size mismatch alone catches every realistic regression the detection
  layer cares about (partial overwrites, truncated files), and the
  receipt's own stamped version is what gets shown either way.
  Recording hashes during install is unchanged.

- FFmpeg version detection no longer freezes the wizard for tens of
  seconds on Windows when an FFmpeg install is present. Probe 2
  previously called `ffmpeg.exe -version` via `std::process::Command`
  on the UI thread, which on Windows blocks for the entire AV scan of
  FFmpeg's dozens of DLL dependencies — easily 20-30 s per launch on
  a default-configured machine, and the same stall ran on the
  post-install rescan after installing FFmpeg too. Probe 2 now scans
  the `ffmpeg.exe` binary for the contiguous `show_banner` format
  string anchored on the unique `the FFmpeg developers` literal, and
  pulls `<VERSION>` out between the trailing `version ` token and the
  next `Copyright` marker. That matches both the upstream FFmpeg
  banner (`%s version <VERSION>, Copyright (c) …`) and the Gyan.dev
  full-builds variant that drops the comma and pads with spaces
  (`%s version 8.1.1-full_build-www.gyan.dev         Copyright (c) …`),
  so externally-installed Gyan FFmpegs now report `8.1.1` instead of
  the libavformat-major fallback's `8.0.0`. Same `High` confidence as
  before; the matching detector id changed from `ffmpeg-cli-version`
  to `ffmpeg-binary-version-string`.

## [0.1.2] - 2026-05-13

### Changed

- Self-update is now a modal Yes/No prompt at startup instead of a
  button on the wizard's Done page. The previous design was effectively
  unreachable: users had to finish an install before they ever saw the
  "Apply RABBIT update" button, and the always-visible status bar line
  pointed at a button most users couldn't find. The startup self-update
  check now raises a Yes/No dialog as soon as it completes; "Yes" runs
  the apply inline (with progress in the status bar) and relaunches
  RABBIT, "No" dismisses the prompt for the rest of the session. Users
  who change their mind can relaunch RABBIT to be re-prompted; the
  status-bar line spells that out.

### Fixed

- Per-arch artifact dispatch on a fresh first-time install. On macOS
  with no existing `/Applications/REAPER.app`, the binary-header probe
  in `standard_macos_installation` couldn't read a file that wasn't
  there yet and returned `Architecture::Unknown`. The SWS and ReaPack
  resolvers then fell through to their `Unknown → X64` fallback arms
  and downloaded `sws-…-Darwin-x86_64.dmg` and
  `reaper_reapack-x86_64.dylib`, even on Apple Silicon hosts running
  natively where the freshly-installed REAPER would launch as `arm64`
  and refuse to load the mismatched extension binaries. The dispatch-
  time canonicalizer (renamed from `canonicalize_macos_universal_arch`
  to `canonicalize_dispatch_arch`) now collapses `Unknown` to the host
  slice the same way it collapses `Universal` — `Architecture::current()`
  with Rosetta correction — so the upcoming install lands arch-correct
  plug-ins regardless of whether REAPER was already on disk when the
  wizard launched. The fix also closes the equivalent x64-fallback bug
  on Windows-on-ARM, where an unprobed target would have produced
  `Windows-x64.exe` SWS and `reaper_reapack-x64.dll` instead of the
  arm64ec variants.

- macOS Rosetta detection. v0.1.1 shelled out to `/usr/sbin/sysctl -n
  sysctl.proc_translated` to determine whether RABBIT was running
  under Rosetta, but `sysctl.proc_translated` reports the *querying*
  process's translation state — and the shelled-out `sysctl` binary
  always runs as the host's native arch (the kernel picks its native
  slice at exec time, ignoring the parent's translation state). The
  probe therefore always reported `false`, including when RABBIT
  itself was the translated `x86_64` slice on Apple Silicon. The
  artifact dispatcher then canonicalized REAPER-Universal to
  `Architecture::current()` (i.e., `X64`) and installed `x86_64`
  plug-ins against an `arm64`-native REAPER process. The probe now
  calls `sysctlbyname` directly via FFI from RABBIT's own process, so
  the kernel resolves the key against RABBIT's translation state.

- macOS self-update used to leave `Rabbit.app` structurally invalid
  after the binary swap. The release pipeline now ad-hoc signs the
  bare `rabbit-<version>-macos-universal` artifact itself (so the
  staged-in binary has a valid embedded signature even though
  `lipo -create` strips its inputs' sigs), and `apply_self_update`
  re-seals the enclosing `.app` bundle with `codesign --force --deep
  --sign -` after the swap. Both pieces are needed: the bare-binary
  signing keeps Apple Silicon's exec checks happy and the bundle
  re-seal restores the `_CodeSignature/CodeResources` consistency that
  Gatekeeper checks on Finder launch. Without these, post-update
  Finder launches on macOS 15 (Sequoia) and 26 (Tahoe) would refuse
  the bundle as corrupt rather than just untrusted.

## [0.1.1] - 2026-05-10

### Added

- Stable always-latest download URLs for every platform, e.g.
  `https://github.com/Timtam/rabbit/releases/latest/download/rabbit-windows-x86_64.exe`
  and `…/rabbit-macos-universal.app.zip`. The release pipeline now
  publishes version-less aliases of each artifact alongside the
  versioned originals, so the URLs above resolve to whatever release is
  current at the time of the click. The README's Download section has
  been switched to direct links; the GitHub Releases page is still
  there for users who want to pin a version or verify SHA-256 sums.

### Changed

- macOS: ship a single universal Mach-O instead of separate Apple Silicon
  and Intel builds. The release pipeline now builds both arches and
  `lipo -create`s them into one binary, published as
  `rabbit-<version>-macos-universal` (and `…-universal.app.zip`). The
  self-update manifest's `platforms` map keeps its `macos-aarch64` and
  `macos-x86_64` keys for backward compatibility with already-released
  RABBIT 0.1.0 clients, both pointing at the universal artifact, so
  existing installs migrate to the fat binary on their next self-update
  check without a manual download.

### Fixed

- macOS: `Cmd+C` (and `Cmd+S` in the German build) no longer closes the
  wizard. Wizard buttons used `&Close` / `&Schließen`-style mnemonics
  for Alt-key access on Windows and Linux, but wxWidgets' OSX backend
  binds those `&` mnemonics as `Cmd+letter` accelerators — colliding
  with macOS system shortcuts (Cmd+C copy, Cmd+S save, …). Mnemonics
  on buttons aren't HIG-conformant on macOS anyway, so they're now
  stripped from the label entirely on that platform; underlined
  Alt-key access continues to work on Windows and Linux.

- macOS first-launch helper: `Open Me First.command` no longer falsely
  reports success on macOS 15 (Sequoia) and 26 (Tahoe). Removing the
  `com.apple.quarantine` xattr is no longer enough on those versions —
  Gatekeeper blocks unsigned bundles on first launch regardless of
  quarantine state. The helper now (a) verifies the xattr was actually
  cleared *recursively* across the bundle (the previous version checked
  only the bundle root, missing inner-file failures), (b) detects the
  macOS version, and (c) on macOS 15+ triggers a launch attempt to
  register Rabbit.app with Gatekeeper, then deep-links System Settings
  → Privacy & Security so the user's "Open Anyway" approval is one
  click away. macOS 14 and earlier keep the original quiet behavior.

- macOS: install no longer aborts with
  `no artifact found for sws on MacOs/Universal` (and the analogous
  silent ReaPack-arm64 mis-install on Intel hosts) when REAPER is a
  universal Mach-O. The artifact dispatcher now canonicalizes
  `Architecture::Universal` to the host slice on macOS before
  per-package resolvers run, so SWS picks the matching `Darwin-x86_64`
  / `Darwin-arm64` `.dmg` and ReaPack picks the matching
  `reaper_reapack-x86_64.dylib` / `reaper_reapack-arm64.dylib`. On
  Apple Silicon Macs running RABBIT under Rosetta, the dispatcher
  consults `sysctl.proc_translated` and forces the `arm64` slice so
  plug-ins match the `arm64`-native REAPER process rather than the
  `x86_64` translator's view.

- macOS: VoiceOver now reads the German UI with a German voice. The
  bundle previously declared only English (`CFBundleDevelopmentRegion`
  with no `CFBundleLocalizations` and no `.lproj` directories), so
  Cocoa picked the English voice for every accessibility string
  regardless of the in-app language. The bundle now ships
  `CFBundleLocalizations` for `en` and `de`, matching empty
  `en.lproj` / `de.lproj` stubs, and seats `AppleLanguages` from the
  resolved runtime locale before wxDragon brings up Cocoa so the
  override also takes effect for users on an English-language macOS
  who switch RABBIT to German.

## [0.1.0] - 2026-05-09

Initial public release. RABBIT is a REAPER accessibility bootstrap and
bundle installer with a screen-reader-friendly GUI wizard and a matching
CLI, packaged as a single self-contained executable per platform.

### Added

- **Accessibility-first wizard** built on wxDragon: keyboard-first
  navigation, native controls, tested with NVDA, JAWS, Narrator, and
  VoiceOver. Wizard runs when launched without arguments; arguments hand
  off to the CLI.
- **Cross-platform builds**: Windows x86_64, Windows aarch64, macOS
  aarch64, and macOS x86_64. macOS ships as an ad-hoc-signed `.app.zip`
  with an `Open Me First.command` helper that clears Gatekeeper
  quarantine on first launch.
- **Localization**: English (en-US) and German (de-DE) bundled, with
  Fluent-based runtime locale selection. Auto-picks the OS language when
  a translation is available.
- **REAPER discovery**: detects standard installations on Windows and
  macOS, plus user-supplied portable folders. Reports app path, resource
  path, version, architecture, writability, and confidence.
- **Package install / update** for REAPER, OSARA, SWS, ReaPack,
  ReaKontrol, JAWS-for-REAPER scripts (Windows + JAWS only), and FFmpeg
  shared runtime (Windows, opt-in; pulled from gyan.dev on x64 and
  tordona/ffmpeg-win-arm64 on ARM64, pinned to FFmpeg 8.x).
- **Architecture-aware artifact resolution**: selects x86_64 / aarch64 /
  arm64ec / universal builds appropriate to the detected REAPER, with
  per-arch self-update assets in the release manifest.
- **Configuration steps**: post-install tweaks managed alongside package
  installs. Ships with `reapack-add-reaper-accessibility-remote`, which
  adds the REAPER Accessibility ReaPack repository
  (`Timtam/reapack`) and is idempotent on re-runs.
- **Preflight checks** that run before any apply: REAPER process state,
  resource path writability, donation acknowledgement for ReaPack, etc.
  CLI returns a non-zero exit when checks fail.
- **Dry-run by default in the CLI**: every install / update / restore
  command requires `--apply` to make changes. JSON output available on
  every command via `--json`.
- **Backups and rollback**: each install writes a backup set under
  `<resource>/RABBIT/backups/<timestamp>/`; `rabbit backups` lists them
  and `rabbit restore-backup` rolls one back.
- **JSON + text reports** under `<resource>/RABBIT/logs/` for every
  install, update, restore, and setup operation.
- **Self-update**: signed update manifest published alongside each
  GitHub release with per-arch URLs and SHA-256 sums; `rabbit
  self-update check / stage / apply [--restart]` (CLI) and an automatic
  on-startup check (GUI). Handles bare-binary replacement on Windows and
  macOS without the user re-clearing Gatekeeper.
- **Portable runtime**: the binary carries embedded resources (locales,
  package manifest). Cache and lock files live under the OS cache directory by
  default; nothing is left behind in the resource path beyond explicit
  reports and backups.
