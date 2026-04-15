#!/usr/bin/env bash
# Design system enforcement checks.
# Run after cargo doc and before smoke_tui in the pre-commit sequence.

set -e

# 1. No manual Block construction outside design.rs/mod.rs.
if grep -rn 'Block::bordered()\|Block::new()\.borders(\|Block::default()\.borders(' src/ui/ --include='*.rs' \
    | grep -v 'design\.rs' | grep -v 'mod\.rs' \
    | grep -q .; then
    echo "ERROR: Manual Block construction found outside allowed files."
    echo "       Use design::overlay_block() / overlay_block_line() / plain_overlay_block() /"
    echo "       danger_block() / danger_block_line() / main_block() / main_block_line() /"
    echo "       search_block() / search_block_line()."
    grep -rn 'Block::bordered()\|Block::new()\.borders(\|Block::default()\.borders(' src/ui/ --include='*.rs' \
        | grep -v 'design\.rs' | grep -v 'mod\.rs'
    exit 1
fi

# 2. No direct footer builders (footer_action / footer_key_span) called from screens.
#    Inline `theme::footer_key()` styling inside content (e.g. welcome "Press ? for help"
#    or the host-list compound title's tag labels) is allowed — those are content spans,
#    not footer actions. Footer actions must flow through the `design::Footer` builder.
if grep -rn 'super::footer_action\|super::footer_key_span' src/ui/ \
    --include='*.rs' | grep -v 'design\.rs' | grep -v 'mod\.rs' \
    | grep -q .; then
    echo "ERROR: Manual footer construction found. Use design::Footer builder."
    grep -rn 'super::footer_action\|super::footer_key_span' src/ui/ \
        --include='*.rs' | grep -v 'design\.rs' | grep -v 'mod\.rs'
    exit 1
fi

# 3. No old notification API outside method definitions and delegations.
if grep -rn 'set_status\|set_background_status\|set_sticky_status\|set_info_status' \
    src/ --include='*.rs' \
    | grep -v 'tests\.rs' | grep -v 'test_' | grep -v '#\[deprecated' \
    | grep -v 'pub fn ' | grep -v 'pub use ' \
    | grep -v 'self\.set_' | grep -v '// ' | grep -v '/// ' \
    | grep -q .; then
    echo "ERROR: Old notification API used. Use app.notify/notify_error/etc."
    grep -rn 'set_status\|set_background_status\|set_sticky_status\|set_info_status' \
        src/ --include='*.rs' \
        | grep -v 'tests\.rs' | grep -v 'test_' | grep -v '#\[deprecated' \
        | grep -v 'pub fn ' | grep -v 'pub use ' \
        | grep -v 'self\.set_' | grep -v '// ' | grep -v '/// '
    exit 1
fi

# 4. No direct centered_rect calls from screen files.
if grep -rn 'centered_rect(' src/ui/ --include='*.rs' \
    | grep -v 'design\.rs' | grep -v 'mod\.rs' | grep -q .; then
    echo "ERROR: Direct centered_rect() call found. Use design::overlay_area()."
    grep -rn 'centered_rect(' src/ui/ --include='*.rs' \
        | grep -v 'design\.rs' | grep -v 'mod\.rs'
    exit 1
fi

# 5. No hardcoded highlight_symbol outside design.rs/mod.rs
if grep -rn 'highlight_symbol("' src/ui/ --include='*.rs' \
    | grep -v 'design\.rs' | grep -v 'mod\.rs' | grep -q .; then
    echo "ERROR: Hardcoded highlight_symbol found. Use design::LIST_HIGHLIGHT or design::HOST_HIGHLIGHT."
    grep -rn 'highlight_symbol("' src/ui/ --include='*.rs' \
        | grep -v 'design\.rs' | grep -v 'mod\.rs'
    exit 1
fi

echo "Design system checks: OK"
