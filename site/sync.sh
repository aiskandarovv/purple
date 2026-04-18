#!/bin/sh
# Regenerate embedded constants in worker.ts from install.sh, page.html and
# ../llms.txt. Thin wrapper around sync.mjs so this can be called from any
# shell context. Run after editing any of the three source files.
set -e
cd "$(dirname "$0")"
node sync.mjs
