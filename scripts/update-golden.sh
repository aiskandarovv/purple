#!/usr/bin/env bash
# Regenerate visual regression golden files after intentional UI changes.
# Usage: ./scripts/update-golden.sh
set -e
echo "Regenerating visual golden files..."
UPDATE_GOLDEN=1 cargo test --bin purple visual_regression -- --test-threads=1
echo "Done. Review changes with: git diff tests/visual_golden/"
echo "Stage with: git add tests/visual_golden/"
