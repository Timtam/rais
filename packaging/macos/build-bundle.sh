#!/usr/bin/env bash
# Assembles a Rabbit.app bundle from a built rabbit binary, then wraps it in a
# zip alongside an "Open Me First.command" helper that clears macOS's
# first-launch quarantine. RABBIT ships unsigned, so the helper is the
# friction-free path users take in place of right-click → Open or
# `xattr -dr com.apple.quarantine`.
#
# Zip layout (one wrapper folder so both items extract together):
#   Rabbit/
#     Rabbit.app/Contents/{Info.plist,MacOS/rabbit,Resources,PkgInfo}
#     Open Me First.command
set -euo pipefail

usage() {
	cat >&2 <<'USAGE'
Usage: build-bundle.sh --binary <path> --version <x.y.z> --out <dir> --zip-name <name.zip>
  --binary     Path to the built rabbit Mach-O executable.
  --version    Version string to embed in CFBundleVersion / CFBundleShortVersionString.
  --out        Output directory; will be created if missing. Both Rabbit.app and the zip land here.
  --zip-name   Filename for the zipped bundle (e.g. rabbit-0.1.0-macos-aarch64.app.zip).
  --adhoc-sign Optionally ad-hoc sign the bundle (codesign -s -). Off by default.
USAGE
	exit 64
}

BINARY=""
VERSION=""
OUT_DIR=""
ZIP_NAME=""
ADHOC_SIGN=0

while [ $# -gt 0 ]; do
	case "$1" in
		--binary) BINARY="$2"; shift 2 ;;
		--version) VERSION="$2"; shift 2 ;;
		--out) OUT_DIR="$2"; shift 2 ;;
		--zip-name) ZIP_NAME="$2"; shift 2 ;;
		--adhoc-sign) ADHOC_SIGN=1; shift ;;
		-h|--help) usage ;;
		*) echo "unknown argument: $1" >&2; usage ;;
	esac
done

if [ -z "$BINARY" ] || [ -z "$VERSION" ] || [ -z "$OUT_DIR" ] || [ -z "$ZIP_NAME" ]; then
	usage
fi
if [ ! -f "$BINARY" ]; then
	echo "binary not found: $BINARY" >&2
	exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INFO_PLIST_TEMPLATE="$SCRIPT_DIR/Info.plist"
if [ ! -f "$INFO_PLIST_TEMPLATE" ]; then
	echo "Info.plist template missing at $INFO_PLIST_TEMPLATE" >&2
	exit 1
fi

mkdir -p "$OUT_DIR"
APP_DIR="$OUT_DIR/Rabbit.app"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

# Substitute the version token. Escape any '/' or '&' so sed doesn't
# misinterpret them — versions with build metadata can contain '+'.
ESCAPED_VERSION="$(printf '%s' "$VERSION" | sed -e 's/[\/&]/\\&/g')"
sed -e "s/@VERSION@/$ESCAPED_VERSION/g" "$INFO_PLIST_TEMPLATE" > "$APP_DIR/Contents/Info.plist"

cp "$BINARY" "$APP_DIR/Contents/MacOS/rabbit"
chmod +x "$APP_DIR/Contents/MacOS/rabbit"

# PkgInfo is optional but Launch Services historically reads it. APPL????
# matches CFBundlePackageType + CFBundleSignature in Info.plist.
printf 'APPL????' > "$APP_DIR/Contents/PkgInfo"

if [ "$ADHOC_SIGN" -eq 1 ]; then
	# Ad-hoc signing (-s -) doesn't satisfy Gatekeeper for distribution but
	# avoids the "damaged and can't be opened" error that hits unsigned
	# binaries on Apple Silicon for downloads carrying the quarantine bit.
	# First-launch trust is cleared by the bundled "Open Me First.command"
	# helper below or by `xattr -dr com.apple.quarantine`.
	codesign --force --deep --sign - "$APP_DIR"
fi

