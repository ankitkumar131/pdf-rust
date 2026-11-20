#!/usr/bin/env bash
# Tags the current commit and publishes a GitHub release with the built assets.
#
#   scripts/release.sh 0.1.0 [--draft]
#
# Expects dist/ to be populated by scripts/build-release.sh and
# docs/release-<version>.md to exist (it becomes the release body).
set -euo pipefail

cd "$(dirname "$0")/.."
version=${1:?usage: scripts/release.sh <version> [--draft]}
draft=${2:-}
tag="v$version"
notes="docs/release-$version.md"
repo="ankitkumar131/pdf-rust"

[ -f "$notes" ] || { echo "error: $notes is missing" >&2; exit 1; }
[ -f "dist/pdf-rust_${version}_amd64.deb" ] || {
  echo "error: dist/ has no .deb; run scripts/build-release.sh $version first" >&2
  exit 1
}

# Refresh the checksums so they always match what is uploaded.
(cd dist && sha256sum ./*.deb ./*.tar.gz ./*.AppImage 2>/dev/null > SHA256SUMS.txt || true)

# Publish the binaries inside the tagged tree as well: release-asset uploads go
# to uploads.github.com, which some build environments cannot reach.
downloads="downloads/v$version"
mkdir -p "$downloads"
for file in "pdf-rust_${version}_amd64.deb" "pdf-rust-${version}-linux-x86_64.tar.gz"; do
  [ -f "dist/$file" ] || { echo "error: dist/$file is missing" >&2; exit 1; }
  cp -f "dist/$file" "$downloads/$file"
done
(cd "$downloads" && sha256sum ./*.deb ./*.tar.gz > SHA256SUMS.txt)
git add -f "$downloads"
if ! git diff --cached --quiet; then
  git commit -q -m "chore(release): publish $version binaries under $downloads"
fi

assets=(
  "dist/pdf-rust_${version}_amd64.deb"
  "dist/pdf-rust-${version}-linux-x86_64.tar.gz"
  "dist/pdf-rust-${version}-src.tar.gz"
  "dist/SHA256SUMS.txt"
)
# The AppImage is optional: it needs appimagetool, which is not always available.
if compgen -G "dist/pdf-rust-*.AppImage" >/dev/null; then
  assets+=(dist/pdf-rust-*.AppImage)
fi

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "tag $tag already exists locally"
else
  git tag -a "$tag" -m "pdf-rust $version"
fi

if git ls-remote --tags origin "refs/tags/$tag" | grep -q "$tag"; then
  echo "tag $tag already on origin"
else
  git push origin "$tag"
fi

if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  gh release edit "$tag" --repo "$repo" --notes-file "$notes"
else
  gh release create "$tag" --repo "$repo" --title "pdf-rust $version" \
    --notes-file "$notes" ${draft:+--draft}
fi

# Attaching assets needs uploads.github.com; keep going if it is unreachable,
# the notes link to $downloads inside the tag instead.
if ! gh release upload "$tag" "${assets[@]}" --repo "$repo" --clobber; then
  echo "warning: could not upload release assets (uploads.github.com unreachable)." >&2
  echo "         The binaries are published under $downloads in the tag." >&2
fi
gh release view "$tag" --repo "$repo" --json url,assets \
  -q '.url, (.assets[] | "  \(.name)  \(.size) bytes  \(.url)")'
