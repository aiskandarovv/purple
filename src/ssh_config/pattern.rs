//! OpenSSH host-pattern matching.
//!
//! Implements `Host` keyword wildcard semantics: `*`, `?`, `[charset]`,
//! `[!charset]`/`[^charset]`, `[a-z]` ranges and `!pattern` negation.
//! All functions here are pure; they own no state and depend only on
//! `PatternEntry` from the model module.

use super::model::PatternEntry;

/// Does this pattern contain any SSH wildcard metacharacters?
pub fn is_host_pattern(pattern: &str) -> bool {
    pattern.contains('*')
        || pattern.contains('?')
        || pattern.contains('[')
        || pattern.starts_with('!')
        || pattern.contains(' ')
        || pattern.contains('\t')
}

/// Match a text string against an SSH host pattern.
/// Supports `*` (any sequence), `?` (single char), `[charset]` (character class),
/// `[!charset]`/`[^charset]` (negated class), `[a-z]` (ranges) and `!pattern` (negation).
pub fn ssh_pattern_match(pattern: &str, text: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix('!') {
        return !match_glob(rest, text);
    }
    match_glob(pattern, text)
}

/// Core glob matcher without negation prefix handling.
/// Empty text only matches empty pattern.
fn match_glob(pattern: &str, text: &str) -> bool {
    if text.is_empty() {
        return pattern.is_empty();
    }
    if pattern.is_empty() {
        return false;
    }
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match(&pat, &txt)
}

/// Iterative glob matching with star-backtracking.
fn glob_match(pat: &[char], txt: &[char]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star: Option<(usize, usize)> = None; // (pattern_pos, text_pos)

    while ti < txt.len() {
        if pi < pat.len() && pat[pi] == '?' {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star = Some((pi + 1, ti));
            pi += 1;
        } else if pi < pat.len() && pat[pi] == '[' {
            if let Some((matches, end)) = match_char_class(pat, pi, txt[ti]) {
                if matches {
                    pi = end;
                    ti += 1;
                } else if let Some((spi, sti)) = star {
                    let sti = sti + 1;
                    star = Some((spi, sti));
                    pi = spi;
                    ti = sti;
                } else {
                    return false;
                }
            } else if let Some((spi, sti)) = star {
                // Malformed class: backtrack
                let sti = sti + 1;
                star = Some((spi, sti));
                pi = spi;
                ti = sti;
            } else {
                return false;
            }
        } else if pi < pat.len() && pat[pi] == txt[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((spi, sti)) = star {
            let sti = sti + 1;
            star = Some((spi, sti));
            pi = spi;
            ti = sti;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Parse and match a `[...]` character class starting at `pat[start]`.
/// Returns `Some((matched, end_index))` where `end_index` is past `]`.
/// Returns `None` if no closing `]` is found.
fn match_char_class(pat: &[char], start: usize, ch: char) -> Option<(bool, usize)> {
    let mut i = start + 1;
    if i >= pat.len() {
        return None;
    }

    let negate = pat[i] == '!' || pat[i] == '^';
    if negate {
        i += 1;
    }

    let mut matched = false;
    while i < pat.len() && pat[i] != ']' {
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            let lo = pat[i];
            let hi = pat[i + 2];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            matched |= pat[i] == ch;
            i += 1;
        }
    }

    if i >= pat.len() {
        return None;
    }

    let result = if negate { !matched } else { matched };
    Some((result, i + 1))
}

/// Check whether a `Host` pattern matches a given alias.
/// OpenSSH `Host` keyword matches only against the target alias typed on the
/// command line, never against the resolved HostName.
pub fn host_pattern_matches(host_pattern: &str, alias: &str) -> bool {
    let patterns: Vec<&str> = host_pattern.split_whitespace().collect();
    if patterns.is_empty() {
        return false;
    }

    let mut any_positive_match = false;
    for pat in &patterns {
        if let Some(neg) = pat.strip_prefix('!') {
            if match_glob(neg, alias) {
                return false;
            }
        } else if ssh_pattern_match(pat, alias) {
            any_positive_match = true;
        }
    }

    any_positive_match
}

/// Returns true if any hop in a (possibly comma-separated) ProxyJump value
/// matches the given alias. Strips optional `user@` prefix and `:port`
/// suffix from each hop before comparing. Handles IPv6 bracket notation
/// `[addr]:port`. Used to detect self-referencing loops.
pub fn proxy_jump_contains_self(proxy_jump: &str, alias: &str) -> bool {
    proxy_jump.split(',').any(|hop| {
        let h = hop.trim();
        // Strip optional user@ prefix (take everything after the first @).
        let h = h.split_once('@').map_or(h, |(_, host)| host);
        // Strip optional :port suffix. Handle [IPv6]:port bracket notation.
        let h = if let Some(bracketed) = h.strip_prefix('[') {
            bracketed.split_once(']').map_or(h, |(host, _)| host)
        } else {
            h.rsplit_once(':').map_or(h, |(host, _)| host)
        };
        h == alias
    })
}

/// Apply first-match-wins inheritance from a pattern to mutable field refs.
/// Only fills fields that are still empty. Self-referencing ProxyJump values
/// are assigned (SSH would do the same) so the UI can warn about the loop.
pub(super) fn apply_first_match_fields(
    proxy_jump: &mut String,
    user: &mut String,
    identity_file: &mut String,
    p: &PatternEntry,
) {
    if proxy_jump.is_empty() && !p.proxy_jump.is_empty() {
        proxy_jump.clone_from(&p.proxy_jump);
    }
    if user.is_empty() && !p.user.is_empty() {
        user.clone_from(&p.user);
    }
    if identity_file.is_empty() && !p.identity_file.is_empty() {
        identity_file.clone_from(&p.identity_file);
    }
}
