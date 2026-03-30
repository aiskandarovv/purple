## 2.22.0

- Dimmed background behind overlays for better visual depth and focus
- Form divider lines rendered dim to establish clear visual hierarchy (border > labels > dividers)
- Three-tier color support: truecolor (dark grey fg), ANSI 16 (DarkGray), NO_COLOR (DIM modifier only)
- Consistent dimming across open animation, steady-state and close animation

## 2.21.0

- Host patterns support
- Wildcard blocks like `Host *.example.com` and `Host 10.30.0.*` now appear in a dedicated Patterns group at the bottom of the host list
- Add, edit, clone and delete pattern blocks directly from the TUI (`A` to add a new pattern)
- Full SSH glob matching engine: `*`, `?`, `[charset]`, `[!charset]`/`[^charset]`, ranges, `!negation` and multi-pattern (space-separated)
- Detail panel shows inherited directives from matching patterns for each host. Pattern detail view lists all matching hosts
- Patterns included in search, tag filtering (`tag:` fuzzy and `tag=` exact) and tag picker
- Context-aware footer: pattern-specific actions shown when a pattern is selected
- Dynamic column widths in key list and key picker overlay. Columns now adapt to content width instead of using fixed sizes

## 2.20.0

- TransIP provider. 16 cloud providers now supported
- RSA-SHA512 signed JWT authentication or pre-generated Bearer token
- Syncs VPS name, IP, availability zone, product plan, OS and status as provider metadata
- Native TransIP tags synced as provider tags
- Single API call fetches all VPS instances (no pagination needed for typical fleets)

## 2.19.0

- i3D.net provider. 15 cloud providers now supported
- Syncs both dedicated/game servers and FlexMetal on-demand bare metal in a single sync
- Static API key authentication via PRIVATE-TOKEN header
- Dedicated hosts: server category and CPU specs as provider metadata
- FlexMetal servers: location, instance type, OS and status as provider metadata. FlexMetal tags synced as provider tags
- Cursor-based pagination for dedicated hosts, offset-based for FlexMetal

## 2.18.0

- Leaseweb provider. 14 cloud providers now supported
- Syncs both dedicated (bare metal) servers and public cloud instances in a single sync
- Static API key authentication via X-Lsw-Auth header
- Dedicated servers: location, specs (CPU and RAM) and delivery status as provider metadata
- Public cloud instances: region, instance type, OS image and state as provider metadata
- Offset-based pagination for both product lines

## 2.17.0

- OVHcloud Public Cloud provider. 13 cloud providers now supported
- Custom OVH API signature authentication (SHA-1 signing with application key, secret and consumer key)
- EU, CA and US API endpoint picker (defaults to EU)
- Syncs instance name, IP, region, flavor type, OS image and status as provider metadata
- Public IPv4 preferred, falls back to public IPv6 then private IPv4. CIDR suffixes stripped automatically
- HTTPS enforced on all OVH API calls. Project ID validated before any network request

## 2.16.0

