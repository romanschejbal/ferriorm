#!/usr/bin/env bash
#
# Release + publish ferriorm to crates.io.
#
# Usage:
#   scripts/publish.sh <version>              # real release
#   scripts/publish.sh --dry-run <version>    # build + verify, no writes
#   scripts/publish.sh --no-push <version>    # commit + publish, don't git push
#
# What it does (in order):
#   1. Verifies the working tree is clean and on `main`.
#   2. Runs fmt-check, clippy, and the full test suite.
#   3. Bumps the version in workspace Cargo.toml (workspace.package and the
#      five internal path+version dependency entries).
#   4. Refreshes Cargo.lock.
#   5. Reruns tests after the bump.
#   6. Commits as `v<version>: release` and tags `v<version>`.
#   7. Publishes the six crates to crates.io in dependency order, with a
#      propagation sleep between each so dependents see the new version.
#   8. Pushes the commit and the tag to `origin/main`.
#
# Requirements: you must already be logged into crates.io (`cargo login`).

set -euo pipefail

# ─── Argument parsing ─────────────────────────────────────────────
DRY_RUN=0
NO_PUSH=0
NEW_VERSION=""

usage() {
    sed -n '2,20p' "$0"
    exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=1 ;;
        --no-push) NO_PUSH=1 ;;
        -h|--help) usage 0 ;;
        -*) echo "unknown flag: $1" >&2; usage 2 ;;
        *)
            if [[ -z "$NEW_VERSION" ]]; then
                NEW_VERSION="$1"
            else
                echo "unexpected positional arg: $1" >&2; usage 2
            fi
            ;;
    esac
    shift
done

if [[ -z "$NEW_VERSION" ]]; then
    echo "error: <version> is required" >&2
    usage 2
fi

if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$ ]]; then
    echo "error: '$NEW_VERSION' is not a valid semver version" >&2
    exit 2
fi

# Operate from the repo root so relative paths are stable regardless of cwd.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# In dry-run mode, always restore the bumped files on exit so the working
# tree stays clean even if something errors out mid-flight.
cleanup_dry_run() {
    if [[ "$DRY_RUN" -eq 1 ]]; then
        git checkout -- Cargo.toml Cargo.lock 2>/dev/null || true
    fi
}
trap cleanup_dry_run EXIT

# ─── Helpers ──────────────────────────────────────────────────────
say() { printf "\n\033[1;34m==\033[0m %s\n" "$*"; }

run_or_say() {
    if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "[dry-run] skipping: $*"
    else
        "$@"
    fi
}

# Dependency order for publishing. Must match the onion: each crate's
# internal deps must already be on crates.io when we publish it.
CRATES=(
    ferriorm-core
    ferriorm-parser
    ferriorm-runtime
    ferriorm-codegen
    ferriorm-migrate
    ferriorm-cli
)

PROPAGATION_SLEEP_SECS="${PROPAGATION_SLEEP_SECS:-20}"

# ─── 1. Preflight ─────────────────────────────────────────────────
say "preflight checks"

if [[ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]]; then
    echo "error: must be on 'main' branch, currently on $(git rev-parse --abbrev-ref HEAD)" >&2
    exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree is not clean. commit or stash first:" >&2
    git status --short >&2
    exit 1
fi

# Read current workspace version from the [workspace.package] table.
CUR_VERSION="$(
    python3 - <<'PY'
import re, pathlib, sys
text = pathlib.Path("Cargo.toml").read_text()
m = re.search(
    r'\[workspace\.package\][^\[]*?version = "([^"]+)"',
    text,
    re.DOTALL,
)
if not m:
    sys.exit(1)
print(m.group(1))
PY
)"

if [[ -z "$CUR_VERSION" ]]; then
    echo "error: could not read current workspace version from Cargo.toml" >&2
    exit 1
fi

if [[ "$CUR_VERSION" == "$NEW_VERSION" ]]; then
    echo "error: Cargo.toml is already at $NEW_VERSION; nothing to bump" >&2
    exit 1
fi

echo "current version: $CUR_VERSION"
echo "new version    : $NEW_VERSION"
echo "dry-run        : $DRY_RUN"
echo "push           : $([[ $NO_PUSH -eq 1 ]] && echo no || echo yes)"

