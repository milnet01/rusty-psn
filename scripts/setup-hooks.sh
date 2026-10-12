#!/usr/bin/env bash
# Run once in every clone: ./scripts/setup-hooks.sh
#
# Git keeps these settings in .git/config, and no clone inherits them
# (`local-gate.md` § 2). Without them the hooks in .githooks/ never run,
# and every docs-only push runs the whole gate.
# The values are this skeleton's local-ci.sh contract, not defaults:
# a repository with another gate script sets its own.
set -Eeuo pipefail
cd "$(dirname "$0")/.."

git config core.hooksPath .githooks
git config ants.gate.command ./scripts/local-ci.sh
git config ants.gate.docsMode --docs
git config ants.gate.docsGlob "$(./scripts/local-ci.sh --docs-glob)"
