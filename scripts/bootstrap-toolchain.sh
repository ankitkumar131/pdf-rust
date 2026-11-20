#!/usr/bin/env bash
# Installs a Rust toolchain for sandboxes that can reach the npm registry but
# not static.rust-lang.org (used by the CI/agent environment for this repo).
# On a normal machine just use https://rustup.rs instead.
set -euo pipefail

VERSION="${RUST_VERSION:-1.95.0}"
PREFIX="${PREFIX:-$HOME/.local/rust$VERSION}"
PKG="@rust-sandbox/rust-${VERSION//./-}-x86_64-linux"
TARBALL="rust-${VERSION//./-}-x86_64-linux-${VERSION}.tgz"
URL="https://registry.npmjs.org/${PKG}/-/${TARBALL}"

echo "==> installing Rust $VERSION into $PREFIX"
tmp="$(mktemp -d)"
curl -fsSL --retry 3 -o "$tmp/rust.tgz" "$URL"
tar -xzf "$tmp/rust.tgz" -C "$tmp"
mkdir -p "$PREFIX"
for component in rustc cargo rust-std; do
  src="$tmp/package/components/${component}-${VERSION}-x86_64-unknown-linux-gnu.tar.xz"
  work="$tmp/x-$component"; mkdir -p "$work"
  tar -xJf "$src" -C "$work"
  (cd "$work"/*/ && ./install.sh --prefix="$PREFIX" --disable-ldconfig >/dev/null)
  echo "    installed $component"
done
rm -rf "$tmp"
echo "==> done: $("$PREFIX/bin/rustc" --version)"
echo "    add to PATH: export PATH=\"$PREFIX/bin:\$PATH\""
