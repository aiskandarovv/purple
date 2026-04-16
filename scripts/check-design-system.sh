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

# 6. No local padded() closures in screen files (use design::padded_usize).
if grep -rEn 'w \+ w / 10' src/ui/ --include='*.rs' \
    | grep -v 'design\.rs' | grep -v 'mod\.rs' | grep -q .; then
    echo "ERROR: Local padded() closure found. Use design::padded_usize()."
    grep -rEn 'w \+ w / 10' src/ui/ --include='*.rs' \
        | grep -v 'design\.rs' | grep -v 'mod\.rs'
    exit 1
fi

# 7. No local render_divider wrappers in screen files (call super::render_divider directly).
if grep -rEn '^fn render_divider\(' src/ui/ --include='*.rs' \
    | grep -v 'mod\.rs' | grep -q .; then
    echo "ERROR: Local render_divider() wrapper found. Call super::render_divider() directly."
    grep -rEn '^fn render_divider\(' src/ui/ --include='*.rs' \
        | grep -v 'mod\.rs'
    exit 1
fi

# 8. No inline picker/toggle glyphs outside design.rs (use design::PICKER_ARROW / TOGGLE_HINT).
if grep -rEn '"\\u\{25B8\}"|"\\u\{2423\}"' src/ui/ --include='*.rs' \
    | grep -v 'design\.rs' | grep -q .; then
    echo "ERROR: Inline glyph found. Use design::PICKER_ARROW or design::TOGGLE_HINT."
    grep -rEn '"\\u\{25B8\}"|"\\u\{2423\}"' src/ui/ --include='*.rs' \
        | grep -v 'design\.rs'
    exit 1
fi

# 9. Golden file count matches expected screen count.
GOLDEN_COUNT=$(ls tests/visual_golden/*.golden 2>/dev/null | wc -l | tr -d ' ')
EXPECTED_GOLDEN=29
if [ "$GOLDEN_COUNT" != "$EXPECTED_GOLDEN" ]; then
    echo "ERROR: Expected $EXPECTED_GOLDEN golden files, found $GOLDEN_COUNT."
    echo "If you added a new Screen variant, add a visual regression test and update EXPECTED_GOLDEN."
    exit 1
fi

# 10. No content_and_footer / content_spacer_footer usage (use design::form_footer).
# These helpers were removed when all overlay footers unified to render below the block border.
if grep -rEn 'content_and_footer\(|content_spacer_footer\(' src/ui/ --include='*.rs' | grep -q .; then
    echo "ERROR: content_and_footer / content_spacer_footer were removed. Use design::form_footer for footer placement."
    grep -rEn 'content_and_footer\(|content_spacer_footer\(' src/ui/ --include='*.rs'
    exit 1
fi

# 11. No render_picker_overlay_wide usage (removed in favour of single uniform picker).
# All pickers must call render_picker_overlay so they share the same width range and
# height ceiling (design::PICKER_MIN_W..=PICKER_MAX_W and design::PICKER_MAX_H).
if grep -rEn 'render_picker_overlay_wide' src/ui/ --include='*.rs' | grep -q .; then
    echo "ERROR: render_picker_overlay_wide was removed. All pickers must use render_picker_overlay."
    grep -rEn 'render_picker_overlay_wide' src/ui/ --include='*.rs'
    exit 1
fi

# 12. Picker overlays must use picker_overlay_width(frame), not raw PICKER_MIN_W or
# overlay_area(70, ...) for width. This keeps every picker the same visual size.
if grep -rEn 'centered_rect_fixed\(design::PICKER_MIN_W,' src/ui/ --include='*.rs' \
    | grep -v 'design\.rs\|mod\.rs' | grep -q .; then
    echo "ERROR: Picker uses raw PICKER_MIN_W for width. Use super::picker_overlay_width(frame)."
    grep -rEn 'centered_rect_fixed\(design::PICKER_MIN_W,' src/ui/ --include='*.rs' \
        | grep -v 'design\.rs\|mod\.rs'
    exit 1
fi

# 13. No internal footer rendering to Layout chunks in overlay screens.
# Overlays must render footers via design::render_overlay_footer / form_footer,
# not via Layout::vertical footer rows. host_list.rs is the main screen (not an
# overlay) and is the only allowed exception.
if grep -rEn 'render_footer_with_status\(frame, (chunks|rows|inner_chunks)\[|render_with_status\(frame, (chunks|rows|inner_chunks)\[' \
    src/ui/ --include='*.rs' | grep -v 'host_list\.rs' | grep -v 'test' | grep -q .; then
    echo "ERROR: Footer rendered to Layout chunk instead of design::render_overlay_footer."
    grep -rEn 'render_footer_with_status\(frame, (chunks|rows|inner_chunks)\[|render_with_status\(frame, (chunks|rows|inner_chunks)\[' \
        src/ui/ --include='*.rs' | grep -v 'host_list\.rs' | grep -v 'test'
    exit 1
fi

echo "Design system checks: OK"
