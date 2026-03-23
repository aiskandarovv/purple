## 2.8.1

- Add CI workflow with format, clippy, test, cargo-deny and MSRV checks
- Fix parser handling of lone \r line endings breaking round-trip idempotency
- Add property-based and fuzz testing for SSH config parser
- Add Dependabot for weekly cargo and GitHub Actions updates
- Add cargo-deny for license and vulnerability scanning
- Update GitHub Actions to latest versions (checkout v6, upload-artifact v7, download-artifact v8)
- Update rustls-webpki to 0.103.10 (security fix)

## 2.8.0

- Welcome screen shows host count and offers known_hosts import on first launch
- Import hosts from ~/.ssh/known_hosts with I key

## 2.7.1

- Detail panel tags wrap across multiple lines to fit panel width
- Update badge headline truncates with ellipsis instead of being clipped

## 2.7.0

- Provider metadata uses provider-specific terminology (instance, vm_size, zone, location, image, specs)
- Improved SSH config compatibility: UTF-8 BOM, Host= syntax, ${VAR} in includes, quoted paths, depth 16
- Automatic repair of absorbed group comments and orphaned group headers
- Synced hosts insert adjacent to existing provider group for consistent grouping
- Multi-level undo for host deletion (up to 50 levels)
- Welcome screen with one-time backup of original SSH config to ~/.purple/config.original
- Advisory file locking prevents concurrent write corruption
- New hosts insert before trailing Host * blocks to preserve SSH first-match-wins ordering
- Inline comments preserved when updating directives
- UpCloud boot disk preferred over first storage device for image metadata
- Scaleway pagination via response body instead of X-Total-Count header
- Proxmox QEMU OS type labels match qm.conf(5) manpage
- Atomic writes call fsync before rename and clean up temp files on failure

## 2.6.0

- Added release notes to update flow and GitHub releases
- TUI update badge shows changelog headline from GitHub release body
- Full release notes displayed after `purple update` with markdown stripping
- Release workflow extracts changelog section as GitHub release body
- Added CHANGELOG.md with full release history

## 2.5.0

- Improved Hetzner location migration and GCP zones/IPv6 support
- Added provider metadata (region, plan, os, status) to sync and detail panel
- Added Tailscale to provider badges on landing pages

## 2.4.0

- Added Tailscale provider with local CLI and HTTP API support

## 2.3.0

- Added Linux support for pre-built binaries, installer and self-update

## 2.2.0

- Improved snippet picker: column headers, aligned layout, allow spaces in names, rename raw to terminal

## 2.1.0

- Added in-TUI snippet output, parameterized snippets, snippet search, parallel execution and terminal fallback

## 2.0.4

- Fixed status message leaking into overlay footers

## 2.0.3

- Added file browser sort directions and sync history persistence
- Improved footer and help overlays

## 2.0.2

- Fixed symlink handling in file browser
- Rewritten product messaging for TUI-first positioning

## 2.0.1

- Fixed Include equals-sign parsing, stale multi-select, tag input cursor and sort selection persistence

## 2.0.0

- Added remote file explorer with scp transfer

## 1.28.2

- Fixed 9 bugs found during code review

## 1.28.1

- Redesigned help overlay with two-column layout
- Added getpurple.sh label to host list

## 1.28.0

- Added Azure cloud provider sync

## 1.27.0

- Added GCP Compute Engine cloud provider sync

## 1.26.1

- Dimmed group headers in host list for better visual hierarchy

## 1.26.0

- Redesigned host list with composite host column, purple accent theme and active tunnel visibility

## 1.25.1

- Fixed UI/UX consistency across footers, forms, lists and delete confirmations

## 1.25.0

- Added Scaleway cloud provider sync

## 1.24.0

- Added AWS EC2 cloud provider sync

## 1.23.1

- Fixed keychain migration safety on alias rename

## 1.23.0

- Added activity sparkline, history timestamps and detail scroll
- Improved form clarity and performance

## 1.22.0

- Stream snippet output in real-time instead of buffering

## 1.21.0

- Added full provider metadata and volatile sync
- Improved UI consistency and help attribution

## 1.20.0

- Added provider metadata sync with detail panel display

## 1.19.0

- Added command snippets with multi-host execution

## 1.18.0

- Added ProxyJump picker to host form

## 1.17.0

- Redesigned UI with rounded borders, column layout and compact forms

## 1.16.0

- Added split-pane detail panel with v key toggle

## 1.15.0

- Detect changed host keys and offer to remove old key and reconnect

