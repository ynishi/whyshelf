
# `check` is the definition of green. CI runs it and nothing else, so a gate
# added here is picked up there without a second edit.

set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Everything a change owes before it is handed over. The docs are built before
# the tests, which resolve the books' Rust paths in them.
check: rust-fmt-check rust-clippy rust-doc rust-test deny books

# The repository's own books are sound, every document is in the one form
# (DEC-52), and every approval in them still holds (DEC-26). The answer is
# printed only when it fails the recipe.
books:
    @out="$(cargo run -q -p whyshelf-cli -- --root . check)" || { printf '%s\n' "$out"; exit 1; }
    @out="$(cargo run -q -p whyshelf-cli -- --root . fmt --check)" || { printf '%s\n' "$out"; exit 1; }
    @out="$(cargo run -q -p whyshelf-cli -- --root . stale --exit-code)" || { printf '%s\n' "$out"; exit 1; }

rust-fmt-check:
    cargo fmt --all -- --check

rust-clippy:
    cargo clippy --workspace --all-targets -- -D warnings

rust-test:
    cargo test --workspace

# The documentation is the rustdoc, and it builds without a warning (a broken
# link counts). The `whyshelf` binary and the umbrella library are both named
# `whyshelf`, so cargo would write both into one directory; the command line's page
# is built apart, into a target directory of its own. The docs are removed
# first, so the space `rust-item` (`just books`) reads the docs of this tree
# only, never pages left by an earlier build of an item or a crate since gone.
rust-doc:
    rm -rf "${CARGO_TARGET_DIR:-target}/doc"
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --exclude whyshelf-cli --no-deps
    RUSTDOCFLAGS="-D warnings" cargo doc -p whyshelf-cli --no-deps --target-dir target/doc-cli

deny:
    cargo deny check

# Its build directory is its own (`target` inside it), seeded from this
# checkout's when cargo-shared-target is installed, so that its first build
# reuses what was built here and shares the storage where the filesystem allows.
# One worktree per issue, cut from a freshly fetched origin/main.
[positional-arguments]
worktree-new type slug:
    #!/usr/bin/env bash
    set -euo pipefail
    type="$1" slug="$2"
    case "$slug" in ''|*/*|.*) echo "worktree-new: '$slug' cannot name a worktree (no '/', no leading '.')" >&2; exit 1;; esac
    git fetch origin
    git worktree add ".worktrees/$slug" -b "$type/$slug" origin/main
    (cd ".worktrees/$slug" && just branch-check)
    if [ -n "${CARGO_TARGET_DIR:-}" ]; then
        echo "worktree-new: CARGO_TARGET_DIR is set, so the worktree builds there; nothing seeded"
    elif [ ! -d target ]; then
        echo "worktree-new: no target directory here to seed from; the first build in the worktree starts from nothing"
    elif cargo shared-target --version >/dev/null 2>&1; then
        cargo shared-target --src target --dest ".worktrees/$slug/target"
    else
        echo "worktree-new: cargo-shared-target is not installed, so nothing is seeded; the first build in the worktree starts from nothing"
    fi

# Refuses a branch origin/main does not hold, a worktree with changes not
# committed, and a run from inside the worktree.
# Removes a merged branch's worktree, its build directory with it, and the branch.
[positional-arguments]
worktree-remove slug from=invocation_directory():
    #!/usr/bin/env bash
    set -euo pipefail
    slug="$1"
    case "$slug" in ''|*/*|.*) echo "worktree-remove: '$slug' is not the name of a worktree" >&2; exit 1;; esac
    dir=".worktrees/$slug"
    [ -d "$dir" ] || { echo "worktree-remove: no $dir in $(pwd); run it from the main checkout" >&2; exit 1; }
    case "$2/" in "$(cd "$dir" && pwd -P)"/*)
        echo "worktree-remove: run it from the main checkout, not from inside $dir" >&2; exit 1;;
    esac
    [ -z "$(git -C "$dir" status --porcelain)" ] || { echo "worktree-remove: $dir has changes not committed" >&2; exit 1; }
    branch="$(git -C "$dir" symbolic-ref --short -q HEAD)" || { echo "worktree-remove: $dir is on no branch" >&2; exit 1; }
    git fetch origin
    git merge-base --is-ancestor "$(git -C "$dir" rev-parse HEAD)" origin/main || { echo "worktree-remove: origin/main does not hold $branch" >&2; exit 1; }
    git worktree remove "$dir"
    # An editor's build can make the directory again while it is removed.
    if [ -e "$dir" ]; then rm -rf "$dir"; fi
    git branch -d "$branch"

# Storage a seeded directory clones from another is counted in each, a file it
# shares by a hard link once, so the sum is an upper bound.
# The disk each build directory takes: this checkout's and every worktree's.
worktree-disk:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    dirs=()
    [ ! -d target ] || dirs+=(target)
    [ -z "${CARGO_TARGET_DIR:-}" ] || [ ! -d "$CARGO_TARGET_DIR" ] || dirs+=("$CARGO_TARGET_DIR")
    dirs+=(.worktrees/*/target)
    if [ "${#dirs[@]}" -eq 0 ]; then echo "worktree-disk: no build directory"; else du -sh "${dirs[@]}"; fi

# Refuses main, and a branch that does not contain origin/main.
branch-check:
    #!/usr/bin/env bash
    set -euo pipefail
    branch="$(git rev-parse --abbrev-ref HEAD)"
    if [ "$branch" = "main" ]; then
        echo "branch-check: on main; cut a branch with 'just worktree-new'" >&2
        exit 1
    fi
    if git rev-parse --verify --quiet origin/main >/dev/null; then
        git merge-base --is-ancestor origin/main HEAD || {
            echo "branch-check: $branch does not contain origin/main; rebase or recut" >&2
            exit 1
        }
    fi
    echo "branch-check: $branch ok"

# Commit messages on this branch carry no AI attribution (CONTRIBUTING.md).
commit-msg-check:
    #!/usr/bin/env bash
    set -euo pipefail
    range="origin/main..HEAD"
    git rev-parse --verify --quiet origin/main >/dev/null || range="HEAD"
    if git log --format=%B "$range" | grep -inE '^(co-authored-by|signed-off-by):|generated with'; then
        echo "commit-msg-check: attribution lines found in $range" >&2
        exit 1
    fi
    echo "commit-msg-check: $range ok"

# The gate before a hand-over: run it after the last commit, over the tree
# that is actually handed over.
pre-push: branch-check commit-msg-check check

