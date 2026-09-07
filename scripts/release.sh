#!/bin/sh
# Cut a release: bump the version in Cargo.toml first, then run this script.
# It generates the changelog section from commit history, commits, tags, and
# pushes. Pushing the "v<version>" tag triggers .github/workflows/release.yml
# (binaries + GitHub Release); publishing that release triggers
# .github/workflows/container.yml (container re-tag).
set -eu

# Work from the repository root regardless of the invocation directory.
cd "$(dirname "$0")/.."

[ "$(git branch --show-current)" = "main" ] || {
  echo "refusing to release from branch '$(git branch --show-current)' (expected 'main')" >&2
  exit 1
}

VERSION="$(awk -F'"' '/^version = /{print $2}' Cargo.toml)"
TAG="v$VERSION"

# Generate this release's section from commit history. Skip if git-cliff is
# not installed; the grep below still forces a matching section to exist.
if command -v git-cliff >/dev/null 2>&1; then
  # Don't duplicate the section if a previous run already prepended it but
  # failed before committing (e.g. pre-commit aborted the commit).
  if ! grep -q "^## \[$VERSION\]" CHANGELOG.md; then
    git cliff --unreleased --tag "$TAG" --prepend CHANGELOG.md
  fi
else
  echo "warning: git-cliff not found; add a '## [$VERSION]' section to CHANGELOG.md manually" >&2
fi

# The release workflow extracts a "## [<version>]" section from CHANGELOG.md
# for the release notes; refuse to release if it can't be found.
grep -q "^## \[$VERSION\]" CHANGELOG.md || {
  echo "CHANGELOG.md has no '## [$VERSION]' section" >&2
  exit 1
}

# Cargo.lock records the package version, so the bump leaves it stale. Sync it
# before staging; otherwise the pre-commit clippy hook rewrites it mid-commit
# and pre-commit aborts with "files were modified by this hook".
cargo check
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: Release $VERSION"
git tag "$TAG"
git push origin main "$TAG"