## 1.14.2

- Fixed ping indicator colored space on selection
- Preserved host selection after SSH

## 1.14.1

- Fixed Left/Right toggle for VerifyTls and AutoSync fields

## 1.14.0

- Added cursor navigation in forms

## 1.13.1

- Fixed tests overwriting ~/.purple/providers
- Preserved selection after host edit

## 1.13.0

- Added SSH password management (keychain, 1Password, Bitwarden, pass, Vault)

## 1.12.0

- Hardened Proxmox provider deserialization
- Added --auto-sync/--no-auto-sync CLI flags

## 1.11.0

- Added Proxmox VE provider

## 1.10.0

- Added per-provider auto_sync toggle

## 1.9.1

- Fixed self-update failing on GitHub redirect

## 1.9.0

- Sort provider list by last sync
- Show footer shortcuts with status on all screens

## 1.8.2

- Fixed redirect following, key_detail height, width-aware truncate, deduplicate providers, validate parse_target

## 1.8.1

- Fixed missing space before update notification in title bar

## 1.8.0

- Added sync history in provider list

## 1.7.0

- Fixed tag selection reset, merged sync tags
- Added --reset-tags flag

## 1.6.0

- Added self-update and TUI version check
- Added getpurple.sh landing page, install script and Bunny edge worker

## 1.5.0

- Added tunnel management

## 1.4.2

- Fixed sync write-failure rollback, cancel-flag replacement, provider config dedup, scoped IPv6 detection

## 1.4.1

- Fixed alias_prefix validation, sync rename stability, tag-edit reload guard, equals-syntax preservation

## 1.4.0

- Added sync cancellation, token env var and atomic_write extraction

## 1.3.0

- Added group-by-provider and form conflict detection
- Hardened parser and improved sync

## 1.2.0

- Added UpCloud provider

## 1.1.1

- Fixed provider CLI config dependency, known_hosts port validation, hex hostname skip, import duplicate counter, token masking UTF-8 panic

## 1.1.0

- Added cloud provider sync (DigitalOcean, Vultr, Linode, Hetzner)

## 1.0.2

- Fixed parser = splitting, shell-quote clipboard, throttle ping-all, quote-aware comments, import reporting, symlink writer, auto-reload guard

## 1.0.1

- Fixed ping dual-stack, event thread hang, known_hosts parser, add-host rollback

## 1.0.0

- Fixed known_hosts wildcard import
- Preserved inline comments on edit

## 0.11.1

- Fixed alias whitespace validation, tab multi-pattern filtering, edit tag rollback, import group headers, included file trailing comments

## 0.11.0

- Added tags to form
- Fixed Include-in-block parsing, inline comments, wildcard validation, search restore and rollback formatting

## 0.10.5

- Fixed broken undo, write-failure rollback, include dir reload, CLI alias validation

## 0.10.4

- Fixed Unicode panic, tab parsing, CRLF preservation, include reload, import errors

## 0.10.3

- Fixed stale edit index, clipboard exit check, search ping guard, known_hosts markers, IPv6 validation

## 0.10.2

- Fixed DNS timeout, undo-on-reload, raw mode guard, deterministic history

## 0.10.1

- Fixed zombie processes, stale delete index, UTF-8 panics, Unicode width, key casing, permission races, panic hook ordering, IPv6 parsing

## 0.10.0

- Columnar layout, key picker comments, simpler keybindings

## 0.9.0

- Added tag picker, search-by-tag and key list improvements

## 0.8.0

- Monochrome theme with purple brand badge

## 0.7.0

- Most recent sort mode and purple accent colors

## 0.6.1

- Reverted Magenta borders, fixed title text readability

## 0.6.0

- Added sort mode persistence and Magenta borders

## 0.5.2

- Fixed brand badge readability across terminal themes

## 0.5.1

- Brand badge title and lowercase branding

## 0.5.0

- Design, UX and accessibility improvements

## 0.4.2

- Fixed round-trip formatting: Include parsing, indentation, blank lines, tags and swap

## 0.4.1

- Fixed SSH connection delay, terminal robustness and atomic write improvements

## 0.4.0

- Added clone, sort, tags, import, inspect, export, undo, auto-reload and connection history

## 0.3.1

- Fixed zombie processes in clipboard detection, search position restore, ProxyJump ping, Linux clipboard support

## 0.3.0

- Added search, ping, grouping, clipboard, quick-add, Include support and shell completions

## 0.2.0

- Added SSH key management (key list, key detail, key picker)

## 0.1.0

- Initial release
