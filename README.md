<h1 align="center">purple.<br>Terminal SSH client with container management,<br>file transfer, cloud sync and AI agent integration.</h1>

<p align="center">
  <strong>Stop scrolling through your SSH config. Start searching it.</strong><br>
  A TUI that edits <code>~/.ssh/config</code> directly. Free and open-source. Runs on macOS and Linux.
</p>

<p align="center">
  <a href="https://crates.io/crates/purple-ssh"><img src="https://img.shields.io/crates/v/purple-ssh.svg" alt="purple-ssh crate version on crates.io"></a>
  <a href="https://crates.io/crates/purple-ssh"><img src="https://img.shields.io/crates/d/purple-ssh.svg" alt="purple-ssh total downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="MIT License"></a>
  <a href="https://ratatui.rs/"><img src="https://img.shields.io/badge/Built_With-Ratatui-000?logo=ratatui&logoColor=fff" alt="Built With Ratatui"></a>
  <a href="https://getpurple.sh"><img src="https://img.shields.io/badge/Website-getpurple.sh-9333ea.svg" alt="purple website"></a>
</p>

<p align="center"><img src="demo.gif" alt="purple terminal SSH client demo: searching hosts, managing Docker containers, transferring files, connecting via SSH and syncing cloud providers" width="800"></p>
<p align="center"><em>Searching hosts, connecting via SSH, browsing remote files and syncing cloud providers. All from the terminal.</em></p>

## What is purple?

purple is a terminal SSH client and SSH config manager for macOS and Linux. It reads and writes `~/.ssh/config` directly with full round-trip fidelity, preserving your comments, indentation and unknown directives through every edit.

From one terminal interface you can:

- **Search and connect** to any host instantly with fuzzy search and frecency sorting
- **Sync servers** from 16 cloud providers (AWS, Azure, DigitalOcean, GCP, Hetzner, i3D.net, Leaseweb, Linode, OCI, OVHcloud, Proxmox, Scaleway, Tailscale, TransIP, UpCloud and Vultr)
- **Manage containers** over SSH (Docker and Podman, no agent required)
- **Browse remote files** in a split-screen explorer and copy with a keystroke
- **Run command snippets** across one host, a selection or all hosts at once
- **Retrieve SSH passwords** automatically (OS Keychain, 1Password, Bitwarden, pass, HashiCorp Vault or a custom command)
- **Integrate with AI agents** via MCP (Model Context Protocol). Claude Code, Cursor and other AI assistants can query hosts, run commands and manage containers

Written in Rust. Single binary, no daemon, no runtime required. 5000+ tests. MIT license.

## Install

```bash
curl -fsSL getpurple.sh | sh
```

<details>
<summary>Other install methods</summary>

<br>

**Homebrew (macOS)**

```bash
brew install erickochen/purple/purple
```

**Cargo** (crate name: `purple-ssh`)

```bash
cargo install purple-ssh
```

**From source**

```bash
git clone https://github.com/erickochen/purple.git
cd purple && cargo build --release
```

</details>

## Quick start

```bash
purple                   # 1. Launch the TUI
                         # 2. Press a to add a host, or I to import from known_hosts
                         # 3. Press S to configure a cloud provider
                         # 4. Press / to search, Enter to connect
```

Press `?` on any screen for context-sensitive help.

