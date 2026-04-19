#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# version.sh — single source of truth for workspace version management.
#
# Usage:
#   ./scripts/version.sh show          Print current version
#   ./scripts/version.sh patch         Bump patch  (0.1.0 → 0.1.1)
#   ./scripts/version.sh minor         Bump minor  (0.1.0 → 0.2.0)
#   ./scripts/version.sh major         Bump major  (0.1.0 → 1.0.0)
#   ./scripts/version.sh sync          Re-sync all Cargo.toml to VERSION file
#   ./scripts/version.sh tag           Create annotated git tag from VERSION
#   ./scripts/version.sh release       Bump patch, commit, tag, push
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION_FILE="$ROOT/VERSION"

current_version() {
  tr -d '[:space:]' < "$VERSION_FILE"
}

# Replace version = "OLD" with version = "NEW" in a Cargo.toml,
# matching only the first occurrence (the [package] version line).
update_cargo_toml() {
  local file="$1" old="$2" new="$3"
  sed -i "s/^version = \"$old\"/version = \"$new\"/" "$file"
}

sync_all() {
  local old="$1" new="$2"
  update_cargo_toml "$ROOT/Cargo.toml" "$old" "$new"
  for f in "$ROOT"/crates/*/Cargo.toml; do
    update_cargo_toml "$f" "$old" "$new"
  done
}

cmd_show() {
  current_version
}

cmd_bump() {
  local level="$1"
  local old new
  old="$(current_version)"
  IFS='.' read -r MAJOR MINOR PATCH <<< "$old"

  case "$level" in
    patch) PATCH=$((PATCH + 1)) ;;
    minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
    major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
    *) echo "Unknown level: $level" >&2; exit 1 ;;
  esac

  new="$MAJOR.$MINOR.$PATCH"
  echo "$new" > "$VERSION_FILE"
  sync_all "$old" "$new"
  echo "$old → $new"
}

cmd_sync() {
  local ver
  ver="$(current_version)"
  # Force-set version in all Cargo.toml files regardless of current value.
  sed -i "s/^version = \"[^\"]*\"/version = \"$ver\"/" "$ROOT/Cargo.toml"
  for f in "$ROOT"/crates/*/Cargo.toml; do
    sed -i "s/^version = \"[^\"]*\"/version = \"$ver\"/" "$f"
  done
  echo "All crates synced to $ver"
}

cmd_tag() {
  local ver
  ver="$(current_version)"
  git tag -a "v$ver" -m "Release v$ver"
  echo "Tagged v$ver"
}

cmd_release() {
  cmd_bump patch
  local ver
  ver="$(current_version)"
  git add "$VERSION_FILE" "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$ROOT"/crates/*/Cargo.toml
  git commit -m "chore: bump version to $ver"
  git tag "v$ver"
  git push && git push origin "v$ver"
  echo "Released v$ver"
}

case "${1:-show}" in
  show)    cmd_show ;;
  patch)   cmd_bump patch ;;
  minor)   cmd_bump minor ;;
  major)   cmd_bump major ;;
  sync)    cmd_sync ;;
  tag)     cmd_tag ;;
  release) cmd_release ;;
  *)
    echo "Usage: $0 {show|patch|minor|major|sync|tag|release}" >&2
    exit 1
    ;;
esac
