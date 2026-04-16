#!/usr/bin/env bash
# Message centralization enforcement.
# Ensures all user-facing strings go through src/messages.rs.
# Run as part of the pre-commit checks.

set -e

FAIL=0

# 1. No hardcoded string literals in notify calls (handler + app code).
#    Allows: crate::messages::, variable refs, test files.
for pattern in \
    '\.notify("[A-Z]' \
    '\.notify_error("[A-Z]' \
    '\.notify_warning("[A-Z]' \
    '\.notify_info("[A-Z]' \
    '\.notify_background("[A-Z]' \
    '\.notify_progress("[A-Z]' \
    '\.notify_sticky_error("[A-Z]' \
    '\.notify_background_error("[A-Z]'; do
    HITS=$(grep -rn "$pattern" src/handler/ src/handler.rs src/app.rs src/main.rs \
        --include='*.rs' \
        | grep -v '_tests\.rs' \
        | grep -v 'tests\.rs' \
        | grep -v '#\[cfg(test)\]' \
        || true)
    if [ -n "$HITS" ]; then
        echo "ERROR: Hardcoded string in notify call. Use crate::messages::*"
        echo "$HITS"
        FAIL=1
    fi
done

# 2. No format! inside notify calls (should use messages:: functions).
for pattern in \
    '\.notify(format!' \
    '\.notify_error(format!' \
    '\.notify_warning(format!' \
    '\.notify_info(format!' \
    '\.notify_background(format!' \
    '\.notify_progress(format!' \
    '\.notify_sticky_error(format!' \
    '\.notify_background_error(format!'; do
    HITS=$(grep -rn "$pattern" src/handler/ src/handler.rs src/app.rs src/main.rs \
        --include='*.rs' \
        | grep -v '_tests\.rs' \
        | grep -v 'tests\.rs' \
        || true)
    if [ -n "$HITS" ]; then
        echo "ERROR: format! inside notify call. Move to crate::messages::*"
        echo "$HITS"
        FAIL=1
    fi
done

# 3. No .to_string() on string literals passed to notify (sign of inline text).
HITS=$(grep -rn '\.notify.*".*"\.to_string()' src/handler/ src/handler.rs src/app.rs src/main.rs \
    --include='*.rs' \
    | grep -v '_tests\.rs' \
    | grep -v 'tests\.rs' \
    || true)
if [ -n "$HITS" ]; then
    echo "ERROR: Inline .to_string() in notify call. Use crate::messages::*"
    echo "$HITS"
    FAIL=1
fi

if [ $FAIL -eq 0 ]; then
    echo "Message centralization checks: OK"
fi

exit $FAIL
