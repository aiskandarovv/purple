# purple

**A terminal cockpit for your servers.** Search, connect, transfer files, manage containers and run commands across hosts. All keyboard-driven. Free and open source.

[![crates.io](https://img.shields.io/crates/v/purple-ssh?color=b44aff&labelColor=0a0a14)](https://crates.io/crates/purple-ssh)
[![downloads](https://img.shields.io/crates/d/purple-ssh?color=b44aff&labelColor=0a0a14)](https://crates.io/crates/purple-ssh)
[![MIT](https://img.shields.io/badge/License-MIT-b44aff?labelColor=0a0a14)](LICENSE)
[![Built With Ratatui](https://img.shields.io/badge/Built_With-Ratatui-b44aff?labelColor=0a0a14&logo=ratatui&logoColor=fff)](https://ratatui.rs/)
[![Website](https://img.shields.io/badge/Website-getpurple.sh-00f0ff?labelColor=0a0a14)](https://getpurple.sh)

![purple terminal SSH client demo](demo.gif)

## Why I built this

My SSH config was fine. Proper aliases, ProxyJump chains, organized by provider. Not the problem.

The problem was everything around it. Need to check a container? `ssh host docker ps`. Copy a file? `scp` with the right flags. Run the same command on ten hosts? Write a loop or boot up Ansible for a one-liner. Spin up a VM on Hetzner? Open the console, grab the IP, edit config, save. Someone asks which box runs what? Good luck.

I wanted one place for all of that. So I built it.

## Install

```
curl -fsSL getpurple.sh | sh
```

<details>
<summary>brew, cargo or from source</summary>

```
brew install erickochen/purple/purple
```
```
cargo install purple-ssh
```
```
git clone https://github.com/erickochen/purple.git
cd purple && cargo build --release
```
</details>

Run `purple`. Press `?` on any screen for help. That's it.

## What you get

🔍 **Instant fuzzy search.** Names, IPs, tags, users. Frecency sorting puts your most-used hosts on top. Works the same with 5 hosts or 500.

☁️ **Cloud sync for 16 providers.** AWS, Azure, GCP, Hetzner, DigitalOcean, Proxmox VE, Tailscale and [9 more](https://github.com/erickochen/purple/wiki/Cloud-Providers). VMs show up, IPs update, deleted ones disappear. Your SSH config stays the source of truth.

🐳 **Container management over SSH.** Docker and Podman. Start, stop, restart. No agent on the remote, no extra ports. Just SSH.

📂 **Visual file transfer.** Split-pane explorer, local on the left, remote on the right. Works through ProxyJump chains and tunnels.

⚡ **Multi-host command execution.** Save snippets, select hosts, run. Output per host in a scrollable view. `{{param}}` placeholders with defaults.

🔑 **Automatic password retrieval.** OS Keychain, 1Password, Bitwarden, pass, HashiCorp Vault or any custom command. Pulled on connect via SSH_ASKPASS. No clipboard, no typing.

🤖 **MCP server for AI agents.** Claude Code, Cursor and others can list your hosts, run commands and manage containers. Three lines in your config.

## How it works

purple reads `~/.ssh/config` directly. No database, no daemon, no account. Comments, indentation, Include files, unknown directives. All preserved.

Written in Rust. Single binary. 5000+ tests. MIT license.

## Links

📖 [Wiki](https://github.com/erickochen/purple/wiki) · ☁️ [Cloud Providers](https://github.com/erickochen/purple/wiki/Cloud-Providers) · 🤖 [MCP Server](https://github.com/erickochen/purple/wiki/MCP-Server) · ❓ [FAQ](https://github.com/erickochen/purple/wiki/FAQ) · 🔒 [Security](SECURITY.md) · 🧠 [llms.txt](https://getpurple.sh/llms.txt)

## Feedback

Bug or feature request? [Open an issue](https://github.com/erickochen/purple/issues).
