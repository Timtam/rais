# RAIS — REAPER Accessibility Installation Software

RAIS sets up a fully accessible REAPER on Windows and macOS in a few clicks.
Instead of hunting through download pages, copying files into the right
folders, and fighting installers that fight your screen reader, you launch one
small program and it does the work.

RAIS installs and keeps up to date:

- **REAPER** — the DAW itself
- **OSARA** — the screen-reader extension that makes REAPER usable with
  NVDA, JAWS, Narrator, and VoiceOver
- **SWS** — the popular SWS Extension
- **ReaPack** — REAPER's package manager
- **ReaKontrol** — Native Instruments Komplete Kontrol support
- **JAWS-for-REAPER scripts** *(Windows only, when JAWS is detected)*

Built with screen reader users in mind: keyboard-first wizard, native
controls, NVDA/JAWS/Narrator/VoiceOver tested, German + English UI out of the
box. No console window, no installer, no settings file — one executable you
can run from any folder and delete when you're done.

## Download

Get the latest release from the [GitHub Releases
page](https://github.com/Timtam/rais/releases/latest). Each release publishes
versioned, per-platform downloads plus their SHA-256 sums. Pick the file that
matches your machine:

- **Windows (Intel/AMD 64-bit)**: `rais-<version>-windows-x86_64.exe`
- **Windows (ARM 64-bit)**: `rais-<version>-windows-aarch64.exe`
- **macOS (Apple Silicon)** — recommended: `rais-<version>-macos-aarch64.app.zip`
- **macOS (Intel 64-bit)** — recommended: `rais-<version>-macos-x86_64.app.zip`
- **macOS bare binary** (CLI use): `rais-<version>-macos-aarch64` /
  `rais-<version>-macos-x86_64`

On Windows, place the downloaded executable wherever you like (Desktop,
Downloads, a USB stick) and double-click it. You can rename it to `RAIS.exe`
if you prefer — RAIS still updates itself in place under whatever filename you
chose.

### macOS first launch

RAIS is distributed unsigned (the project doesn't pay for an Apple Developer
ID). Unzipping `rais-<version>-macos-<arch>.app.zip` gives you a `Rais` folder
containing `Rais.app` and an `Open Me First.command` helper. **Double-click
`Open Me First.command` once** — Terminal opens, the helper clears macOS's
first-launch quarantine on `Rais.app`, and you can close the window. From
then on `Rais.app` launches normally, and self-updates keep working without
re-triggering Gatekeeper.

If you'd rather not use the helper, you can clear the quarantine yourself in
Terminal:

```sh
xattr -dr com.apple.quarantine /path/to/Rais.app
```

…or use Apple's built-in path: open `Rais.app` once, dismiss the warning,
then go to **System Settings → Privacy & Security** and click **Open
Anyway** next to the entry for RAIS.

The bare `rais-<version>-macos-<arch>` download is a plain Mach-O CLI
executable (no `.app` wrapper). After downloading, run `chmod +x` and invoke
it from Terminal.

## Use it

Launch the downloaded executable. The wizard walks you through:

1. **Pick a REAPER target** — RAIS detects existing standard installs
   automatically; pick "portable" if you want a self-contained REAPER folder.
2. **RAIS checks for the latest versions** of REAPER and the accessibility
   packages.
3. **Pick the packages** you want installed or updated. Sensible defaults are
   already checked.
4. **Review and install.** RAIS downloads, verifies, and installs everything
   without further prompts.

When it finishes, you can launch REAPER straight from the wizard or open the
saved report.

### Switching the language

Use the language picker at the bottom of the window. Currently bundled
languages: English (United States) and Deutsch (Deutschland). RAIS auto-picks
your OS language on first launch when a translation is available.

## Command-line usage

The same `RAIS.exe` / `RAIS` executable also exposes a CLI when invoked with
arguments. Run `RAIS --help` for the full list. The most useful commands grouped
by what they do:

### See what you have

```
RAIS detect                                  # list detected REAPER installs
RAIS detect --portable C:\REAPER             # also probe a portable folder
RAIS components --resource-path "%APPDATA%\REAPER"
RAIS latest                                  # show latest upstream versions
```

### Plan an install or update

```
RAIS plan --resource-path "%APPDATA%\REAPER"
RAIS plan --resource-path "%APPDATA%\REAPER" --online
RAIS preflight --resource-path "%APPDATA%\REAPER"
```

`plan` prints what RAIS *would* do for the given REAPER target. Add
`--online` to compare detected versions against the live upstream feeds.

### Install and update

```
# One-shot setup of a portable REAPER + accessibility packages:
RAIS setup --resource-path C:\REAPER --portable --apply

# Update or install one specific package:
RAIS install-extension --package osara --resource-path "%APPDATA%\REAPER" --apply

# Install/update everything that needs it for an existing REAPER:
RAIS apply-packages --resource-path "%APPDATA%\REAPER" --apply
```

The CLI is dry-run by default; pass `--apply` to actually make changes.
`--save-report` writes a JSON report next to the resource path so you have a
record of what was installed.

### Maintain

```
RAIS backups --resource-path "%APPDATA%\REAPER"          # list rollback sets
RAIS restore-backup --resource-path "%APPDATA%\REAPER" \
     --backup-id unix-1234567890 --apply                  # roll back one set
```

### Update RAIS itself

```
RAIS self-update check                       # see if a new RAIS is out
RAIS self-update apply --restart             # update + relaunch
```

The GUI does this automatically on startup; the CLI commands are there for
unattended environments and CI.

## Reports and logs

Every installation produces a JSON report under `<resource>/RAIS/logs/`.
Backups go to `<resource>/RAIS/backups/<timestamp>/`. The download cache lives
in `%LOCALAPPDATA%\RAIS\cache` (Windows) or `~/Library/Caches/RAIS` (macOS) and
can be deleted safely at any time.

## Development

See [DESIGN.md](./DESIGN.md) for the full architecture and design rules. To
build from source you need a recent stable Rust toolchain. The wxDragon GUI
feature on Windows additionally needs the Visual Studio C++ build tools, an
LLVM `libclang.dll` discoverable through `LIBCLANG_PATH`, and Ninja on
`PATH`.

```
cargo fmt
cargo test --workspace
.\scripts\build-wxdragon-test.ps1            # Windows GUI smoke build
```

CI lives under `.github/workflows/`:

- `ci.yml` — formatting, tests, and release-mode artifacts on every push.
- `macos-smoke.yml` — daily live-upstream smoke against real REAPER + OSARA
  + SWS + ReaKontrol downloads.
- `release.yml` — builds tagged `v*` releases, signs and notarizes the
  artifacts when the corresponding repository secrets are configured, and
  publishes the GitHub Release with checksums and the self-update manifest.

Issues, pull requests, and translation contributions welcome — RAIS is for
the REAPER accessibility community first.
