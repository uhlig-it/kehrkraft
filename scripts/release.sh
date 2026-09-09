#!/bin/sh
# Cut a release: bump the version in Cargo.toml first, then run this script.
# It generates the changelog section from commit history, commits, tags, and
# pushes. Pushing the "v<version>" tag triggers .github/workflows/release.yml
# (binaries + GitHub Release); publishing that release triggers
# .github/workflows/container.yml (container re-tag).
#
# Every step skips what a previous run already did, so rerunning the script
# after a failure (pre-commit aborted the commit, the push timed out, ...)
# resumes where the last run stopped instead of duplicating sections, commits,
# or tags.
set -eu

# Work from the repository root regardless of the invocation directory.
cd "$(dirname "$0")/.."

[ "$(git branch --show-current)" = "main" ] || {
  echo "refusing to release from branch '$(git branch --show-current)' (expected 'main')" >&2
  exit 1
}

VERSION="$(awk -F'"' '/^version = /{print $2}' Cargo.toml)"
[ -n "$VERSION" ] || {
  echo "no version found in Cargo.toml" >&2
  exit 1
}
TAG="v$VERSION"
RELEASE_SUBJECT="chore: Release $VERSION"

# --- Preflight ---------------------------------------------------------------
# Nothing may be staged, and only the files of the release commit may carry
# local modifications: the version bump in Cargo.toml (and the Cargo.lock it
# goes stale) are committed by this script, everything else must be committed
# first.
git diff --cached --quiet || {
  echo "staged changes present; commit or unstage them first" >&2
  exit 1
}
DIRTY="$(git diff --name-only | grep -vE '^(Cargo\.toml|Cargo\.lock|CHANGELOG\.md)$' || true)"
[ -z "$DIRTY" ] || {
  echo "unexpected local modifications: $DIRTY" >&2
  echo "(only Cargo.toml, Cargo.lock and CHANGELOG.md may be modified)" >&2
  exit 1
}

# Releasing from a main that diverged from origin/main has left release tags
# on abandoned history before; refuse unless HEAD is exactly origin/main.
git fetch origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || {
  echo "main is not up to date with origin/main; push or pull first" >&2
  exit 1
}

# An existing tag means a previous run already got as far as tagging (only
# the push may be missing) - unless it points elsewhere, in which case this
# version was already released and rerunning must not move the tag.
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  [ "$(git rev-parse "refs/tags/$TAG")" = "$(git rev-parse HEAD)" ] || {
    echo "tag $TAG already exists but does not point at HEAD; bump the version or investigate" >&2
    exit 1
  }
fi

# --- Changelog section -------------------------------------------------------
# Generate the section from commit history unless a previous run already
# prepended it. Skip if git-cliff is not installed; the grep below still
# forces a matching section to exist.
if ! grep -q "^## \[$VERSION\]" CHANGELOG.md && command -v git-cliff >/dev/null 2>&1; then
  # git-cliff's --unreleased range starts at the most recent reachable tag,
  # which must be the previous release: CHANGELOG.md's newest section is that
  # release (sections are prepended, never rewritten). A mismatch means the
  # release history is inconsistent (e.g. a tag on a branch that was never
  # merged into main); the generated section would silently re-summarize
  # commits that are already released.
  LAST_TAG="$(git describe --tags --abbrev=0 2>/dev/null || true)"
  TOP_SECTION="$(sed -n 's/^## \[\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)\].*/\1/p' CHANGELOG.md | head -1)"
  [ "$LAST_TAG" = "v$TOP_SECTION" ] || {
    echo "inconsistent release state: latest reachable tag is '${LAST_TAG:-<none>}' but CHANGELOG.md's newest section is '$TOP_SECTION'" >&2
    exit 1
  }

  git cliff --unreleased --tag "$TAG" --prepend CHANGELOG.md
fi

# The release workflow extracts a "## [<version>]" section from CHANGELOG.md
# for the release notes; refuse to release if it can't be found.
grep -q "^## \[$VERSION\]" CHANGELOG.md || {
  echo "CHANGELOG.md has no '## [$VERSION]' section; install git-cliff or add it manually" >&2
  exit 1
}

# --- Commit, tag, push -------------------------------------------------------
# Commit only when the previous run did not already get that far.
if [ "$(git log -1 --format=%s)" != "$RELEASE_SUBJECT" ]; then
  # Cargo.lock records the package version, so the bump leaves it stale. Sync
  # it before staging; otherwise the pre-commit clippy hook rewrites it
  # mid-commit and pre-commit aborts with "files were modified by this hook".
  cargo check
  git add Cargo.toml Cargo.lock CHANGELOG.md
  git commit -m "$RELEASE_SUBJECT"
fi

# Create the tag only when it does not exist yet (an existing tag was checked
# above to point at HEAD).
if ! git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  git tag "$TAG"
fi

git push origin main "$TAG"