- Smooth animations for overlays, detail panel and welcome screen
- Overlay screens open and close with a zoom scale effect (150ms ease-out cubic, 350ms for welcome)
- Detail panel slides open and closes with a smooth width transition. Pressing `v` mid-animation reverses direction seamlessly
- Welcome screen redesigned with block-art logo (line-by-line reveal), typewriter text and larger dialog
- `Ctrl+E` edits the selected host directly from search mode without leaving the search (thanks [@UnspecifiedId](https://www.reddit.com/user/UnspecifiedId/))
- Form borders use purple accent color for host, provider, tunnel, snippet and parameter forms
- Animation loop runs at ~60fps during transitions, returns to blocking event wait when idle

## 2.15.0

- MCP server for AI agents
- `purple mcp` lets Claude Code, Cursor and other AI assistants list hosts, run commands and manage containers via JSON-RPC 2.0
- Five MCP tools: list_hosts, get_host, run_command, list_containers and container_action
- Alias validation against SSH config before any SSH execution (prevents connections to hosts outside your config)
- SSH timeout protection on all MCP tool calls including container operations
- Container ID injection prevention via ASCII allowlist validation

## 2.14.2

- Add fuzz-equivalent property tests for SSH config parser
- Arbitrary Unicode and raw byte inputs now verify idempotency and mutation safety (delete, undo, update, swap, add) in CI

## 2.14.1

- Fix Oracle Cloud group header cleanup. Orphaned headers were not removed when all Oracle hosts disappeared
- Shell-escape alias and hostname in custom askpass commands to prevent metacharacter injection
- Strip control characters from provider config values to prevent INI format corruption
- Terminal cleanup on TUI init failure. Raw mode and alternate screen are now restored if cursor hide or clear fails
- Safe PID conversion in snippet process guard
- Deduplicate percent-encoding and date formatting across provider modules
- Cache clipboard command detection to avoid repeated subprocess spawns
- URL-encode Vultr pagination cursor in query parameters
- Replace tautological connection tests with real integration tests

## 2.14.0

- Oracle Cloud Infrastructure (OCI) provider. Sync Compute instances via OCI REST API with RSA-SHA256 request signing
- Multi-region support with TUI region picker (38 regions across Americas, EMEA and Asia Pacific)
- Recursive compartment sync. Point at a tenancy or compartment OCID and all sub-compartments are included
- Authenticate with your existing OCI config file (~/.oci/config). No extra credentials to manage
- Metadata: region, shape, os and status shown in detail panel. Freeform tags synced as provider tags
- Help overlay layout refined. Narrower width, rebalanced columns and tighter key alignment
- Region picker spacing fix in provider form

## 2.13.0

- Context-sensitive help. Press `?` on any screen to see its shortcuts
- Help works in host list, file browser, snippets, containers, tunnels, providers, SSH keys, tag picker and host detail
- Improved visual hierarchy. Section headers are bold, descriptions are dim, keys are right-aligned
- Host list help reorganized into task-based groups: Navigate, View, Forms, Manage Hosts, Connect and Run, Tools
- Smaller help overlays for sub-screens. No duplicate headers, compact sizing
- Missing shortcuts added across all screens (q/Esc, PgDn/PgUp, j/k)
- Help accessible through confirmation guards and search mode

## 2.12.0

- Container management over SSH. Works with Docker and Podman
- Press `C` on any host to see all containers. Start, stop and restart without leaving purple
- Auto-detects Docker or Podman on the remote host. No agent. No web UI. No extra ports
- Cached container data shown in the detail panel after first fetch

## 2.11.1

- Consistent footer spacing across all overlay screens
- Spacer row between content and footer in all overlay screens for cleaner visual separation
- Startup sort now selects the first host in sorted order instead of the first host in config order
- Rebranded from "SSH config manager" to "terminal SSH client" across all user-facing text
- 1500+ new parser robustness tests covering malformed input, quoting edge cases, Match blocks and mutation sequences (4200+ total)

## 2.11.0

- Soft-delete for disappeared provider hosts
- Hosts that vanish from cloud sync are marked stale instead of silently kept or hard-deleted. Stale hosts appear dimmed in the host list and sort to the bottom
- Purge stale hosts with X. Confirmation dialog shows host names (up to 6) before deletion
- Per-provider purge from the provider list (X key scoped to the selected provider)
- Provider list shows per-provider stale count in red with X key hint in footer
- Detail panel shows "Stale" field with relative timestamp in red
- Virtual "stale" tag for filtering (tag:stale fuzzy, tag=stale exact, appears in tag picker)
- Stale connection warning on Enter, edit, delete, clone, tunnels, snippets and file browser
- Editing a stale host clears the stale marker on save
- Stale hosts automatically un-stale when they reappear in the next provider sync (including stopped VMs with empty IP)
- Partial sync failures suppress stale marking to prevent false positives
- Active tunnels cleaned up on purge (after successful config write)
- CLI: `purple sync` prints "Marked N stale." per provider
- Footer separators between every action (consistency fix across all screens)
- Delete confirmation dialog widened to 52 columns (consistent with other dialogs)
- Detail panel route visualization uses display width instead of byte length for Unicode correctness
- Fix missing blank line when adding a provider host before another provider's group header
- 143 new tests covering stale marking, clearing, purge, sort, filter, config integrity and round-trip fidelity (4111 total)

## 2.10.1

- Sparkline now shows your full connection history
- Fix timestamp retention to match sparkline range. History now keeps 365 days instead of 90 so the auto-scaling sparkline can show up to 1 year of connection activity

## 2.10.0

- Smarter forms, visual routes and a sparkline that fits your data
- ProxyJump chain visualization in detail panel. Shows the full hop route (○ you → ● bastion → ● target) with validation for missing hosts in red
- ProxyJump arrow indicator (→) in host list for hosts using a jump host
- Activity sparkline auto-scales to your data. Ranges from 5 days to 1 year based on connection history
- Sparkline shows dotted baseline (·) for empty periods and a midpoint time label for orientation
- Fewer than 3 connections show a compact text list instead of a sparkline
- Dirty-check on Esc. All four form types now ask "Discard changes?" when you press Esc with unsaved edits
- Auto-submit after picker selection. Pick a key, proxy host or password source and the form submits if ready
- Space bar toggles and cycles. Tunnel type and provider booleans now use Space instead of arrow keys
- Arrow keys are cursor-only in all forms. Left/Right never toggle or cycle values
- HostDetail overlay is no longer a dead end. Press e to edit, T for tunnels, r for snippets
- Signal safety during SSH. Ctrl+C reaches SSH normally but no longer kills purple
- Tunnel processes run in their own process group for clean signal isolation
- Context-aware mode badges in title bar (TAGGING, N SELECTED)
- Search footer shows tag syntax hints (tag: fuzzy, tag= exact) and improved match count (N of M)
- Import confirmation accepts both y and Y
- Consistent footer separators (│) across all screens with shared helper functions
- Help screen updated with Space toggle, detail panel scroll, snippet output navigation and smart-paste hint
- Smart-paste placeholder in Alias field shows user@host:port format
- Edit form title shows the host alias being edited
- 62 new tests covering dirty-check, delete confirmations, navigation, ProxyJump chain resolution and sparkline behavior (3968 total)

## 2.9.0

- Redesigned host list with smarter column layout and provider tag separation
- Provider tags are now stored in a dedicated comment and always mirror the remote. Your own tags are never touched by sync
- Two-cluster column system. Left cluster (name and host) and right cluster (auth, tags, last) separated by a flexible gap
- Add header underline and bold column headers for better scannability
- Add sort indicator next to the active sort column name
- Add selection indicator on the left edge of the selected row
- Show dash for empty auth and last cells instead of blank space
- Show read-only provider tags in the tag edit bar
- Group headers show a horizontal leader line after the label
- Tighter column gaps (2-3 fixed) for a more compact and professional look
- Shorten time labels in the last column (5m instead of 5m ago)
- Sanitize tag values: strip control characters, commas, bidi overrides and enforce max length
- Remove --reset-tags CLI flag (no longer needed)

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
- New hosts insert before trailing Host \* blocks to preserve SSH first-match-wins ordering
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
