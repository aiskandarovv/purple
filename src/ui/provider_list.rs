use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::{App, ProviderFormField};
use crate::history::ConnectionHistory;

/// Render the provider management list as a centered overlay.
pub fn render_provider_list(frame: &mut Frame, app: &mut App) {
    let sorted_names = app.sorted_provider_names();

    // Overlay: percentage-based width, height fits content. Reserve 1 row
    // below the block for the external footer.
    let item_count = sorted_names.len();
    let height = (item_count as u16 + 3).min(frame.area().height.saturating_sub(5));
    let area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, height);
    frame.render_widget(Clear, area);

    let block = design::overlay_block("Providers");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Content width inside the overlay
    let content_width = inner.width as usize;

    let items: Vec<ListItem> = sorted_names
        .iter()
        .map(|name| {
            let display_name = crate::providers::provider_display_name(name.as_str());
            let configured = app.providers.config.section(name.as_str()).is_some();

            let name_col = format!(" {:<16}", display_name);
            let mut spans = vec![Span::styled(name_col, theme::bold())];
            let mut used = 17;

            if configured {
                let has_error = app
                    .providers
                    .sync_history
                    .get(name.as_str())
                    .is_some_and(|r| r.is_error);
                if has_error {
                    spans.push(Span::styled(design::ICON_WARNING, theme::error()));
                } else {
                    spans.push(Span::styled(design::ICON_SUCCESS, theme::success()));
                }
                used += 1;

                if let Some(section) = app.providers.config.section(name.as_str()) {
                    if !section.auto_sync {
                        spans.push(Span::styled(" (manual)", theme::muted()));
                        used += 9;
                    }
                }

                // Stale count for this provider
                let stale_count = app
                    .hosts_state
                    .list
                    .iter()
                    .filter(|h| h.stale.is_some() && h.provider.as_deref() == Some(name.as_str()))
                    .count();

                // Sync detail on same line
                if app.providers.syncing.contains_key(name.as_str()) {
                    let max = content_width.saturating_sub(used + 2);
                    if max > 1 {
                        spans.push(Span::styled(
                            format!("  {}", super::truncate("syncing...", max)),
                            theme::muted(),
                        ));
                    }
                } else if let Some(record) = app.providers.sync_history.get(name.as_str()) {
                    let ago = ConnectionHistory::format_time_ago(record.timestamp);
                    // Build segments: "N servers" [", N stale"] [", Xm ago"]
                    let prefix = format!("  {}", record.message);
                    let stale_text = if stale_count > 0 {
                        format!(", {} stale", stale_count)
                    } else {
                        String::new()
                    };
                    let ago_text = if ago.is_empty() {
                        String::new()
                    } else {
                        format!(", {} ago", ago)
                    };
                    let max = content_width.saturating_sub(used);
                    let total_len = prefix.len() + stale_text.len() + ago_text.len();
                    if max > 1 && total_len <= max {
                        spans.push(Span::styled(prefix, theme::muted()));
                        if stale_count > 0 {
                            spans.push(Span::styled(stale_text, theme::warning()));
                        }
                        if !ago_text.is_empty() {
                            spans.push(Span::styled(ago_text, theme::muted()));
                        }
                    } else if max > 1 {
                        // Fallback: truncate combined string
                        let combined = format!("{}{}{}", prefix, stale_text, ago_text);
                        spans.push(Span::styled(
                            super::truncate(&combined, max),
                            theme::muted(),
                        ));
                    }
                }
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::LIST_HIGHLIGHT);

    frame.render_stateful_widget(list, inner, &mut app.ui.provider_list_state);

    // Footer below the block
    let footer_area = design::render_overlay_footer(frame, area);
    if app.providers.pending_delete.is_some() {
        let name = app.providers.pending_delete.as_deref().unwrap_or("");
        let display = crate::providers::provider_display_name(name);
        let mut spans = vec![Span::styled(
            format!(" Remove {}? ", display),
            theme::bold(),
        )];
        // Stakes test: removing the provider config is destructive
        // (synced hosts stay but the integration is gone). Action verbs.
        spans.extend(design::confirm_footer_destructive("remove", "keep").into_spans());
        super::render_footer_with_status(frame, footer_area, spans, app);
    } else {
        // Count stale hosts for selected provider
        let selected_stale_count: usize = app
            .ui
            .provider_list_state
            .selected()
            .and_then(|idx| sorted_names.get(idx))
            .map(|name| {
                app.hosts_state
                    .list
                    .iter()
                    .filter(|h| h.stale.is_some() && h.provider.as_deref() == Some(name.as_str()))
                    .count()
            })
            .unwrap_or(0);

        let mut f = design::Footer::new()
            .primary("Enter", " edit ")
            .action("s", " sync ")
            .action("d", " remove ");
        if selected_stale_count > 0 {
            f = f.action("X", &format!(" purge {} stale ", selected_stale_count));
        }
        f = f.action("Esc", " back");
        f.render_with_status(frame, footer_area, app);
    }
}

/// Render the provider configuration form.
pub fn render_provider_form(frame: &mut Frame, app: &mut App, provider_name: &str) {
    let display_name = crate::providers::provider_display_name(provider_name);
    let title = format!("Providers > {}", display_name);

    let expanded = app.providers.form.expanded;
    // Progressive disclosure: when `vault_role` is empty, `VaultAddr` is
    // filtered out by `visible_fields(provider)` and therefore never
    // rendered or navigable. Re-enabling the role brings the field back
    // with whatever value the user had previously typed.
    let filtered_all: Vec<ProviderFormField> = app.providers.form.visible_fields(provider_name);
    let all_fields: &[ProviderFormField] = &filtered_all;
    let required_count = all_fields
        .iter()
        .filter(|f| ProviderFormField::is_required_field(**f, provider_name))
        .count();
    // VaultRole and VaultAddr are both optional fields and are gated behind
    // the expanded state, identical to every other non-required field.
    // Per-host VaultSsh (in host_form.rs) follows the same rule.
    // TODO: Enter-to-pick from `vault list <mount>/roles`
    let base_fields: &[ProviderFormField] = if expanded {
        all_fields
    } else {
        // Required fields are always first in fields_for() ordering
        &all_fields[..required_count]
    };
    let visible_fields: &[ProviderFormField] = base_fields;
    // Block: top(1) + fields * 2 (divider + content) + bottom(1)
    let block_height = 2 + visible_fields.len() as u16 * 2;
    let total_height = block_height + 1; // + footer

    let form_area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, total_height);
    frame.render_widget(Clear, form_area);

    let block_area = Rect::new(form_area.x, form_area.y, form_area.width, block_height);

    let block = design::overlay_block(&title);

    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    let mut y_offset: u16 = 0;
    for &field in visible_fields.iter() {
        let divider_y = inner.y + y_offset;
        let content_y = divider_y + 1;
        y_offset += 2;

        let is_focused = app.providers.form.focused_field == field;
        let label_style = if is_focused {
            theme::accent_bold()
        } else {
            theme::muted()
        };
        let is_mandatory = ProviderFormField::is_mandatory_field(field, provider_name);
        let field_label =
            if field == ProviderFormField::Regions && matches!(provider_name, "scaleway" | "gcp") {
                "Zones"
            } else if field == ProviderFormField::Regions && provider_name == "azure" {
                "Subscriptions"
            } else if field == ProviderFormField::Regions && provider_name == "ovh" {
                "Endpoint"
            } else {
                field.label()
            };
        let label = if is_mandatory {
            format!(" {}* ", field_label)
        } else {
            format!(" {} ", field_label)
        };
        super::render_divider(
            frame,
            block_area,
            divider_y,
            &label,
            label_style,
            theme::accent(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        render_field_content(
            frame,
            content_area,
            field,
            &app.providers.form,
            provider_name,
        );
    }

    // Footer below the block. Discard prompt takes precedence; otherwise
    // dynamic save footer reflects the focused field's kind so users discover
    // Space-toggle (VerifyTls/AutoSync) and Space-pick (IdentityFile/Regions).
    let footer_area = design::render_overlay_footer(frame, block_area);
    if app.forms.pending_discard_confirm {
        design::render_discard_prompt(frame, footer_area, app);
    } else {
        let mode = if !expanded && visible_fields.len() < all_fields.len() {
            design::FormFooterMode::Collapsed
        } else {
            design::FormFooterMode::Expanded(app.providers.form.focused_field.kind(provider_name))
        };
        design::form_save_footer(mode).render_with_status(frame, footer_area, app);
    }

    // Key picker popup overlay
    if app.ui.key_picker.open {
        super::host_form::render_key_picker_overlay(frame, app);
    }

    // Region picker popup overlay
    if app.ui.region_picker.open {
        render_region_picker_overlay(frame, app);
    }
}

fn placeholder_for(field: ProviderFormField, provider_name: &str) -> &'static str {
    use crate::messages::hints;
    match field {
        ProviderFormField::Url => hints::PROVIDER_URL,
        ProviderFormField::Token => match provider_name {
            "proxmox" => hints::PROVIDER_TOKEN_PROXMOX,
            "aws" => hints::PROVIDER_TOKEN_AWS,
            "gcp" => hints::PROVIDER_TOKEN_GCP,
            "azure" => hints::PROVIDER_TOKEN_AZURE,
            "tailscale" => hints::PROVIDER_TOKEN_TAILSCALE,
            "oracle" => hints::PROVIDER_TOKEN_ORACLE,
            "ovh" => hints::PROVIDER_TOKEN_OVH,
            _ => hints::PROVIDER_TOKEN_DEFAULT,
        },
        ProviderFormField::Profile => hints::PROVIDER_PROFILE,
        ProviderFormField::Project => match provider_name {
            "ovh" => hints::PROVIDER_PROJECT_OVH,
            _ => hints::PROVIDER_PROJECT_DEFAULT,
        },
        ProviderFormField::Compartment => hints::PROVIDER_COMPARTMENT,
        ProviderFormField::Regions => match provider_name {
            "gcp" => hints::PROVIDER_REGIONS_GCP,
            "scaleway" => hints::PROVIDER_REGIONS_SCALEWAY,
            "azure" => hints::PROVIDER_REGIONS_AZURE,
            "ovh" => hints::PROVIDER_REGIONS_OVH,
            _ => hints::PROVIDER_REGIONS_DEFAULT,
        },
        // Alias prefix suggestions are provider short labels (identifiers),
        // not translatable copy, so they stay inline.
        ProviderFormField::AliasPrefix => match provider_name {
            "digitalocean" => "do",
            "vultr" => "vultr",
            "linode" => "linode",
            "hetzner" => "hetzner",
            "upcloud" => "uc",
            "proxmox" => "pve",
            "aws" => "aws",
            "scaleway" => "scw",
            "gcp" => "gcp",
            "azure" => "az",
            "tailscale" => "ts",
            "oracle" => "oci",
            "ovh" => "ovh",
            _ => hints::PROVIDER_ALIAS_PREFIX_DEFAULT,
        },
        ProviderFormField::User => match provider_name {
            "aws" => hints::PROVIDER_USER_AWS,
            "gcp" => hints::PROVIDER_USER_GCP,
            "azure" => hints::PROVIDER_USER_AZURE,
            "oracle" => hints::PROVIDER_USER_ORACLE,
            "ovh" => hints::PROVIDER_USER_OVH,
            _ => hints::DEFAULT_SSH_USER,
        },
        ProviderFormField::IdentityFile => hints::IDENTITY_FILE_PICK,
        ProviderFormField::VaultRole => hints::PROVIDER_VAULT_ROLE,
        ProviderFormField::VaultAddr => hints::PROVIDER_VAULT_ADDR,
        ProviderFormField::VerifyTls | ProviderFormField::AutoSync => "",
    }
}

fn render_field_content(
    frame: &mut Frame,
    area: Rect,
    field: ProviderFormField,
    form: &crate::app::ProviderFormFields,
    provider_name: &str,
) {
    let is_focused = form.focused_field == field;

    // Toggle fields
    if field == ProviderFormField::VerifyTls {
        let value_text = if form.verify_tls {
            "yes"
        } else {
            "no (accept self-signed)"
        };
        render_toggle_content(frame, area, value_text, is_focused);
        return;
    }
    if field == ProviderFormField::AutoSync {
        let value_text = if form.auto_sync {
            "yes"
        } else {
            "no (sync manually)"
        };
        render_toggle_content(frame, area, value_text, is_focused);
        return;
    }

    let value = match field {
        ProviderFormField::Url => &form.url,
        ProviderFormField::Token => &form.token,
        ProviderFormField::Profile => &form.profile,
        ProviderFormField::Project => &form.project,
        ProviderFormField::Compartment => &form.compartment,
        ProviderFormField::Regions => &form.regions,
        ProviderFormField::AliasPrefix => &form.alias_prefix,
        ProviderFormField::User => &form.user,
        ProviderFormField::IdentityFile => &form.identity_file,
        ProviderFormField::VaultRole => &form.vault_role,
        ProviderFormField::VaultAddr => &form.vault_addr,
        ProviderFormField::VerifyTls | ProviderFormField::AutoSync => {
            debug_assert!(
                false,
                "toggle fields must be handled by the early-return branches above"
            );
            return;
        }
    };

    // Mask token except last 4 chars when not focused
    let display_value: String =
        if field == ProviderFormField::Token && !value.is_empty() && !is_focused {
            let char_count = value.chars().count();
            if char_count > 4 {
                let last4: String = value.chars().skip(char_count - 4).collect();
                format!("{}{}", "*".repeat(char_count - 4), last4)
            } else {
                value.clone()
            }
        } else {
            value.clone()
        };

    let is_picker = matches!(field, ProviderFormField::IdentityFile)
        || (field == ProviderFormField::Regions
            && matches!(provider_name, "aws" | "scaleway" | "gcp" | "oracle" | "ovh"));

    let content = if value.is_empty() && is_focused && !is_picker {
        Line::from(Span::styled(
            placeholder_for(field, provider_name),
            theme::muted(),
        ))
    } else if is_picker && is_focused {
        let inner_width = area.width as usize;
        let arrow_pos = inner_width.saturating_sub(1);
        let (display, display_style) = if value.is_empty() {
            (
                placeholder_for(field, provider_name).to_string(),
                theme::muted(),
            )
        } else {
            (display_value.clone(), theme::bold())
        };
        let val_width = display.width();
        let gap = arrow_pos.saturating_sub(val_width);
        Line::from(vec![
            Span::styled(display, display_style),
            Span::raw(" ".repeat(gap)),
            Span::styled(design::PICKER_ARROW, theme::muted()),
        ])
    } else if display_value.is_empty() {
        Line::from(Span::raw(""))
    } else {
        Line::from(Span::styled(display_value, theme::bold()))
    };

    frame.render_widget(Paragraph::new(content), area);

    if is_focused {
        let prefix: String = value.chars().take(form.cursor_pos).collect();
        let cursor_x = area
            .x
            .saturating_add(prefix.width().min(u16::MAX as usize) as u16);
        let cursor_y = area.y;
        if area.width > 0 && cursor_x < area.x.saturating_add(area.width) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn render_toggle_content(frame: &mut Frame, area: Rect, value_text: &str, is_focused: bool) {
    let content = if is_focused {
        let inner_width = area.width as usize;
        let val_width = value_text.width();
        let gap = inner_width.saturating_sub(val_width + 3);
        Line::from(vec![
            Span::styled(value_text, theme::bold()),
            Span::raw(" ".repeat(gap)),
            Span::styled(design::TOGGLE_HINT, theme::muted()),
        ])
    } else {
        Line::from(Span::styled(value_text, theme::bold()))
    };
    frame.render_widget(Paragraph::new(content), area);
}

/// Build display rows for the grouped region/zone picker.
/// Returns a list of (label, Option<region_code>) pairs.
/// Group headers have None as region_code, regions have Some(code).
fn build_region_rows(provider: &str) -> Vec<(String, Option<&'static str>)> {
    let (zones, groups) = crate::handler::zone_data_for(provider);
    let mut rows = Vec::new();
    for &(label, start, end) in groups {
        rows.push((format!(" {}", label), None));
        for &(code, name) in &zones[start..end] {
            rows.push((format!("{} {}", code, name), Some(code)));
        }
    }
    rows
}

fn render_region_picker_overlay(frame: &mut Frame, app: &mut App) {
    let provider_name = match &app.screen {
        crate::app::Screen::ProviderForm { provider } => provider.as_str(),
        _ => "aws",
    };
    let rows = build_region_rows(provider_name);
    let selected: std::collections::HashSet<&str> = app
        .providers
        .form
        .regions
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let visible_rows = 18u16;
    let block_height = visible_rows + 2; // top + bottom border
    let total_height = block_height + 1; // + footer
    let picker_area = design::overlay_area(frame, 60, 80, total_height);
    frame.render_widget(Clear, picker_area);

    let count = selected.len();
    let zone_label = if matches!(provider_name, "scaleway" | "gcp") {
        "Zones"
    } else if provider_name == "ovh" {
        "Endpoint"
    } else {
        "Regions"
    };
    let title = format!("Select {} ({} selected)", zone_label, count);
    let block_area = Rect::new(
        picker_area.x,
        picker_area.y,
        picker_area.width,
        block_height,
    );
    let block = design::overlay_block(&title);
    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    // Scroll so cursor is always visible
    let cursor = app.ui.region_picker.cursor;
    let scroll_offset = if cursor >= visible_rows as usize {
        cursor - visible_rows as usize + 1
    } else {
        0
    };

    for (i, y) in (0..visible_rows as usize).zip(inner.y..) {
        let idx = scroll_offset + i;
        if idx >= rows.len() {
            break;
        }
        let (label, region_code) = &rows[idx];
        let is_cursor = idx == cursor;

        if let Some(code) = region_code {
            // Region row
            let is_selected = selected.contains(code);
            let check: String = if is_selected {
                format!(" {} ", design::ICON_SUCCESS)
            } else {
                "   ".to_string()
            };
            let display = format!("{}{}", check, label);
            let style = if is_cursor {
                theme::selected_row()
            } else if is_selected {
                theme::bold()
            } else {
                theme::muted()
            };
            let row_area = Rect::new(inner.x, y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    super::truncate(&display, inner.width as usize),
                    style,
                ))),
                row_area,
            );
        } else {
            // Group header
            let style = if is_cursor {
                theme::selected_row()
            } else {
                theme::accent_bold()
            };
            let row_area = Rect::new(inner.x, y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    super::truncate(label, inner.width as usize),
                    style,
                ))),
                row_area,
            );
        }
    }

    let footer_area = design::render_overlay_footer(frame, block_area);
    design::Footer::new()
        .action("Space", " toggle ")
        .primary("Enter", " done ")
        .action("Esc", " back")
        .render_with_status(frame, footer_area, app);
}

