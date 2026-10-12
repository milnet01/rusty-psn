#!/usr/bin/env bash
# THE CI GATE, run by the pre-push hook before every push.
#
# The GitHub workflows (.github/workflows/) are the upstream author's and do
# not call this file. So this file MIRRORS them: the "GitHub mirror" steps run
# the workflows' own commands, word for word, for every leg that can run on
# Linux. When a workflow changes, change the matching step here in the same
# commit — a mirror is only correct while both are edited together.
#
# What GitHub runs that this cannot: main.yml's windows-latest and
# macos-latest legs (no Windows or Apple toolchain here, and `ring` needs a C
# cross-compiler). Those are named on every run below, so a pass here is never
# read as a pass on those platforms.
#
# Usage:
#   ./scripts/local-ci.sh           the full gate
#   ./scripts/local-ci.sh --docs    the checks that do not need a build
#   ./scripts/local-ci.sh --docs-glob   print what counts as documentation
#
# WHY --docs EXISTS, AND WHY IT IS NOT A SKIP. Rebuilding and re-testing to fix
# a prose typo is how a person learns to reach for --no-verify. So --docs omits
# only the legs that need a compiler or a full environment, and NOTHING ELSE:
# every check that can run without one runs in both modes. A mode cheaper than
# the pipeline is fine; a mode that checks LESS of what the pipeline checks is
# a green that lies. The pre-push hook selects it on a documentation-only push.
set -Eeuo pipefail
cd "$(dirname "$0")/.."

# The one definition of what counts as documentation. The pre-push hook reads
# it with `--docs-glob` and does its own matching, so the decision has one home
# as well as one value.
DOCS_GLOB='docs/*|*.md|LICENSE'

# Tool versions are pinned HERE and installed from here by the workflow. A
# version pinned only in the workflow is a second copy: this script would take
# whatever happened to be on PATH, so the same commit could pass locally and
# fail on GitHub. Measured case: ruff's default rule set changed between
# releases, so an unpinned run enforces a different set on each machine.
# RUFF_VERSION='0.16.8'
CARGO_AUDIT_VERSION='0.22.2'

if [[ ${1:-} == --docs-glob ]]; then printf '%s\n' "$DOCS_GLOB"; exit 0; fi

DOCS_ONLY=0
[[ ${1:-} == --docs ]] && DOCS_ONLY=1

# ── Have you filled anything in yet? ────────────────────────────────────────
# SET THIS TO 1 once the steps below run at least one real check. Until then
# this script reports honestly that it verified nothing.
#
# Why it exists: as shipped, every step below is a commented example, so the
# script ran nothing and printed "all checks passed". A gate that cannot fail
# is worse than no gate — it spends the attention that would have caught the
# defect, and the machine-wide pre-push hook reports its exit 0 as a pass.
# Measured 2026-09-26 on a fresh scaffold.
CONFIGURED=1

step() { printf '\n=== %s ===\n' "$1" >&2; }

# A check whose tool is absent SAYS SO and is never silently dropped: a check
# that did not run must not look like one that passed.
need() {
    command -v "$1" >/dev/null 2>&1 && return 0
    printf 'local-ci: %s is not installed — that check did NOT run\n' "$1" >&2
    return 1
}

# ── Checks that run in BOTH modes ───────────────────────────────────────────
# Anything not needing a compiler or a full environment belongs here.

step 'documentation'
# GitHub runs no documentation check. main.yml does rebuild on every push,
# documentation-only ones included, but a documentation change cannot change
# that build's result, so a documentation-only push mirrors GitHub with no step.
# SET THIS TO 1 once the documentation step runs a real check. CONFIGURED
# covers the script as a whole, so a project with lint and tests filled in
# still had --docs report a pass that checked nothing (Deedbox, 2026-09-28).
DOCS_CONFIGURED=0

if (( DOCS_ONLY )); then
    if (( CONFIGURED && DOCS_CONFIGURED )); then
        printf '\nlocal-ci: --docs passed; build and test legs skipped by design.\n' >&2
    elif (( CONFIGURED )); then
        printf '\nlocal-ci: --docs ran NO documentation check (none configured) — nothing was verified.\n' >&2
        printf 'local-ci: fill in the documentation step, then set DOCS_CONFIGURED=1.\n' >&2
    else
        printf '\nlocal-ci: NO CHECKS ARE CONFIGURED YET — this run verified nothing.\n' >&2
        printf 'local-ci: fill in the steps above, then set CONFIGURED=1.\n' >&2
    fi
    exit 0
fi

# ── Checks that need a build or a full environment ──────────────────────────

need cargo

# ── GitHub mirror ───────────────────────────────────────────────────────────

step 'format (rustfmt.yml: nightly rustfmt)'
# GitHub installs the newest nightly on each run; `rustup update nightly` keeps
# this one current.
if ! rustup run nightly rustfmt --version >/dev/null 2>&1; then
    printf 'local-ci: nightly rustfmt is not installed — that check did NOT run\n' >&2
    printf 'local-ci: install it: rustup toolchain install nightly --profile minimal --component rustfmt\n' >&2
    exit 1
fi
cargo +nightly fmt --check

for feature in cli egui; do
    step "lint (clippy.yml, feature $feature)"
    cargo clippy --no-default-features --features "$feature" -- -D warnings
done

for feature in cli egui; do
    step "build (main.yml ubuntu-22.04, feature $feature)"
    cargo build --release --no-default-features --features "$feature"
done

step 'dependency audit (audit.yml)'
need cargo-audit || exit 1
installed=$(cargo audit --version | awk '{print $2}')
if [[ $installed != "$CARGO_AUDIT_VERSION" ]]; then
    printf 'local-ci: cargo-audit %s is installed, the pin is %s — that check did NOT run\n' "$installed" "$CARGO_AUDIT_VERSION" >&2
    printf 'local-ci: install it: cargo install cargo-audit --locked --version %s\n' "$CARGO_AUDIT_VERSION" >&2
    exit 1
fi
cargo audit

printf '\nlocal-ci: NOT RUN here: main.yml windows-latest and macos-latest legs (cli and egui).\n' >&2

# ── Local additions — GitHub runs no tests ──────────────────────────────────

step 'lint (local: all targets, tests included)'
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --no-default-features --features cli -- -D warnings

step 'test (local: default build, then the cli build)'
cargo test --locked
cargo test --locked --no-default-features --features cli

if (( CONFIGURED )); then
    printf '\nlocal-ci: all checks passed.\n' >&2
else
    printf '\nlocal-ci: NO CHECKS ARE CONFIGURED YET — this run verified nothing.\n' >&2
    printf 'local-ci: fill in the steps above, then set CONFIGURED=1.\n' >&2
fi
