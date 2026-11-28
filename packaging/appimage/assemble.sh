#!/bin/bash
# Builds the Snow Launcher AppImage. Runs inside the build container (see
# build.sh), from the repository root. Output: target/appimage/.
set -euo pipefail

app_id=io.github.ninjairocode.SnowLauncher
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
out=target/appimage
appdir="$out/AppDir"
work="$out/work"

# The AppImage runtime: a fixed release, checked with its GPG signature
# against the fingerprint recorded here (experiment E7, project.md 10).
runtime_tag=20251108
runtime_fingerprint=570C77ACEA40C0F1B758902CBF96CCA56490F695

echo "==> Building snow-launcher, snowd and snowctl ${version} (release)"
cargo build --release --locked \
    -p snowlauncher-gui -p snowlauncher-daemon -p snowlauncher-cli

echo "==> Assembling the AppDir"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" \
         "$appdir/usr/share/applications" \
         "$appdir/usr/share/icons/hicolor/scalable/apps" \
         "$appdir/usr/share/metainfo"
release="${CARGO_TARGET_DIR:-target}/release"
install -m 0755 "$release/snow-launcher" "$release/snowd" "$release/snowctl" "$appdir/usr/bin/"
install -m 0755 packaging/appimage/AppRun "$appdir/AppRun"
install -m 0644 "data/$app_id.desktop" "$appdir/usr/share/applications/"
install -m 0644 "data/icons/$app_id.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/"
install -m 0644 "data/$app_id.metainfo.xml" "$appdir/usr/share/metainfo/"
# The AppImage format wants the entry and its icon at the top too.
ln -s "usr/share/applications/$app_id.desktop" "$appdir/$app_id.desktop"
ln -s "usr/share/icons/hicolor/scalable/apps/$app_id.svg" "$appdir/$app_id.svg"
ln -s "$app_id.svg" "$appdir/.DirIcon"
desktop-file-validate "$appdir/usr/share/applications/$app_id.desktop"

echo "==> Getting the AppImage runtime $runtime_tag and checking its signature"
mkdir -p "$work"
base="https://github.com/AppImage/type2-runtime/releases/download/$runtime_tag"
for file in runtime-x86_64 runtime-x86_64.sig signing-pubkey.asc; do
    curl -fsSLo "$work/$file" "$base/$file"
done
export GNUPGHOME="$work/gnupg"
rm -rf "$GNUPGHOME"
mkdir -m 0700 "$GNUPGHOME"
gpg --batch --quiet --import "$work/signing-pubkey.asc"
fingerprint="$(gpg --batch --with-colons --fingerprint | awk -F: '/^fpr:/ {print $10; exit}')"
if [ "$fingerprint" != "$runtime_fingerprint" ]; then
    echo "The runtime's signing key is not the expected one ($fingerprint): stopping." >&2
    exit 1
fi
gpg --batch --verify "$work/runtime-x86_64.sig" "$work/runtime-x86_64"

echo "==> Packing"
image="$out/Snow_Launcher-$version-x86_64.AppImage"
rm -f "$work/AppDir.squashfs" "$image"
mksquashfs "$appdir" "$work/AppDir.squashfs" -root-owned -noappend -comp zstd -quiet
cat "$work/runtime-x86_64" "$work/AppDir.squashfs" > "$image"
chmod 0755 "$image"
(cd "$out" && sha256sum "$(basename "$image")" > "$(basename "$image").sha256")
echo "==> $image ($(du -h "$image" | cut -f1))"
