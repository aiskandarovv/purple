#!/bin/sh
# Regenerate embedded constants in site/worker.ts from site/install.sh,
# site/page.html and llms.txt. Thin wrapper around sync.mjs so this can be
# called from any shell context. Run after editing any of the three source
# files. Usage: sh scripts/site/sync.sh (from anywhere).
set -e
cd "$(dirname "$0")"
node sync.mjs
