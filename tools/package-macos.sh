#!/usr/bin/env bash
# Assembles the macOS release package from a built Launcher binary.
#
#   ./tools/package-macos.sh <path/to/Launcher> <OutDir>
#
# Writes:
#   <Out>/Launcher/Launcher.app             the macOS application bundle
#   <Out>/Launcher/launcher-manifest.json   every file with its size and SHA-256
#   <Out>/Launcher-macos.zip                zip archive for portable use & self-updates
#   <Out>/Launcher-macos.dmg                disk image for standard macOS installation
#   <Out>/release-entry-macos.json          this release's entry for update-macos.json

set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "Usage: $0 <path/to/Launcher> <OutDir>" >&2
  exit 1
fi

EXE="$1"
OUT="$2"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)"

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
FOLDER="$OUT/Launcher"
APP_BUNDLE="$FOLDER/Launcher.app"
ZIP_PATH="$OUT/Launcher-macos.zip"
DMG_PATH="$OUT/Launcher-macos.dmg"
MANIFEST="launcher-manifest.json"
EXE_REL="Launcher.app/Contents/MacOS/Launcher"

rm -rf "$FOLDER" "$ZIP_PATH" "$DMG_PATH"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"

cp "$EXE" "$APP_BUNDLE/Contents/MacOS/Launcher"
chmod 755 "$APP_BUNDLE/Contents/MacOS/Launcher"

if [ -f "$ROOT/assets/launcher.icns" ]; then
  cp "$ROOT/assets/launcher.icns" "$APP_BUNDLE/Contents/Resources/launcher.icns"
fi

cat > "$APP_BUNDLE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>Javacrafter's Launcher</string>
  <key>CFBundleExecutable</key>
  <string>Launcher</string>
  <key>CFBundleIconFile</key>
  <string>launcher.icns</string>
  <key>CFBundleIdentifier</key>
  <string>com.javacrafter.launcher</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Launcher</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

if [ -d "$ROOT/package" ]; then
  cp -R "$ROOT/package/"* "$FOLDER/" 2>/dev/null || true
fi

# Ad-hoc sign the bundle BEFORE hashing so _CodeSignature and the signed Mach-O
# binary match the hashes recorded in launcher-manifest.json.
if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP_BUNDLE"
fi

python3 - "$FOLDER" "$VERSION" "$EXE_REL" "$MANIFEST" "$ZIP_PATH" "$OUT/release-entry-macos.json" <<'PY'
import hashlib
import json
import os
import stat
import sys
import zipfile

folder, version, exe_rel, manifest_name, zip_path, entry_path = sys.argv[1:7]

files = []
for root, dirs, filenames in os.walk(folder):
    dirs[:] = [d for d in dirs if d.lower() != "data"]
    for name in filenames:
        if name == manifest_name or name == ".DS_Store":
            continue
        full = os.path.join(root, name)
        rel = os.path.relpath(full, folder).replace("\\", "/")
        h = hashlib.sha256()
        with open(full, "rb") as f:
            while chunk := f.read(65536):
                h.update(chunk)
        files.append({
            "path": rel,
            "size": os.path.getsize(full),
            "sha256": h.hexdigest().lower(),
        })

files.sort(key=lambda item: item["path"])

manifest = {
    "version": version,
    "exe": exe_rel,
    "files": files,
}
manifest_full = os.path.join(folder, manifest_name)
with open(manifest_full, "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2)

with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for rel in [item["path"] for item in files] + [manifest_name]:
        src = os.path.join(folder, rel)
        st = os.stat(src)
        zi = zipfile.ZipInfo.from_file(src, arcname=rel)
        zi.compress_type = zipfile.ZIP_DEFLATED
        zi.create_system = 3  # Unix
        mode = stat.S_IMODE(st.st_mode)
        if rel == exe_rel or ".app/Contents/MacOS/" in rel:
            mode |= 0o755
        zi.external_attr = (stat.S_IFREG | mode) << 16
        with open(src, "rb") as f:
            zf.writestr(zi, f.read())

zip_hash = hashlib.sha256()
with open(zip_path, "rb") as f:
    while chunk := f.read(65536):
        zip_hash.update(chunk)

entry = {
    "version": version,
    "exe": exe_rel,
    "package": {
        "name": os.path.basename(zip_path),
        "size": os.path.getsize(zip_path),
        "sha256": zip_hash.hexdigest().lower(),
    },
    "files": files,
}
with open(entry_path, "w", encoding="utf-8") as f:
    json.dump(entry, f, indent=2)
PY

# Also build a standard macOS .dmg with an Applications shortcut.
if command -v hdiutil >/dev/null 2>&1; then
  DMG_STAGE="$OUT/dmg-stage"
  rm -rf "$DMG_STAGE"
  mkdir -p "$DMG_STAGE"
  cp -R "$APP_BUNDLE" "$DMG_STAGE/Launcher.app"
  cp "$FOLDER/$MANIFEST" "$DMG_STAGE/$MANIFEST"
  ln -s /Applications "$DMG_STAGE/Applications"
  hdiutil create -volname "Javacrafter's Launcher" \
    -fs HFS+ \
    -srcfolder "$DMG_STAGE" \
    -ov -format UDZO "$DMG_PATH" >/dev/null || true
  rm -rf "$DMG_STAGE"
fi

echo "Packaged macOS Launcher $VERSION -> $ZIP_PATH"