#[cfg(test)]
mod tests {
    use super::super::truncate;
    use super::render_field_content;
    use crate::app::{ProviderFormField, ProviderFormFields};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    // Every ProviderFormField variant must render without hitting the
    // debug_assert fallback. Adding a new variant to the enum without
    // handling it in render_field_content will cause the match below to
    // fail to compile, flagging the gap before it reaches production.
    #[test]
    fn render_field_content_handles_every_variant() {
        let form = ProviderFormFields::new();
        let area = Rect::new(0, 0, 40, 1);
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        let all: &[ProviderFormField] = &[
            ProviderFormField::Url,
            ProviderFormField::Token,
            ProviderFormField::Profile,
            ProviderFormField::Project,
            ProviderFormField::Compartment,
            ProviderFormField::Regions,
            ProviderFormField::AliasPrefix,
            ProviderFormField::User,
            ProviderFormField::IdentityFile,
            ProviderFormField::VerifyTls,
            ProviderFormField::VaultRole,
            ProviderFormField::VaultAddr,
            ProviderFormField::AutoSync,
        ];

        // Exhaustiveness guard: the compiler forces this match to cover
        // every variant. Add new variants to `all` above when adding them
        // to ProviderFormField.
        for variant in all {
            match variant {
                ProviderFormField::Url
                | ProviderFormField::Token
                | ProviderFormField::Profile
                | ProviderFormField::Project
                | ProviderFormField::Compartment
                | ProviderFormField::Regions
                | ProviderFormField::AliasPrefix
                | ProviderFormField::User
                | ProviderFormField::IdentityFile
                | ProviderFormField::VerifyTls
                | ProviderFormField::VaultRole
                | ProviderFormField::VaultAddr
                | ProviderFormField::AutoSync => {}
            }
        }

        for variant in all {
            terminal
                .draw(|frame| render_field_content(frame, area, *variant, &form, "aws"))
                .unwrap();
        }
    }

