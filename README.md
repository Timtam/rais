# RAIS

RAIS is the REAPER Accessibility Installation Software. It installs and updates
REAPER accessibility-related components such as OSARA, SWS, and ReaPack.

This repository currently contains the first implementation slice:

- a Rust workspace
- core data models
- REAPER installation discovery for standard and portable paths
- component detection in a REAPER resource path
- RAIS install receipts
- install/update plan generation
- embedded package metadata for OSARA, SWS, and ReaPack
- a diagnostic CLI
- a wxDragon UI crate with an accessible wizard model and native shell
- an English localization seed

The wxDragon shell is still early, but the core and UI crates now share embedded
localization and embedded package metadata so development builds do not require
neighboring resource directories.

## Commands

```powershell
cargo run -p rais-cli -- detect
cargo run -p rais-cli -- detect --json
cargo run -p rais-cli -- detect --portable C:\path\to\portable\REAPER
cargo run -p rais-cli -- components --resource-path "$env:APPDATA\REAPER"
cargo run -p rais-cli -- plan --resource-path "$env:APPDATA\REAPER"
cargo run -p rais-cli -- latest
cargo run -p rais-cli -- plan --resource-path "$env:APPDATA\REAPER" --online
cargo run -p rais-cli -- artifacts
cargo run -p rais-cli -- artifacts --package reaper --architecture x64
cargo run -p rais-cli -- download --package reapack
cargo run -p rais-cli -- packages
cargo run -p rais-cli -- packages --manifest --json
cargo run -p rais-cli -- preflight --resource-path "$env:APPDATA\REAPER"
cargo run -p rais-cli -- init-resource --resource-path C:\path\to\portable\REAPER --portable
cargo run -p rais-cli -- init-resource --resource-path C:\path\to\portable\REAPER --portable --apply
cargo run -p rais-cli -- backups --resource-path "$env:APPDATA\REAPER"
cargo run -p rais-cli -- restore-backup --resource-path "$env:APPDATA\REAPER" --backup-id unix-1234567890
cargo run -p rais-cli -- restore-backup --resource-path "$env:APPDATA\REAPER" --backup-id unix-1234567890 --apply
cargo run -p rais-cli -- install-extension --package reapack --resource-path "$env:APPDATA\REAPER"
cargo run -p rais-cli -- install-extension --package reapack --resource-path "$env:APPDATA\REAPER" --apply
cargo run -p rais-cli -- install-extension --package reapack --resource-path "$env:APPDATA\REAPER" --apply --allow-reaper-running
cargo run -p rais-cli -- apply-packages --resource-path "$env:APPDATA\REAPER"
cargo run -p rais-cli -- apply-packages --resource-path "$env:APPDATA\REAPER" --apply
cargo run -p rais-cli -- apply-packages --resource-path "$env:APPDATA\REAPER" --apply --preserve-osara-keymap
cargo run -p rais-cli -- apply-packages --resource-path "$env:APPDATA\REAPER" --stage-unsupported
cargo run -p rais-cli -- setup --resource-path C:\path\to\portable\REAPER --portable --stage-unsupported
cargo run -p rais-cli -- setup --resource-path C:\path\to\portable\REAPER --portable --stage-unsupported --apply
cargo run -p rais-cli -- setup --resource-path C:\path\to\portable\REAPER --portable --stage-unsupported --apply --preserve-osara-keymap
cargo run -p rais-cli -- setup --resource-path C:\path\to\portable\REAPER --portable --stage-unsupported --save-report
cargo run -p rais-cli -- setup --resource-path C:\path\to\portable\REAPER --portable --stage-unsupported --report-path C:\path\to\report.json
cargo run -p rais-cli -- locales
cargo run -p rais-cli -- localize --id app-title
cargo run -p rais-cli -- localize --id status-package-installed --arg package=ReaPack
cargo run -p rais-cli -- portable-check --locales-dir target\missing-locales
cargo run -p rais-ui-wxdragon
cargo run -p rais-ui-wxdragon --features gui
.\scripts\build-wxdragon-test.ps1
```

`packages` prints the current-platform package specs. `packages --manifest`
prints the embedded package manifest, including package kind, supported
platforms and architectures, providers, detector hints, install steps, and
backup policy.

For `preflight`, `init-resource`, `install-extension`, `apply-packages`, and
`setup`, pass `--target-app-path` when the selected REAPER application path is
known and differs from what RAIS can infer from the resource path alone. This
improves running-process checks and package-specific manual instructions for
standard installs and custom targets.

`rais-ui-wxdragon` is the native UI crate. Its default build exercises the Rust
wizard model without requiring wxWidgets native libraries. Build it with
`--features gui` to run the wxDragon window. The UI defaults to embedded
localization and embedded package metadata so it can start without neighboring
resource directories. The Install button now runs the shared setup engine on a
background thread, updates the progress page, and writes a completion report to
the Done page. Current engine support automatically copies direct extension
binaries such as ReaPack. It also now includes unattended upstream-installer
paths for:
- REAPER on Windows
- OSARA on Windows, including default key map replacement with backup and a
  preserve-current opt-out
- SWS on Windows

Those unattended Windows installer paths now also update RAIS install receipts,
so later detection can verify the installed state through `RAIS/install-state.json`
instead of relying only on best-effort file presence or metadata fallbacks.

Other upstream installers and archives are still downloaded or reported for
manual attention until their package-specific execution steps are added.
The target design in [DESIGN.md](./DESIGN.md) is full unattended installation
and update of REAPER, OSARA, SWS, and ReaPack, including RAIS launching
executable installers itself during the install run where needed, so those
manual-attention paths are current implementation gaps rather than intended
product behavior.
The first wizard page lists detected REAPER installations and includes a
separate portable target option. Choosing that portable option enables the
native directory picker for selecting an existing portable REAPER folder or an
empty folder where RAIS should create the portable resource layout.

For accessibility and layout testing, run:

```powershell
.\scripts\build-wxdragon-test.ps1
.\target\wxdragon-test\RAIS-wxdragon-test.exe
```

The script builds the wxDragon GUI feature and copies the resulting executable
to a stable test path. During implementation, each completed UI iteration should
refresh `target\wxdragon-test\RAIS-wxdragon-test.exe` so it is ready to launch.
On Windows/MSVC, the wxDragon test executable embeds a Common Controls v6
manifest so wxWidgets uses the current native controls and does not show its
deprecated missing-manifest warning.

On Windows, the wxDragon feature currently requires native build prerequisites
from wxDragon/wxWidgets:

- Visual Studio Build Tools with the C++ toolchain
- LLVM `libclang.dll` discoverable through `LIBCLANG_PATH`
- Ninja on `PATH`

Example PowerShell setup before a GUI build:

```powershell
$env:LIBCLANG_PATH = "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\Llvm\x64\bin"
winget install --id Ninja-build.Ninja -e
cargo run -p rais-ui-wxdragon --features gui
```

## Development

```powershell
cargo fmt
cargo test
```

## CI/CD

GitHub Actions workflow files live under `.github/workflows/`:

- `ci.yml`: runs formatting/tests on Windows and macOS and uploads release-style
  build artifacts for every push and pull request
- `release.yml`: builds tagged `v*` releases, publishes GitHub Release assets,
  emits checksums, and generates `rais-update-stable.json` for future RAIS
  self-update support
