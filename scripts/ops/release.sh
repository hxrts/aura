#!/usr/bin/env bash
# Publish workspace crates to crates.io using cargo publish --workspace.
# Crates marked publish = false in Cargo.toml are automatically excluded.
# Re-running after a partial failure is safe — crates.io rejects duplicates.
set -euo pipefail

# ── Setup ──────────────────────────────────────────────────────────────
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

# ── Defaults ──────────────────────────────────────────────────────────
DRY_RUN=0
SKIP_CI=0
CREATE_TAG=1
PUSH=0
ALLOW_DIRTY=0
REQUIRE_MAIN=1
SKIP_NIX=0
SKIP_PUBLISH=0
VERSION=""
CURRENT_VERSION=""
TAG_PREFIX="v"
TAG_NAME=""

# ── Helpers ───────────────────────────────────────────────────────────
usage() {
  cat <<'EOF'
Usage:
  ./scripts/ops/release.sh [options]

Options:
  --version <version>   Release version (defaults to workspace version)
  --dry-run             Run all publishing steps with --dry-run
  --skip-ci             Skip just ci-dry-run preflight checks
  --skip-nix            Skip nix build and flake check
  --skip-publish        Skip cargo publish / cargo publish --dry-run
  --no-tag              Skip git tag creation
  --push                Push current branch and tag after successful publish
  --allow-dirty         Allow a dirty git working tree
  --no-require-main     Allow releasing from non-main branches
  -h, --help            Show this help text

Re-running after a partial failure is safe. crates.io rejects duplicate
versions, and cargo publish --workspace skips already-published crates.
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

require_command() {
  local cmd="$1"
  command -v "${cmd}" >/dev/null 2>&1 || die "${cmd} is required"
}

# Extract version from the workspace [workspace.package] section
extract_workspace_version() {
  awk '
    /^\[workspace\.package\]/ { in_section = 1; next }
    /^\[/ { in_section = 0 }
    in_section && $1 == "version" {
      gsub(/ /, "", $0)
      sub(/^version="/, "", $0)
      sub(/"$/, "", $0)
      print $0
      exit
    }
  ' "${ROOT_DIR}/Cargo.toml"
}

workspace_manifest_files() {
  git ls-files -- 'Cargo.toml' 'crates/*/Cargo.toml' 'examples/*/Cargo.toml'
}

update_manifest_versions() {
  local old_version="$1"
  local new_version="$2"
  local manifest

  echo "== bumping workspace version ${old_version} -> ${new_version} =="

  perl -0pi -e 's/(\[workspace\.package\][^\[]*?\bversion\s*=\s*")\Q'"${old_version}"'\E(")/${1}'"${new_version}"'${2}/s' \
    "${ROOT_DIR}/Cargo.toml"

  while IFS= read -r manifest; do
    perl -0pi -e 's/(\[package\][^\[]*?\bversion\s*=\s*")\Q'"${old_version}"'\E(")/${1}'"${new_version}"'${2}/s' \
      "${manifest}"
    perl -0pi -e 's/(version\s*=\s*"=)\Q'"${old_version}"'\E(")/${1}'"${new_version}"'${2}/g' \
      "${manifest}"
  done < <(workspace_manifest_files)
}

refresh_lockfile() {
  echo "== refreshing Cargo.lock =="
  cargo generate-lockfile
}

create_release_commit() {
  if git diff --quiet && git diff --cached --quiet; then
    echo "== no release commit needed =="
    return
  fi

  git add Cargo.toml crates/*/Cargo.toml examples/*/Cargo.toml
  if [[ -f Cargo.lock ]]; then
    git add -f Cargo.lock
  fi
  git commit -m "Release v${VERSION}"
  echo "== created release commit for v${VERSION} =="
}

# ── Validation ─────────────────────────────────────────────────────────

assert_version_format() {
  local version="$1"
  if [[ ! "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
    die "invalid release version '${version}'"
  fi
}

assert_clean_tree() {
  if [[ "${ALLOW_DIRTY}" -eq 1 ]]; then
    return
  fi
  if ! git diff --quiet || ! git diff --cached --quiet; then
    git status --short
    die "working tree is not clean. Use --allow-dirty if intentional"
  fi
}

assert_branch() {
  local branch
  branch="$(git rev-parse --abbrev-ref HEAD)"
  if [[ "${branch}" == "HEAD" ]]; then
    die "refusing to release from detached HEAD"
  fi
  if [[ "${REQUIRE_MAIN}" -eq 1 && "${branch}" != "main" ]]; then
    die "releases must be run from main unless --no-require-main is passed"
  fi
}

# ── Preflight ──────────────────────────────────────────────────────────

run_ci_dry_run() {
  echo "== running preflight: just ci-dry-run =="
  just ci-dry-run
}

run_nix_checks() {
  echo "== running nix build =="
  nix build
  echo "== running nix flake check =="
  nix flake check
}

# ── Publish ────────────────────────────────────────────────────────────

publish_workspace() {
  local cmd=(cargo publish --workspace --locked)
  if [[ "${DRY_RUN}" -eq 1 ]]; then
    cmd+=(--dry-run)
  fi
  if [[ "${ALLOW_DIRTY}" -eq 1 ]]; then
    cmd+=(--allow-dirty)
  fi
  echo "== ${cmd[*]} =="
  "${cmd[@]}"
}

# ── Tagging & Push ─────────────────────────────────────────────────────

create_release_tag() {
  if [[ "${CREATE_TAG}" -eq 0 ]]; then
    return
  fi
  TAG_NAME="${TAG_PREFIX}${VERSION}"
  if git rev-parse "${TAG_NAME}" >/dev/null 2>&1; then
    local existing_commit current_commit
    existing_commit="$(git rev-parse "${TAG_NAME}^{}")"
    current_commit="$(git rev-parse HEAD)"
    if [[ "${existing_commit}" == "${current_commit}" ]]; then
      echo "== tag ${TAG_NAME} already exists and points to HEAD; reusing =="
      return
    fi
    die "tag ${TAG_NAME} already exists at ${existing_commit}, expected ${current_commit}"
  fi
  git tag -a "${TAG_NAME}" -m "Release ${TAG_NAME}"
  echo "== created git tag ${TAG_NAME} =="
}

push_git_refs() {
  if [[ "${PUSH}" -eq 0 ]]; then
    return
  fi
  local branch
  branch="$(git rev-parse --abbrev-ref HEAD)"
  echo "== pushing branch ${branch} =="
  git push origin "${branch}"
  if [[ -n "${TAG_NAME}" ]]; then
    echo "== pushing tag ${TAG_NAME} =="
    git push origin "${TAG_NAME}"
  fi
}

# ── Main ───────────────────────────────────────────────────────────────
main() {
  require_command cargo
  require_command git

  while [[ "$#" -gt 0 ]]; do
    case "$1" in
      --version)
        if [[ "$#" -lt 2 ]]; then
          die "--version requires a value"
        fi
        VERSION="$2"
        shift 2
        ;;
      --version=*)
        VERSION="${1#*=}"
        shift
        ;;
      --dry-run)
        DRY_RUN=1
        shift
        ;;
      --skip-ci)
        SKIP_CI=1
        shift
        ;;
      --skip-nix)
        SKIP_NIX=1
        shift
        ;;
      --skip-publish)
        SKIP_PUBLISH=1
        shift
        ;;
      --no-tag)
        CREATE_TAG=0
        shift
        ;;
      --push)
        PUSH=1
        shift
        ;;
      --allow-dirty)
        ALLOW_DIRTY=1
        shift
        ;;
      --no-require-main)
        REQUIRE_MAIN=0
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown argument: $1"
        ;;
    esac
  done

  # Resolve version
  if [[ -z "${VERSION}" ]]; then
    VERSION="$(extract_workspace_version)"
  fi
  if [[ -z "${VERSION}" ]]; then
    die "unable to determine workspace version"
  fi
  CURRENT_VERSION="$(extract_workspace_version)"
  if [[ -z "${CURRENT_VERSION}" ]]; then
    die "unable to determine current workspace version"
  fi
  assert_version_format "${VERSION}"

  echo "== release ${VERSION} =="
  echo "   dry_run=${DRY_RUN} skip_ci=${SKIP_CI} skip_nix=${SKIP_NIX} skip_publish=${SKIP_PUBLISH}"
  echo "   tag=${CREATE_TAG} push=${PUSH} allow_dirty=${ALLOW_DIRTY}"
  echo ""

  # Pre-publish validation
  assert_branch
  assert_clean_tree

  # Registry token check (skip for dry-run or explicit tag-only releases)
  if [[ "${SKIP_PUBLISH}" -eq 0 && "${DRY_RUN}" -eq 0 && "${CARGO_REGISTRY_TOKEN:-}" == "" ]]; then
    die "CARGO_REGISTRY_TOKEN is not set; publishing will fail"
  fi

  if [[ "${VERSION}" != "${CURRENT_VERSION}" ]]; then
    if [[ "${DRY_RUN}" -eq 1 ]]; then
      die "--dry-run does not support version bumps; bump to ${VERSION} first or run a real release"
    fi
    update_manifest_versions "${CURRENT_VERSION}" "${VERSION}"
    refresh_lockfile
  fi

  # Preflight checks
  if [[ "${SKIP_CI}" -eq 0 ]]; then
    require_command just
    run_ci_dry_run
  else
    echo "== skipping CI preflight checks =="
  fi

  if [[ "${SKIP_NIX}" -eq 0 ]]; then
    require_command nix
    run_nix_checks
  else
    echo "== skipping nix checks =="
  fi

  create_release_commit

  # Publish workspace (cargo handles dependency ordering and skips publish=false crates)
  if [[ "${SKIP_PUBLISH}" -eq 0 ]]; then
    publish_workspace
  else
    echo "== skipping cargo publish =="
  fi

  # Tag and push
  create_release_tag
  push_git_refs

  echo ""
  echo "== release ${VERSION} completed =="
}

main "$@"