For detailed guides, keybindings and CLI reference see the [wiki](https://github.com/erickochen/purple/wiki). For AI systems: [llms.txt](https://getpurple.sh/llms.txt) contains complete context including architecture and feature details.

---

## Features

### Search and connect

Find any host in under a second, no matter how large your config. Instant fuzzy search across aliases, hostnames, users, tags and providers. Navigate with `j`/`k`, connect with `Enter`. Frecency sorting surfaces your most-used and most-recent hosts.

### Cloud provider sync

Pull servers from **AWS EC2**, **Azure**, **DigitalOcean**, **GCP (Compute Engine)**, **Hetzner**, **i3D.net**, **Leaseweb**, **Linode (Akamai)**, **Oracle Cloud Infrastructure (OCI)**, **OVHcloud**, **Proxmox VE**, **Scaleway**, **Tailscale**, **TransIP**, **UpCloud** and **Vultr** directly into `~/.ssh/config`. Sync adds new hosts, updates changed IPs and optionally removes deleted servers. Provider tags are synced separately from your own tags and always mirror the remote. Your tags are never modified by sync. Press `S` to configure a provider.

### Docker and Podman containers

Press `C` on any host to see all containers over SSH. Start, stop and restart without leaving the terminal. Auto-detects Docker or Podman. No agent required on the remote server, no extra ports. Container data is cached and shown in the detail panel after first fetch.

### Command snippets

Save frequently used commands and run them on one host, a selection of hosts or all visible hosts at once. Press `r` to run a snippet, `Ctrl+Space` to multi-select hosts. Snippets support `{{param}}` placeholders with optional defaults. Values are shell-escaped automatically.

### Remote file explorer

Press `f` on any host to open a split-screen file explorer. Your local filesystem on the left, the remote server on the right. Navigate directories, select files and copy them between machines with `Enter`. Works through ProxyJump chains, password sources and active tunnels.

### SSH tunnel management

Press `T` on any host to manage tunnels (LocalForward, RemoteForward, DynamicForward). Start and stop tunnels from the TUI. Active tunnels run as background processes and are cleaned up on exit.

### SSH password management

Configure a password source per host and purple retrieves passwords automatically on connect via SSH_ASKPASS. Supported sources: **OS Keychain**, **1Password** (`op://`), **Bitwarden** (`bw:`), **pass** (`pass:`), **HashiCorp Vault** (`vault:`) or any custom command.

### AI agent integration (MCP)

purple ships an MCP server that lets Claude Code, Cursor and other AI agents query your SSH hosts, run remote commands and manage containers over JSON-RPC 2.0. Five tools: `list_hosts`, `get_host`, `run_command`, `list_containers` and `container_action`.

**Claude Code.** Add to `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "purple": {
      "command": "purple",
      "args": ["mcp"]
    }
  }
}
```

The client starts `purple mcp` automatically. No manual server process needed. For Cursor, Claude Desktop and other clients see the [MCP Server](https://github.com/erickochen/purple/wiki/MCP-Server) wiki page.

### Additional features

- **Tags** Organize hosts by environment, team or project. Filter with the tag picker (`#`) or `tag:web` in search
- **Bulk import** From hosts files or `~/.ssh/known_hosts`. Press `I` in the TUI or use `purple import` from the CLI
- **SSH key management** Browse keys with metadata (type, bits, fingerprint) and see which hosts use each key
- **Round-trip fidelity** Comments, indentation, unknown directives, CRLF line endings and Include files all preserved
- **TCP ping** Connectivity check per host or all at once
- **Clipboard** Copy the SSH command (`y`) or full config block (`x`)
- **Atomic writes** Temp file, chmod 600, rename. Automatic backups (last 5)
- **Host key reset** Detects changed host keys after a server reinstall and offers to remove the old key and reconnect
- **Auto-reload** Detects external config changes and reloads automatically
- **Detail panel** Split-pane view with connection info, activity sparkline, tags, provider metadata, tunnels, snippets and containers. Toggle with `v`
- **Minimal UI** Monochrome with subtle color for status messages. Works in any terminal, any font. Respects [NO_COLOR](https://no-color.org/)
- **Shell completions** Bash, zsh and fish via `purple --completions`
- **Self-update** `purple update` downloads the latest release and replaces the binary. The TUI shows update notifications

---

## Cloud providers

| Provider | Auth | CLI setup |
|----------|------|---------|
| **AWS EC2** | `~/.aws/credentials` profile or access key pair | `purple provider add aws --profile default --regions us-east-1,eu-west-1` |
| **Azure** | Service principal JSON or Bearer token | `purple provider add azure --token /path/to/sp.json --regions SUBSCRIPTION_ID` |
| **DigitalOcean** | Personal access token | `purple provider add digitalocean --token YOUR_TOKEN` |
| **GCP** | Service account JSON or access token | `purple provider add gcp --token /path/to/sa-key.json --project my-project` |
| **Hetzner** | API token | `purple provider add hetzner --token YOUR_TOKEN` |
| **i3D.net** | API key | `purple provider add i3d --token YOUR_API_KEY` |
| **Leaseweb** | API key | `purple provider add leaseweb --token YOUR_API_KEY` |
| **Linode (Akamai)** | API token | `purple provider add linode --token YOUR_TOKEN` |
| **Oracle Cloud (OCI)** | `~/.oci/config` file | `purple provider add oracle --token ~/.oci/config --compartment OCID` |
| **OVHcloud** | Application key + secret + consumer key | `purple provider add ovh --token AK:AS:CK --project PROJECT_ID` |
| **Proxmox VE** | API token + cluster URL | `purple provider add proxmox --url https://pve:8006 --token TOKEN` |
| **Scaleway** | Secret key | `purple provider add scaleway --token YOUR_TOKEN --regions fr-par-1` |
| **Tailscale** | Local CLI (no token) or API key | `purple provider add tailscale` |
| **TransIP** | RSA private key or Bearer token | `purple provider add transip --token LOGIN:/path/to/key.pem` |
| **UpCloud** | API token | `purple provider add upcloud --token YOUR_TOKEN` |
| **Vultr** | API token | `purple provider add vultr --token YOUR_TOKEN` |

See the [Cloud Providers](https://github.com/erickochen/purple/wiki/Cloud-Providers) wiki page for per-provider setup details, stale host management and auto-sync configuration.

---

## FAQ

**Does purple modify my existing SSH config?**
Your config is only modified when you explicitly add, edit, delete or sync a host. All writes are atomic with automatic backups. Auto-sync runs on startup for providers that have it enabled (toggle per provider, on by default except Proxmox).

**Will purple break my comments or formatting?**
No. purple preserves comments, indentation and unknown directives through every read-write cycle. Consecutive blank lines are collapsed to one.

**Does purple need a daemon or background process?**
No. It's a single binary. Run it, use it, close it.

**Does purple send my SSH config anywhere?**
No. Your config never leaves your machine. Provider sync calls cloud APIs to fetch server lists. The TUI checks GitHub for new releases on startup (cached for 24 hours). No config data is transmitted in either case.

**Can I use purple with Include files?**
Yes. Hosts from Include files are displayed in the TUI but never modified. purple resolves Include directives recursively (up to depth 16) with tilde and glob expansion.

**Can I use purple on Windows?**
purple runs on macOS, Linux and Windows (via WSL). Install inside your WSL distribution with `curl -fsSL getpurple.sh | sh`. Windows Terminal renders the TUI correctly.

**Why is the crate called `purple-ssh`?**
The name `purple` was taken on crates.io. The binary is still called `purple`.

See the [FAQ](https://github.com/erickochen/purple/wiki/FAQ) wiki page for more questions.

---

## Security

Report vulnerabilities through [GitHub Security Advisories](https://github.com/erickochen/purple/security/advisories/new). See [SECURITY.md](SECURITY.md) for scope, disclosure policy and what to include.

## Feedback

Found a bug or have a feature request? [Open an issue on GitHub](https://github.com/erickochen/purple/issues).

## Built with

Written in Rust. 5000+ tests (unit, integration, property-based and HTTP mocking). Zero clippy warnings. No async runtime. Works in any terminal emulator that supports ANSI escape codes including iTerm2, Terminal.app, Alacritty, kitty, WezTerm, Warp and Windows Terminal (via WSL).

<p align="center">
  <a href="LICENSE">MIT License</a>
</p>