# Stage Rabbit.app + the unquarantine helper under a single wrapper folder so
# both extract together when the user double-clicks the zip.
STAGE_DIR="$OUT_DIR/.bundle-stage"
WRAPPER_NAME="Rabbit"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/$WRAPPER_NAME"
mv "$APP_DIR" "$STAGE_DIR/$WRAPPER_NAME/Rabbit.app"
APP_DIR="$STAGE_DIR/$WRAPPER_NAME/Rabbit.app"

cat > "$STAGE_DIR/$WRAPPER_NAME/Open Me First.command" <<'HELPER'
#!/bin/bash
# RABBIT ships unsigned (no Apple Developer Program enrollment). Running this
# helper once clears macOS's first-launch quarantine on Rabbit.app so it
# launches normally from Finder. Future self-updates inherit the trust.
set -u

DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET="$DIR/Rabbit.app"

pause() {
	# Keep the Terminal window open after the script finishes so the user can
	# read the result regardless of their Terminal "When the shell exits"
	# preference.
	echo
	printf "Press Return to close this window. "
	read -r _ || true
}

echo "RABBIT first-launch trust helper"
echo "================================"
echo

if [ ! -d "$TARGET" ]; then
	echo "Rabbit.app was not found next to this helper at:"
	echo "  $TARGET"
	echo
	echo "Make sure both items (Rabbit.app and 'Open Me First.command')"
	echo "extracted into the same folder, then run this helper again."
	pause
	exit 1
fi

echo "Clearing macOS quarantine from:"
echo "  $TARGET"
echo

# Don't silence stderr — if xattr complains we want the user (and us) to see
# why. -dr removes com.apple.quarantine recursively from every file inside
# the .app so the inner Mach-O and frameworks lose their download markers.
xattr -dr com.apple.quarantine "$TARGET" || true

# Verify the attribute is actually gone from the top-level bundle. The most
# common silent failure is Terminal lacking the Files-and-Folders permission
# needed to write extended attributes when the bundle sits on Desktop,
# Documents, or iCloud Drive — the call returns success-ish but the attr
# stays. Catch that here instead of telling the user "all done".
if xattr -p com.apple.quarantine "$TARGET" >/dev/null 2>&1; then
	echo
	echo "ERROR: com.apple.quarantine is still attached to Rabbit.app."
	echo "Removing it did not take effect. Common causes:"
	echo
	echo "  - The Rabbit folder is on Desktop, Documents, or iCloud Drive and"
	echo "    Terminal does not have permission to modify files there."
	echo "    Fix: move the Rabbit folder to ~/Downloads and run this helper"
	echo "    again, OR open System Settings -> Privacy & Security ->"
	echo "    Files and Folders, find Terminal, and enable access for the"
	echo "    folder you extracted into."
	echo
	echo "  - Rabbit.app is still inside the downloaded .zip (read-only)."
	echo "    Fix: extract the Rabbit folder to a writable location first."
	echo
	echo "Manual fallback (run in Terminal):"
	echo "  xattr -dr com.apple.quarantine \"$TARGET\""
	echo
	echo "Or open Rabbit.app once, dismiss the warning dialog, then click"
	echo "'Open Anyway' in System Settings -> Privacy & Security."
	pause
	exit 1
fi

echo
echo "Rabbit.app is now trusted."
echo "You can close this window and double-click Rabbit.app to launch RABBIT."
pause
HELPER
chmod +x "$STAGE_DIR/$WRAPPER_NAME/Open Me First.command"

ZIP_PATH="$OUT_DIR/$ZIP_NAME"
rm -f "$ZIP_PATH"
# `ditto -c -k --keepParent` preserves the executable bit and resource forks
# (plain `zip` does not, which would produce a broken .app on extract). The
# wrapper folder keeps Rabbit.app + the helper grouped after extraction.
ditto -c -k --keepParent "$STAGE_DIR/$WRAPPER_NAME" "$ZIP_PATH"

shasum -a 256 "$ZIP_PATH" | awk -v name="$ZIP_NAME" '{print tolower($1) "  " name}' > "$ZIP_PATH.sha256"

rm -rf "$STAGE_DIR"

echo "wrote zip:    $ZIP_PATH"
echo "wrote sha256: $ZIP_PATH.sha256"
