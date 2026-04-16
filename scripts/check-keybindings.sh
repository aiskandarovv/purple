#!/usr/bin/env bash
# Keyboard interaction enforcement.
#
# Codifies the four invariants from CLAUDE.md "Keyboard interaction rules"
# as commit-time checks so future contributions cannot regress them:
#
#   1. Enter ALWAYS submits a form (never opens pickers).
#   2. Space activates the focused field (picker/toggle/literal).
#   3. Confirm dialogs accept y/n/Esc only — no `_ =>` catch-all that
#      transitions screen state.
#   4. Confirm footer labels follow the stakes test (action verbs for
#      destructive confirms).
#
# This script enforces invariant 1 (Enter must not open pickers in form
# handlers), the route_confirm_key adoption hint, and the destructive
# confirm-footer helper for new confirms. Invariant 4's specific verb
# choices are a content decision left to humans.

set -e
FAIL=0

# 1. Form handlers must NOT dispatch Enter to picker opens.
#    The handler must call submit_*() unconditionally on Enter; pickers
#    are activated via Space (Char(' ')). Picker selection inside picker.rs
#    legitimately uses Enter (to choose an item) and is excluded.
ENTER_HITS=$(grep -rn 'show_key_picker\|show_proxyjump_picker\|show_vault_role_picker\|show_password_picker\|show_region_picker' \
    src/handler/host_form.rs src/handler/provider.rs --include='*.rs' \
    -B 1 \
    | grep 'KeyCode::Enter' \
    || true)
if [ -n "$ENTER_HITS" ]; then
    echo "ERROR: Enter dispatches to picker open in a form handler."
    echo "       Enter must always submit; pickers open on Space."
    echo "       See CLAUDE.md 'Keyboard interaction rules' invariant 1."
    echo "$ENTER_HITS"
    FAIL=1
fi

# 2. Confirm handlers must use route_confirm_key (or have no `_ =>` arm
#    that transitions state). Detection heuristic: any handler function
#    that matches Char('y') / Char('Y') AND Char('n') in close proximity
#    is a confirm handler. We scan for `_ =>` lines with `app.screen` or
#    `app.pending_*` mutations within ~4 lines after a Char('y') match.
#
#    Conservative implementation: flag any file under src/handler/ that has
#    BOTH `KeyCode::Char('y')` AND `_ =>` AND `app.screen =` within a
#    20-line window, except confirm.rs (which now uses route_confirm_key).
CONFIRM_FILES=$(grep -rln "KeyCode::Char('y')" src/handler/ --include='*.rs' || true)
for file in $CONFIRM_FILES; do
    # Find each Char('y') line; check the 20 lines that follow for both
    # `_ =>` and a state transition. False positives are acceptable here
    # because the fix is to switch to route_confirm_key.
    awk '
        /KeyCode::Char\(.y.\)/ { window = 20; saw_catch = 0; saw_state = 0 }
        window > 0 {
            if (/^[[:space:]]*_ =>/) saw_catch = 1
            if (saw_catch && /app\.screen[[:space:]]*=/) saw_state = 1
            if (saw_state) {
                print FILENAME ":" NR ": catch-all `_ =>` transitions screen state in confirm handler"
                window = 0
                saw_catch = 0
                saw_state = 0
            }
            window--
        }
    ' "$file" > /tmp/keybindings_check_hits.$$
    if [ -s /tmp/keybindings_check_hits.$$ ]; then
        echo "ERROR: Confirm handler has a `_ =>` arm that transitions state."
        echo "       Use handler::route_confirm_key(key) and match"
        echo "       ConfirmAction::{Yes, No, Ignored} explicitly. Stray keys"
        echo "       must not silently cancel destructive operations."
        echo "       See CLAUDE.md 'Keyboard interaction rules' invariant 3."
        cat /tmp/keybindings_check_hits.$$
        rm -f /tmp/keybindings_check_hits.$$
        FAIL=1
    else
        rm -f /tmp/keybindings_check_hits.$$
    fi
done

# 3. Confirm handlers with both Char('y') AND Char('Y') (the canonical
#    case-insensitive y/Y pattern) must also handle Char('n'). This avoids
#    false positives on lowercase-only `y` shortcuts (e.g. host list yank).
#    Confirm dialogs always handle both cases of the affirmative key.
for file in $CONFIRM_FILES; do
    # Skip files that route through the helper (they handle n via the helper)
    if grep -q 'route_confirm_key' "$file"; then
        continue
    fi
    # Only flag when both lower- and upper-case y are present (confirm pattern).
    if ! grep -q "KeyCode::Char('Y')" "$file"; then
        continue
    fi
    if ! grep -q "KeyCode::Char('n')" "$file"; then
        echo "ERROR: $file matches Char('y')|Char('Y') but not Char('n')."
        echo "       Confirm dialogs must accept n/N as cancel (uniform with"
        echo "       Esc). Either add an explicit Char('n') | Char('N') arm,"
        echo "       or migrate to handler::route_confirm_key(key)."
        echo "       See CLAUDE.md 'Keyboard interaction rules' invariant 3."
        FAIL=1
    fi
done

# 4. New confirm-style footers should use the design helpers, not raw
#    Footer::new().action("y", ...). The helpers encode the stakes test.
#    Existing bare Footer usage in tests and overlays that genuinely need
#    custom labels is allowed via an opt-out comment.
RAW_CONFIRM=$(grep -rn '\.action("y", " yes "\|\.action("y", " confirm "' \
    src/ui/ --include='*.rs' \
    | grep -v 'design\.rs' \
    | grep -v 'test' \
    || true)
if [ -n "$RAW_CONFIRM" ]; then
    echo "ERROR: Raw y/yes or y/confirm footer construction outside design.rs."
    echo "       Use design::confirm_footer_destructive(yes_verb, no_verb) for"
    echo "       destructive confirms (delete, sign, purge) or"
    echo "       design::confirm_footer_benign() for re-enterable yes/no."
    echo "       See CLAUDE.md 'Keyboard interaction rules' invariant 4."
    echo "$RAW_CONFIRM"
    FAIL=1
fi

if [ $FAIL -eq 0 ]; then
    echo "Keyboard interaction checks: OK"
fi

exit $FAIL