    #[test]
    fn truncate_fits() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_fit() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_ascii() {
        assert_eq!(truncate("hello world", 8), "hello w…");
    }

    #[test]
    fn truncate_no_room() {
        assert_eq!(truncate("hello", 1), "");
        assert_eq!(truncate("hello", 0), "");
    }

    #[test]
    fn truncate_wide_cjk() {
        // CJK chars are 2 columns wide each. "你好世界" = 8 columns.
        // With max 5: target = 4 columns, fits "你好" (4 cols) + "…"
        assert_eq!(truncate("你好世界", 5), "你好…");
    }

    #[test]
    fn truncate_wide_cjk_odd_boundary() {
        // max 4: target = 3 columns, "你" = 2 cols fits, "好" = 2 cols won't
        assert_eq!(truncate("你好世界", 4), "你…");
    }

    #[test]
    fn truncate_mixed_ascii_cjk() {
        // "ab你好" = 2 + 4 = 6 columns. max 5: target = 4, "ab你" fits (4 cols)
        assert_eq!(truncate("ab你好", 5), "ab你…");
    }

    #[test]
    fn truncate_multibyte_emoji() {
        // "🚀🔥" = 2+2 = 4 columns (each emoji is 2 cols wide).
        // max 3: target = 2, "🚀" fits (2 cols)
        assert_eq!(truncate("🚀🔥", 3), "🚀…");
    }

    #[test]
    fn provider_list_layout_has_spacer() {
        use ratatui::layout::{Constraint, Layout, Rect};
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(chunks[1].height, 1);
        assert_eq!(chunks[2].height, 1);
        assert!(chunks[2].y > chunks[0].y + chunks[0].height);
    }
}