# ─── 2. Lint + test before bumping ───────────────────────────────
say "cargo fmt --check"
cargo fmt --all -- --check

say "cargo clippy (pedantic, -D warnings)"
cargo clippy --workspace --all-targets -- -D warnings

say "cargo test --workspace"
cargo test --workspace --quiet

# ─── 3. Bump version in Cargo.toml ───────────────────────────────
say "bumping version in workspace Cargo.toml"

# Use a pinned `workspace.package.version` match so we don't touch the
# example's `version = "0.1.0"` line or anything else.
#
# Two edits:
#  (a) [workspace.package] version = "<cur>"  → "<new>"
#  (b) workspace.dependencies internal crates version = "<cur>"  → "<new>"
#      (there are five of them, matching the ferriorm-* crates)
python3 - <<PY
import re, sys, pathlib
path = pathlib.Path("Cargo.toml")
text = path.read_text()

cur = "${CUR_VERSION}"
new = "${NEW_VERSION}"

# (a) [workspace.package] version — match only under that section.
pattern_pkg = re.compile(
    r'(\[workspace\.package\][^\[]*?version = ")' + re.escape(cur) + r'(")',
    re.DOTALL,
)
text, n_pkg = pattern_pkg.subn(r'\g<1>' + new + r'\g<2>', text, count=1)
if n_pkg != 1:
    sys.exit("failed to bump workspace.package.version")

# (b) internal path+version deps: ferriorm-<name> = { path = "...", version = "<cur>" }
pattern_dep = re.compile(
    r'(ferriorm-[a-z]+ = \{ path = "crates/ferriorm-[a-z]+", version = ")'
    + re.escape(cur)
    + r'(")'
)
text, n_dep = pattern_dep.subn(r'\g<1>' + new + r'\g<2>', text)
if n_dep != 5:
    sys.exit(f"expected 5 internal dep version bumps, got {n_dep}")

path.write_text(text)
print(f"bumped workspace.package.version and {n_dep} internal deps")
PY

# ─── 4. Refresh lockfile + re-run tests ─────────────────────────
say "cargo update -w"
cargo update -w

say "post-bump: cargo test --workspace"
cargo test --workspace --quiet

# ─── 5. Commit + tag ────────────────────────────────────────────
say "git commit + tag v${NEW_VERSION}"

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] would: git add Cargo.toml Cargo.lock"
    echo "[dry-run] would: git commit -m 'v${NEW_VERSION}: release'"
    echo "[dry-run] would: git tag v${NEW_VERSION}"
else
    git add Cargo.toml Cargo.lock
    git commit -m "v${NEW_VERSION}: release"
    git tag "v${NEW_VERSION}"
fi

# ─── 6. Publish crates in dep order ─────────────────────────────
#
# Note: we deliberately don't run `cargo publish --dry-run` per-crate here.
# Its verify step builds each packaged tarball against crates.io, which
# chicken-and-eggs for bumped sibling crates: parser@NEW needs core@NEW
# on the registry, and by definition in a dry run it isn't there yet.
# Instead we trust the bumped workspace tests (step 4) and only upload
# for real when `$DRY_RUN` is 0.
say "publishing crates to crates.io in dep order"

for i in "${!CRATES[@]}"; do
    crate="${CRATES[$i]}"
    say "[$((i+1))/${#CRATES[@]}] $crate"
    if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "[dry-run] would: (cd crates/$crate && cargo publish)"
    else
        (cd "crates/$crate" && cargo publish)
        if [[ $((i+1)) -lt ${#CRATES[@]} ]]; then
            echo "(sleeping ${PROPAGATION_SLEEP_SECS}s for crates.io index to catch up)"
            sleep "$PROPAGATION_SLEEP_SECS"
        fi
    fi
done

# ─── 7. Push commit + tag ───────────────────────────────────────
if [[ "$NO_PUSH" -eq 1 ]]; then
    say "skipping push (--no-push)"
    echo "don't forget to: git push origin main && git push origin v${NEW_VERSION}"
elif [[ "$DRY_RUN" -eq 1 ]]; then
    say "skipping push (dry-run)"
else
    say "git push origin main + tag"
    git push origin main
    git push origin "v${NEW_VERSION}"
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
    say "dry-run complete (Cargo.toml + Cargo.lock will be reverted on exit)"
fi

say "done: v${NEW_VERSION}"
