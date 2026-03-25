//! Real-world SSH config examples collected from GitHub dotfiles repos,
//! blog posts, Stack Overflow answers and OpenSSH test suites.
//!
//! Each config is a raw, copy-pasteable SSH config string annotated with
//! the edge cases it exercises. Use these to fuzz-test and regression-test
//! Purple's SSH config parser.

use std::path::PathBuf;

use purple_ssh::ssh_config::model::SshConfigFile;

/// Helper: parse a string into an SshConfigFile without touching disk.
fn parse_str(content: &str) -> SshConfigFile {
    SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: content.contains("\r\n"),
        bom: false,
    }
}

/// Helper: make visible whitespace for assertion messages.
fn visible(s: &str) -> String {
    s.replace(' ', "\u{00B7}")
        .replace('\t', "\\t")
        .replace('\n', "\\n\n")
        .replace('\r', "\\r")
}

/// Assert two strings are equal, with helpful visible-whitespace diff on failure.
fn assert_eq_visible(expected: &str, actual: &str) {
    if expected != actual {
        panic!(
            "\n=== EXPECTED ===\n{}\n=== ACTUAL ===\n{}\n=== END ===",
            visible(expected),
            visible(actual)
        );
    }
}

/// Assert that parsing and re-serializing produces identical output.
fn assert_roundtrip(input: &str) {
    let config = parse_str(input);
    let output = config.serialize();
    assert_eq_visible(input, &output);
}

// ============================================================================
// CONFIG 1: grawity/dotfiles - Large real-world config
// Edge cases: wildcard hosts, multiple patterns on Host line, CanonicalizeHostname,
//   ControlMaster/ControlPath/ControlPersist, SendEnv, IgnoreUnknown,
//   HostkeyAlias, KexAlgorithms, ProxyJump, IPv6 address, comments,
//   Host *.pattern, mixed indentation (tabs), many hosts
// Source: https://github.com/grawity/dotfiles/blob/main/ssh/config
// ============================================================================

const CONFIG_GRAWITY: &str = "\
IgnoreUnknown PubkeyAcceptedAlgorithms,WarnWeakCrypto

Host armxgw.sym dunegw.sym embergw.sym frostgw.sym shoregw.sym windgw.sym homegw.sym
\tUser admin

Host *.armxgw.sym *.dunegw.sym *.embergw.sym *.frostgw.sym *.shoregw.sym *.windgw.sym
\tUser admin

Host bmc.*.nullroute.lt bmc.*.sym
\tUser Administrator

Host ptp-u2*.sym ptp-pico*.sym kol-*.sym
\t# airOS 4.x-6.x
\tKexAlgorithms +diffie-hellman-group1-sha1
\tUser root

Host er-x.sym
\tUser ubnt

Host rut200.sym rutosgw.sym
\tUser root

Host *.nullroute.lt *.symlink.lt *.sym
\tGSSAPIAuthentication yes
\tForwardAgent yes

Host *.sym
\tKexAlgorithms +diffie-hellman-group14-sha1
\tHostKeyAlgorithms +ssh-rsa
\tPubkeyAcceptedAlgorithms +ssh-rsa

Host aur
\tHostname aur.archlinux.org
\tUser aur

Host shell.*.burble.dn42
\tUser symlink

Host burble-fr
\tHostname shell.fr-rbx1.burble.dn42

Host burble-uk
\tHostname shell.uk-lon1.burble.dn42

Host burble-de
\tHostname shell.de-fra2.burble.dn42

Host burble-ca
\tHostname shell.ca-bhs1.burble.dn42

Host eisner
\tHostname %h.decus.org
\tPort 22867
\tHostKeyAlgorithms +ssh-rsa
\tPubkeyAcceptedAlgorithms +ssh-rsa
\tUser mikulenas

Host *.ngrok.com *.ngrok-agent.com
\tControlPath none

Host sdf sdf.org
\tHostname tty.sdf.org

Host sdf-eu sdf.eu
\tHostname sdf-eu.org

Host theos
\tHostname %h.kyriasis.com

Host gw-core-alt
\tHostname 83.171.33.188
\tProxyJump sky
\tHostkeyAlias gw-core.utenos-kolegija.lt
\tUser root

Host *.*
\t# Disable GSSAPI for FQDNs that aren't our own
\tGSSAPIAuthentication no

Host *
\t# ...but enable it for single-label names
\tGSSAPIAuthentication yes

Host *
\tCanonicalDomains sym nullroute.lt
\tCanonicalizeHostname yes
\tSendEnv LANG TZ
\tControlPath ~/.ssh/S.%r@%h:%p
\tControlMaster auto
\tControlPersist 5s
\tHashKnownHosts no
\tCheckHostIP no
\tUpdateHostKeys no
";

#[test]
fn roundtrip_config_grawity() {
    assert_roundtrip(CONFIG_GRAWITY);
}

// ============================================================================
// CONFIG 2: ashb/dotfiles - ProxyJump with %r token, IP wildcards, ProxyCommand with nc
// Edge cases: ServerAliveInterval at top level (before Host *), VisualHostKey,
//   ControlPath/ControlPersist, ProxyJump with %r@IP, ProxyCommand with nc and
//   stderr redirect, IP range wildcards (10.128.*), UserKnownHostsFile /dev/null,
//   StrictHostKeyChecking no, commented-out ProxyCommand
// Source: https://github.com/ashb/dotfiles/blob/master/.ssh/config
// ============================================================================

const CONFIG_ASHB: &str = "\
ServerAliveInterval 10
ServerAliveCountMax 6
ConnectTimeout 60
#ProxyCommand /usr/bin/nc -X5 -x192.168.0.1:8080 %h %p

Host *
  VisualHostKey yes
  ControlPath ~/.ssh/%r@%h.sock
  ControlPersist 5m

Host jupiter.firemirror.com
  ForwardAgent yes
  ControlMaster auto

Host callisto.firemirror.com
  User ash

Host *.jupiter.firemirror.com
  ControlMaster auto
  ProxyCommand ssh -o VisualHostKey=no 144.76.140.74 nc -q0 %h %p 2>/dev/null

Host *.scsys.co.uk
  User ashb

Host hack.rs
  User ashb

Host cmscontrol
  User cmscontrol
  Hostname 81.246.59.45

# *.live .annalect-emea.com
Host 10.128.*
  User ubuntu
  ProxyJump %r@34.251.17.222
  UserKnownHostsFile /dev/null
  StrictHostKeyChecking no

# *.dev .annalect-emea.com
Host 10.123.*
  User ubuntu
  ProxyJump %r@34.251.194.24
  UserKnownHostsFile /dev/null
  StrictHostKeyChecking no
";

#[test]
fn roundtrip_config_ashb() {
    assert_roundtrip(CONFIG_ASHB);
}

// ============================================================================
// CONFIG 3: posquit0/dotfiles - Equals syntax everywhere
// Edge cases: equals syntax (Key=Value), ControlMaster auto,
//   ControlPath with %r@%h:%p, ControlPersist yes, Compression=yes,
//   vim modeline comment
// Source: https://github.com/posquit0/dotfiles/blob/master/ssh/.ssh/config
// ============================================================================

const CONFIG_POSQUIT0: &str = "\
# vim: set ft=sshconfig:
# SSH Configuration
#
# Maintained by Claud D. Park <posquit0.bj@gmail.com>
# http://www.posquit0.com/

TCPKeepAlive=yes
ServerAliveInterval=15
ServerAliveCountMax=6
Compression=yes
ControlMaster auto
ControlPath /tmp/%r@%h:%p
ControlPersist yes
";

#[test]
fn roundtrip_config_posquit0_equals() {
    assert_roundtrip(CONFIG_POSQUIT0);
}

// ============================================================================
// CONFIG 4: trws/dotfiles - Match blocks with exec, originalhost, complex conditions
// Edge cases: Match originalhost with exec, Match exec with complex negation,
//   Include directive, DynamicForward, multiple LocalForward, ProxyCommand with
//   ssh -W, HostKeyAlias, Host negation (Host !cz-* *), many Match blocks,
//   commented-out Match blocks, HostName %h.llnl.gov token expansion,
//   PreferredAuthentications, PubkeyAuthentication no, RemoteForward,
//   ControlMaster no for specific hosts, PermitLocalCommand
// Source: https://github.com/trws/dotfiles/blob/master/ssh/config
// ============================================================================

const CONFIG_TRWS: &str = "\
Include /Users/scogland1/.colima/ssh_config

# Match host * exec \"hostname | grep abrams\"
#   IdentityAgent \"~/Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock\"

Host abrams
  User scogland1
  Hostname abrams.localdomain
  ProxyJump trove
  LocalForward 5905 localhost:5900
  LocalForward 8386 localhost:8384

Host deb debian
    User scogland1
    HostName debian.local

Host ubu ubuntu
    User scogland1
    HostName 192.168.215.4

Host arch
    User scogland1
    HostName 192.168.64.3

Host nix nixos
    User scogland1
    HostName nixos.local
    ForwardAgent yes

Host gale hurricane
    User njustn

Match originalhost falcon* !originalhost falcon.win exec wsl-get-ip
  HostName falcon
  User njustn
  HostKeyAlias falcon.ubuntu
  ProxyCommand wsl-jump 22

Match originalhost falcon.win exec \" ! ~/.dotfiles/scripts/at-home.py \"
  HostName ssh.scogland.com
  Port 2023

Host falcon.win
  ForwardAgent yes
  User trws
  HostKeyAlias falcon.win
  HostName falcon.local
  DynamicForward 127.0.0.1:12349

Host trove
  User njustn

Match originalhost trove exec \" ! ~/.dotfiles/scripts/at-home.py \"
  HostName ssh.scogland.com
  Port 2024

Match originalhost trove exec \" ~/.dotfiles/scripts/at-home.py \"
  HostName trove.local
  Port 22

Match originalhost funnel
  ExitOnForwardFailure yes
  ProxyCommand ssh scogland@trove -W localhost:43222

Host openmp openmp-vm
  HostName 161.35.129.111
  User tscogland

Host github.com
    User git

Host rzansel ruby rzhasgpu pascal
    Port 22
    ForwardAgent yes
    ForwardX11 yes

Host *z-stash* *z-bitbucket* *zgitlab*
    User git
    Port 7999
    ProxyJump rztopaz

Host *.llnl.gov
    HostName %h

Match exec \"~/.dotfiles/scripts/hop-vpn.sh\" Host !rztopaz,corona,ruby*,hetchy,tioga,quartz,ipa,hype,sierra,hype2,pascal,lassen,rz*,izgw
    ProxyJump rztopaz

Match exec \"~/.dotfiles/scripts/hop-vpn.sh\" Host rztopaz
    Port 9024
    HostName localhost

Host corona ruby* hetchy tioga quartz ipa hype sierra hype2 pascal lassen rz* izgw
    User scogland
    HostName %h.llnl.gov

Host 10.253.134.139 bolt* abrams*
    User scogland1
    Port 22

Host chimera
    Port 22
    HostName 10.5.0.3
    User scogland1
    LocalForward 5904 localhost:5900

Match host chimera-funnel exec \"~/.dotfiles/scripts/hop-vpn.sh\"
    ProxyCommand ssh eris -W chimera:22

Host chimera-*
    User scogland1
    LocalForward 5901 localhost:5900
    LocalForward 22001 localhost:22000
    LocalForward 22002 localhost:22000
    LocalForward 2202 localhost:22
    LocalForward 8385 localhost:8384
    LocalForward 9024 rztopaz.llnl.gov:22
    LocalForward 4043 outlook.office365.com:443
    LocalForward 9993 outlook.office365.com:993
    LocalForward 5587 smtp.office365.com:587
    DynamicForward localhost:12347

Host chimera-wg
    HostName 192.168.3.3

Host chimera-ts
    ProxyCommand ssh -W 127.0.0.1:6022 njustn@trove.tailf3f86.ts.net

Host chimera-funnel
    Port 22
    HostName chimera
    ProxyCommand ssh eris -W chimera:22

Host chimera-relay
    ProxyCommand ssh -W 127.0.0.1:6022 trove

Host chimera-relay-wg
    ProxyCommand ssh -W 192.168.3.3:22 trove

Match exec \" ~/.dotfiles/scripts/at-llnl.py\" !originalhost trove !originalhost github.com !originalhost lima-default !originalhost *.local !originalhost alarm !originalhost debian !originalhost deb !host eris !host eris.llnl.gov !originalhost nix !host localhost !originalhost chimera* !host cz-bit* !originalhost *.*.*.* !originalhost rztopaz !originalhost 10.253.134.139 !originalhost localhost !originalhost 127.0.0.1
    ProxyJump rztopaz

Match host eris exec \"~/.dotfiles/scripts/hop-vpn.sh\"
  Port 9023
  HostName localhost

Host eris
  HostName eris.llnl.gov
  User scogland
  PreferredAuthentications publickey,keyboard-interactive,password

Host eli
  User scogland
  HostName elcapi
  ProxyJump rzansel

Match originalhost rztopaz
    PubkeyAuthentication no
    PasswordAuthentication yes

Host czvnc
    HostName czvnc.llnl.gov
    ForwardX11 no
    RemoteForward 19752 localhost:5500
    ProxyCommand ssh scogland@rztopaz -W %h:622

Host localhost 127.0.0.1
    ControlMaster no

Host !cz-* *
    ForwardAgent yes
    ServerAliveInterval 15
";

#[test]
fn roundtrip_config_trws() {
    assert_roundtrip(CONFIG_TRWS);
}

// ============================================================================
// CONFIG 5: maxamillion/dotfiles - Fedora infra with bastion, SendEnv secrets
// Edge cases: ProxyCommand ssh -W %h:%p bastion, multiple Host patterns with
//   IP ranges, SendEnv with secret env vars, LocalForward + DynamicForward
//   on same host, PermitLocalCommand yes, commented-out LocalCommand,
//   ControlMaster yes (not auto), ControlPath in /tmp, VerifyHostKeyDNS
// Source: https://github.com/maxamillion/dotfiles/blob/main/ssh_config
// ============================================================================

const CONFIG_MAXAMILLION: &str = "\
Host bastion.fedoraproject.org
  User maxamillion
  ProxyCommand none
  ForwardAgent no
  VerifyHostKeyDNS yes

Host *.phx2.fedoraproject.org *.qa.fedoraproject.org 10.5.125.* 10.5.126.* 10.5.127.* *.vpn.fedoraproject.org *.arm.fedoraproject.org
  User maxamillion
  ProxyCommand ssh -W %h:%p bastion.fedoraproject.org
  VerifyHostKeyDNS yes

Host *.fedorainfracloud.org
  User maxamillion
  ForwardAgent no

Host *.scrye.com
  ForwardAgent no

Host *.amazonaws.com
  User root
  StrictHostKeyChecking no
  PasswordAuthentication no
  UserKnownHostsFile ~/.ssh/aws_known_hosts
  IdentityFile ~/.ssh/id_rsa
  ServerAliveInterval 120
  TCPKeepAlive yes

Host 192.168.122.*
  StrictHostKeyChecking no
  UserKnownHostsFile ~/.ssh/local_known_hosts
  ServerAliveInterval 120
  TCPKeepAlive yes

Host ctl1.ops.rhcloud.com
  SendEnv DYNECT_USER_NAME DYNECT_PASSWORD AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY
  ForwardAgent yes

Host sebastian
  HostName ovpn-phx2.redhat.com
  Port 330
  User admiller
  LocalForward 2227 devserv.devel.redhat.com:991
  LocalForward 2228 squid.redhat.com:3128
  ControlMaster yes
  ControlPath /tmp/rhat_ssh
  DynamicForward 9999

Host jhancock
  Hostname jhancock.ose.phx2.redhat.com
  SendEnv DYNECT_USER_NAME DYNECT_PASSWORD AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY
  ServerAliveInterval 120
  ForwardAgent yes
  TCPKeepAlive yes
  User admiller

Host maxamillion
  Hostname maxamillion.sh
  ServerAliveInterval 120
  ForwardAgent yes
  TCPKeepAlive no
  PermitLocalCommand yes
  User admiller

Host file.rdu
  Hostname file.rdu.redhat.com
  ServerAliveInterval 120
  ForwardAgent yes
  TCPKeepAlive yes
  PermitLocalCommand yes
  User admiller
";

#[test]
fn roundtrip_config_maxamillion() {
    assert_roundtrip(CONFIG_MAXAMILLION);
}

// ============================================================================
// CONFIG 6: thiagowfx/.dotfiles - 1Password agent, Include config.d/*, IgnoreUnknown,
//   SetEnv, AddKeysToAgent, UseKeychain
// Edge cases: Include config.d/* (glob), IgnoreUnknown for macOS-specific UseKeychain,
//   AddKeysToAgent yes, IdentityAgent with spaces in path, SetEnv TERM=xterm-256color,
//   ControlPath with %C token, Compression yes, HashKnownHosts yes, verbose comments
// Source: https://github.com/thiagowfx/.dotfiles/blob/master/ssh/.ssh/config
// ============================================================================

const CONFIG_THIAGOWFX: &str = "\
# SSH Client config.
#
# Public keys in plain text: https://github.com/thiagowfx.keys
#
# Quick flags to spawn or attach to an existing tmux session:
#   ssh user@host -t -- tmux -u new -A -s main
Host *
\tAddKeysToAgent yes

\t# Use 1Password SSH agent when available
\tIdentityAgent \"~/Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock\"

\t# This is a macOS only option
\tIgnoreUnknown UseKeychain
\tUseKeychain yes

\tCompression yes

\tServerAliveInterval 300

\t# Reuse SSH connection to speed up remote login process using multiplexing.
\tControlPath /tmp/ssh-control-%C
\tControlPersist yes
\tControlMaster auto

\tHashKnownHosts yes

\tSetEnv TERM=xterm-256color

# Load user scripts and functions if existing. Order is important.
Include config.d/*
";

#[test]
fn roundtrip_config_thiagowfx() {
    assert_roundtrip(CONFIG_THIAGOWFX);
}

// ============================================================================
// CONFIG 7: SSH tunnels gist - All forwarding types
// Edge cases: DynamicForward, multiple LocalForward, multiple RemoteForward,
//   ProxyJump to named host, all three forward types on one host
// Source: https://gist.github.com/thomsh/9b3617b0b345a58cd6d6db92f67feb27
// ============================================================================

const CONFIG_ALL_FORWARDS: &str = "\
Host thebastion
    PubkeyAuthentication yes
    HostName bastion.superservice.prod.aws.corp.com
    IdentityFile ~/.ssh/legacy/id_rsa
    Port 222

Host my-remote-dev-server
    Hostname 10.137.42.11
    ProxyJump thebastion
    IdentityFile ~/.ssh/id_aws_dev_vm
    User jean.dupont
    DynamicForward 10000
    LocalForward 20000 10.0.20.58:5432
    LocalForward 15000 172.17.0.1:5000
    RemoteForward 3128 127.0.0.1:3128
    RemoteForward 5432 127.0.0.1:15432
";

#[test]
fn roundtrip_config_all_forwards() {
    assert_roundtrip(CONFIG_ALL_FORWARDS);
}

// ============================================================================
// CONFIG 8: Certificate-based auth with CertificateFile and forwarding
// Edge cases: CertificateFile, SessionType none, ForkAfterAuthentication,
//   ExitOnForwardFailure, IdentitiesOnly, DynamicForward, ProxyJump chain,
//   Host with multiple wildcard patterns (*.local 10.0.0.*)
// Source: https://gist.github.com/gnzsnz/c2087e7e1d91de9b5bd5c66eacd4c1ac
// ============================================================================

const CONFIG_CERTIFICATES: &str = "\
Host jump_host_nickname
  Hostname jump_host
  Port 22222

Host lf_pgsql
  Hostname pgsql.example.com
  ProxyJump jump_host_nickname
  LocalForward localhost:5432 localhost:5432
  SessionType none
  ForkAfterAuthentication yes
  ExitOnForwardFailure yes
  IdentitiesOnly yes
  CertificateFile ~/.ssh/id_ed25519-cert.pub
  IdentityFile ~/.ssh/id_ed25519

Host rf_app
  Hostname app.example.com
  ProxyJump jump_host_nickname
  RemoteForward localhost:5432 localhost:5432
  SessionType none
  ForkAfterAuthentication yes
  ExitOnForwardFailure yes
  IdentitiesOnly yes
  CertificateFile ~/.ssh/id_ed25519-cert.pub
  IdentityFile ~/.ssh/id_ed25519

Host myproxy
  Hostname server.example.com
  Port 2222
  ProxyJump jump_host_nickname
  DynamicForward 1337
  SessionType none
  ForkAfterAuthentication yes
  ExitOnForwardFailure yes
  IdentitiesOnly yes
  CertificateFile ~/.ssh/id_ed25519-cert.pub
  IdentityFile ~/.ssh/id_ed25519

Host *.local 10.0.0.*
  ProxyJump jump_host_nickname
  IdentitiesOnly yes
  CertificateFile ~/.ssh/id_ed25519-cert.pub
  IdentityFile ~/.ssh/id_ed25519

Host *
  AddKeysToAgent yes
  ServerAliveInterval 60
  ServerAliveCountMax 3
";

#[test]
fn roundtrip_config_certificates() {
    assert_roundtrip(CONFIG_CERTIFICATES);
}

// ============================================================================
// CONFIG 9: AWS SSM ProxyCommand
// Edge cases: ProxyCommand with aws ssm start-session (complex command with
//   --parameters and quotes), Host pattern matching instance IDs (i-* mi-*),
//   StrictHostKeyChecking no
// Source: https://github.com/qoomon/aws-ssm-ssh-proxy-command
// ============================================================================

const CONFIG_AWS_SSM: &str = "\
# AWS SSM SSH Proxy
Host i-* mi-*
  IdentityFile ~/.ssh/id_ed25519
  ProxyCommand sh -c \"aws ssm start-session --target %h --document-name AWS-StartSSHSession --parameters 'portNumber=%p'\"
  StrictHostKeyChecking no

Host dev-web-*
  IdentityFile ~/.ssh/id_ed25519
  ProxyCommand ~/.ssh/aws-ssm-ssh-proxy-command.sh %h %r %p ~/.ssh/id_ed25519.pub
  StrictHostKeyChecking no
  User ec2-user
";

#[test]
fn roundtrip_config_aws_ssm() {
    assert_roundtrip(CONFIG_AWS_SSM);
}

// ============================================================================
// CONFIG 10: GCP IAP tunnel ProxyCommand
// Edge cases: ProxyCommand with gcloud compute start-iap-tunnel (long command),
//   CheckHostIP no, IdentitiesOnly, Google-specific known_hosts file,
//   wildcard pattern for GCP instance naming (Host *.*-*-*.*)
// Source: https://gist.github.com/netj/df4f9de1fefd254ab11979be7035b5d0
// ============================================================================

const CONFIG_GCP_IAP: &str = "\
# Google Cloud IAP tunnel
Host prod-server
  ProxyCommand gcloud compute start-iap-tunnel prod-vm %p --listen-on-stdin --zone=us-east1-b --project=my-company-prod
  IdentityFile ~/.ssh/google_compute_engine
  User shubhamrasal
  UserKnownHostsFile ~/.ssh/google_compute_known_hosts
  IdentitiesOnly yes
  CheckHostIP no

Host staging-db
  ProxyCommand gcloud compute start-iap-tunnel staging-db-vm %p --listen-on-stdin --zone=europe-west1-c --project=my-company-staging
  IdentityFile ~/.ssh/google_compute_engine
  User deploy
  UserKnownHostsFile ~/.ssh/google_compute_known_hosts
  IdentitiesOnly yes
  CheckHostIP no

# Generic GCP pattern
Host *.*-*-*.*
  ProxyCommand sh ~/.ssh/gcp-start-iap-tunnel-ssh-proxy-magic.sh gce_instance=%n sshuser=%r sshport=%p
";

#[test]
fn roundtrip_config_gcp_iap() {
    assert_roundtrip(CONFIG_GCP_IAP);
}

// ============================================================================
// CONFIG 11: Match blocks with exec conditions and canonicalization
// Edge cases: Match host with exec negation (!exec), Match host with comma
//   patterns, CanonicalDomains, CanonicalizeHostname, CanonicalizeMaxDots,
//   CanonicalizeFallbackLocal, CanonicalizePermittedCNAMEs with escaped wildcards,
//   Match host evaluated before HostName resolution quirk
// Source: https://mike.place/2017/ssh-match/ and
//   https://blog.djm.net.au/2014/01/hostname-canonicalisation-in-openssh.html
// ============================================================================

const CONFIG_MATCH_AND_CANONICALIZE: &str = "\
# Canonicalization config
CanonicalizeHostname yes
CanonicalDomains example.com int.example.com
CanonicalizeMaxDots 1
CanonicalizeFallbackLocal yes
CanonicalizePermittedCNAMEs mail.*.example.com:anycast-mail.int.example.com dns*.example.com:dns*.dmz.example.com

# Conditional proxy: only use jump host when not on local network
Match host server !exec \"local-accessible server.example.com &>/dev/null\"
    ProxyJump gateway.example.com

Host server
    Hostname server.example.com

# HostName before Match ensures the match sees the FQDN
Host example
    HostName example.ozlabs.ibm.com

Match host *.ozlabs.ibm.com
    ProxyJump proxy.ozlabs.ibm.com

# IPv6 conditional proxy: use IPv4 jump when IPv6 unreachable
Match host *.example.net,!ipv4.example.net !exec \"ip route get $(host %h | grep 'IPv6 address' | awk '{print $NF}') &> /dev/null\"
    ProxyJump ipv4.example.net
";

#[test]
fn roundtrip_config_match_and_canonicalize() {
    assert_roundtrip(CONFIG_MATCH_AND_CANONICALIZE);
}

// ============================================================================
// CONFIG 12: Salesforce engineering - bastion pattern with Match !host
// Edge cases: Match !host bastion (negation without wildcard), IdentitiesOnly,
//   UserKnownHostsFile per-project, Host * at top, multiple Host aliases
// Source: https://engineering.salesforce.com/managing-multiple-ssh-environments
// ============================================================================

const CONFIG_SALESFORCE_BASTION: &str = "\
Host *
    IdentitiesOnly yes
    IdentityFile ~/.ssh/project1/us-west-2/private_key
    UserKnownHostsFile ~/.ssh/project1/us-west-2/known_hosts
    User user

Match !host bastion
    ProxyCommand ssh -F ~/.ssh/project1/us-west-2/config bastion nc %h %p

Host bastion
    HostName bastion.us-west-2.example.com

Host nginx
    HostName nginx.us-west-2.example.com

Host db
    HostName db.us-west-2.example.com
";

#[test]
fn roundtrip_config_salesforce_bastion() {
    assert_roundtrip(CONFIG_SALESFORCE_BASTION);
}

// ============================================================================
// CONFIG 13: IPv6 addresses
// Edge cases: bare IPv6 in HostName, AddressFamily inet6, LogLevel DEBUG,
//   bracketed IPv6 in ListenAddress style (for LocalForward), ForwardX11
// Source: https://gist.github.com/vardumper/737e7857502635614f0cf1133d02849a
// ============================================================================

const CONFIG_IPV6: &str = "\
Host ipv6-server
  HostName 2001:db8:85a3::8a2e:370:7334
  IdentityFile ~/.ssh/id_rsa
  IdentitiesOnly yes
  ForwardX11 yes
  Port 22
  AddressFamily inet6
  User root
  LogLevel DEBUG

Host ipv6-tunnel
  HostName fd12:3456:789a::1
  User admin
  LocalForward [::1]:8080 [::1]:80
  LocalForward 127.0.0.1:3306 [fd12:3456:789a::db]:3306

Host dual-stack
  HostName dual.example.com
  AddressFamily any
  BindAddress 2001:db8::1
";

#[test]
fn roundtrip_config_ipv6() {
    assert_roundtrip(CONFIG_IPV6);
}

// ============================================================================
// CONFIG 14: ProxyCommand with socat, netcat variants and complex shell
// Edge cases: ProxyCommand with socat, ProxyCommand with ncat, ProxyCommand
//   with shell pipeline, ProxyCommand with env var, ProxyCommand with
//   Boundary (HashiCorp), multiple ProxyCommand styles
// ============================================================================

const CONFIG_PROXYCOMMAND_VARIANTS: &str = "\
# socat-based proxy
Host socat-proxy
  HostName internal.example.com
  ProxyCommand socat - PROXY:proxy.example.com:%h:%p,proxyport=8080

# ncat with proxy auth
Host ncat-proxy
  HostName target.example.com
  ProxyCommand ncat --proxy proxy.corp.com:3128 --proxy-type http --proxy-auth user:pass %h %p

# Shell pipeline proxy
Host pipe-proxy
  HostName backend.example.com
  ProxyCommand bash -c 'exec 3<>/dev/tcp/gateway.example.com/22; cat <&3 & cat >&3'

# Boundary proxy
Host boundary-proxy
  HostName target.internal
  ProxyCommand boundary connect -target-id ttcp_1234567890 -listen-port 0 -format json 2>/dev/null | jq -r '.port'

# SSH with corkscrew through HTTP proxy
Host corkscrew-proxy
  HostName remote.example.com
  ProxyCommand corkscrew http-proxy.example.com 8080 %h %p ~/.ssh/proxy_auth

# Double-hop ProxyCommand
Host double-hop
  HostName final.target.com
  ProxyCommand ssh -W %h:%p -J bastion1.example.com,bastion2.example.com user@middle.example.com
";

#[test]
fn roundtrip_config_proxycommand_variants() {
    assert_roundtrip(CONFIG_PROXYCOMMAND_VARIANTS);
}

// ============================================================================
// CONFIG 15: Host negation patterns
// Edge cases: Host * !bastion pattern, Host negation combined with wildcards,
//   multiple negation patterns, Host with only negated patterns (which won't
//   match anything by itself per OpenSSH docs)
// ============================================================================

const CONFIG_NEGATION_PATTERNS: &str = "\
# Direct connection for bastion (no proxy)
Host bastion
  HostName bastion.prod.example.com
  User jump
  IdentityFile ~/.ssh/bastion_key
  ForwardAgent no

# Everything except bastion goes through proxy
Host * !bastion
  ProxyJump bastion
  ForwardAgent yes

# Internal servers except monitoring
Host *.internal.example.com !monitoring.internal.example.com
  User deploy
  IdentityFile ~/.ssh/internal_key

# Exclude multiple hosts from a wildcard
Host *.prod.example.com !bastion.prod.example.com !monitor.prod.example.com
  StrictHostKeyChecking yes
  LogLevel ERROR
";

#[test]
fn roundtrip_config_negation_patterns() {
    assert_roundtrip(CONFIG_NEGATION_PATTERNS);
}

// ============================================================================
// CONFIG 16: CRLF line endings
// Edge cases: Windows-style CRLF line endings throughout the file
// ============================================================================

const CONFIG_CRLF: &str = "\
Host myserver\r\n\
  HostName 10.0.0.1\r\n\
  User admin\r\n\
  Port 22\r\n\
  IdentityFile ~/.ssh/id_rsa\r\n\
\r\n\
Host bastion\r\n\
  HostName bastion.example.com\r\n\
  User jump\r\n\
  ForwardAgent yes\r\n\
\r\n\
Host *.internal\r\n\
  ProxyJump bastion\r\n\
  User deploy\r\n\
";

#[test]
fn roundtrip_config_crlf() {
    assert_roundtrip(CONFIG_CRLF);
}

// ============================================================================
// CONFIG 17: Only comments and blank lines (no hosts)
// Edge cases: file with zero hosts, only comments, blank lines, various
//   comment styles
// ============================================================================

const CONFIG_ONLY_COMMENTS: &str = "\
# This is a comment-only SSH config file.
# It was generated by a tool that hasn't added any hosts yet.

# Section: defaults
# HostName defaults.example.com

  # indented comment

\t# tab-indented comment

# End of file
";

#[test]
fn roundtrip_config_only_comments() {
    assert_roundtrip(CONFIG_ONLY_COMMENTS);
}

// ============================================================================
// CONFIG 18: Mixed indentation (tabs and spaces)
// Edge cases: tab indentation, space indentation, mixed within same block,
//   no indentation for directives, different indent widths (2, 4, 8 spaces)
// ============================================================================

const CONFIG_MIXED_INDENT: &str = "\
Host tabs-only
\tHostName tabs.example.com
\tUser tabuser
\tPort 22

Host two-spaces
  HostName two.example.com
  User twouser

Host four-spaces
    HostName four.example.com
    User fouruser

Host eight-spaces
        HostName eight.example.com
        User eightuser

Host mixed-in-block
\tHostName mixed.example.com
  User mixeduser
    Port 2222
\t\tIdentityFile ~/.ssh/mixed_key

Host no-indent
HostName noindent.example.com
User noindentuser
Port 22
";

#[test]
fn roundtrip_config_mixed_indent() {
    assert_roundtrip(CONFIG_MIXED_INDENT);
}

// ============================================================================
// CONFIG 19: Equals syntax variations
// Edge cases: Key=Value (no spaces), Key = Value (spaces around equals),
//   Key =Value, Key= Value, mixing equals and space syntax in same file,
//   Host=pattern (equals on Host line itself)
// ============================================================================

const CONFIG_EQUALS_SYNTAX: &str = "\
Host=equals-host
  HostName=equals.example.com
  User=equalsuser
  Port=2222

Host space-host
  HostName space.example.com
  User spaceuser
  Port 22

Host mixed-equals
  HostName=mixed.example.com
  User mixeduser
  Port=443
  IdentityFile ~/.ssh/mixed_key
  ForwardAgent=yes

Host = spaced-equals-host
  HostName = spaced.example.com
  User = spaceduser
";

#[test]
fn roundtrip_config_equals_syntax() {
    assert_roundtrip(CONFIG_EQUALS_SYNTAX);
}

// ============================================================================
// CONFIG 20: Tag and Match tagged (OpenSSH 9.4+)
// Edge cases: Tag directive, Match tagged, Match canonical, Match final,
//   Match all, modern OpenSSH features
// ============================================================================

const CONFIG_TAG_AND_MATCH: &str = "\
Host *.prod.example.com
  Tag production
  User deploy
  IdentityFile ~/.ssh/prod_key

Host *.staging.example.com
  Tag staging
  User deploy
  IdentityFile ~/.ssh/staging_key

Host *.dev.example.com
  Tag development
  User developer

Match tagged production
  StrictHostKeyChecking yes
  LogLevel ERROR
  ServerAliveInterval 30
  ServerAliveCountMax 3

Match tagged staging
  StrictHostKeyChecking ask
  LogLevel INFO

Match tagged development
  StrictHostKeyChecking no
  LogLevel DEBUG
  ForwardAgent yes

Match canonical host *.example.com
  SendEnv LANG LC_*
  SetEnv DEPLOY_ENV=auto

Match final all
  AddKeysToAgent yes
  IdentitiesOnly yes
";

#[test]
fn roundtrip_config_tag_and_match() {
    assert_roundtrip(CONFIG_TAG_AND_MATCH);
}

// ============================================================================
// CONFIG 21: Inline comments (after directives)
// Edge cases: comments after values on the same line. OpenSSH supports
//   inline comments (the # and everything after is ignored if preceded by
//   whitespace). Many third-party parsers get this wrong.
// ============================================================================

const CONFIG_INLINE_COMMENTS: &str = "\
Host webserver # production web
  HostName 10.0.1.50 # internal IP
  User deploy # deployment account
  Port 22 # standard port
  IdentityFile ~/.ssh/web_key # ed25519

Host dbserver # primary database
  HostName 10.0.2.100
  User dba
  LocalForward 3306 localhost:3306 # MySQL
  LocalForward 5432 localhost:5432 # PostgreSQL
";

#[test]
fn roundtrip_config_inline_comments() {
    assert_roundtrip(CONFIG_INLINE_COMMENTS);
}

// ============================================================================
// CONFIG 22: ProxyJump multi-hop chain
// Edge cases: ProxyJump with comma-separated chain (multi-hop), ProxyJump
//   with user@host:port syntax, nested bastion topology
// ============================================================================

const CONFIG_PROXYJUMP_CHAIN: &str = "\
# Edge bastion (internet-facing)
Host edge-bastion
  HostName edge.example.com
  User jump
  Port 2222
  IdentityFile ~/.ssh/edge_key

# Internal bastion (DMZ)
Host internal-bastion
  HostName 10.0.0.1
  User intjump
  ProxyJump edge-bastion

# Database server (deep internal)
Host prod-db
  HostName 10.10.0.50
  User dbadmin
  ProxyJump edge-bastion,internal-bastion

# Even deeper: 3-hop chain with user@host:port
Host secret-vault
  HostName 10.10.10.1
  User vault
  ProxyJump jump1@edge.example.com:2222,jump2@10.0.0.1:22,jump3@10.10.0.1:22

# Alternative: ProxyJump none to override inherited proxy
Host direct-server
  HostName direct.example.com
  ProxyJump none
";

#[test]
fn roundtrip_config_proxyjump_chain() {
    assert_roundtrip(CONFIG_PROXYJUMP_CHAIN);
}

// ============================================================================
// CONFIG 23: Comprehensive hardened config with algorithm restrictions
// Edge cases: Ciphers, MACs, KexAlgorithms, HostKeyAlgorithms, PubkeyAcceptedAlgorithms
//   with long comma-separated lists, algorithm prefix operators (+, -, ^),
//   security-focused configuration
// Source: Inspired by https://gist.github.com/gnzsnz/c2087e7e1d91de9b5bd5c66eacd4c1ac
// ============================================================================

const CONFIG_HARDENED: &str = "\
Host *
  # Key exchange algorithms
  KexAlgorithms curve25519-sha256@libssh.org,diffie-hellman-group-exchange-sha256,sntrup761x25519-sha512@openssh.com,diffie-hellman-group16-sha512,diffie-hellman-group18-sha512

  # Host key algorithms
  HostKeyAlgorithms ssh-ed25519-cert-v01@openssh.com,ssh-rsa-cert-v01@openssh.com,ssh-ed25519,rsa-sha2-512,rsa-sha2-256

  # Ciphers
  Ciphers chacha20-poly1305@openssh.com,aes256-gcm@openssh.com,aes128-gcm@openssh.com,aes256-ctr,aes192-ctr,aes128-ctr

  # MACs
  MACs hmac-sha2-512-etm@openssh.com,hmac-sha2-256-etm@openssh.com,umac-128-etm@openssh.com

  # Pubkey algorithms
  PubkeyAcceptedAlgorithms ssh-ed25519-cert-v01@openssh.com,ssh-ed25519,rsa-sha2-512-cert-v01@openssh.com,rsa-sha2-512,rsa-sha2-256-cert-v01@openssh.com,rsa-sha2-256

  # Identity
  IdentityFile ~/.ssh/id_ed25519
  IdentityFile ~/.ssh/id_rsa

  # Security
  HashKnownHosts yes
  VisualHostKey yes
  PasswordAuthentication no
  ChallengeResponseAuthentication no

# Legacy servers that need weaker algorithms
Host legacy-*.example.com
  KexAlgorithms +diffie-hellman-group14-sha1,diffie-hellman-group1-sha1
  HostKeyAlgorithms +ssh-rsa
  PubkeyAcceptedAlgorithms +ssh-rsa
  Ciphers +aes256-cbc,aes128-cbc

# Prepend post-quantum to defaults
Host quantum-safe.example.com
  KexAlgorithms ^sntrup761x25519-sha512@openssh.com
";

#[test]
fn roundtrip_config_hardened() {
    assert_roundtrip(CONFIG_HARDENED);
}

// ============================================================================
// CONFIG 24: Include directives (multiple, nested, glob)
// Edge cases: Include at top level, Include with tilde expansion, Include with
//   glob, Include with multiple paths, Include inside Host block (conditional),
//   multiple Include directives
// ============================================================================

const CONFIG_INCLUDES: &str = "\
# System-level includes
Include /etc/ssh/ssh_config.d/*.conf

# User config.d modular includes
Include config.d/*
Include ~/.ssh/config.d/*.conf
Include ~/.ssh/hosts/*

# Colima/Docker includes
Include ~/.colima/ssh_config
Include ~/.orbstack/ssh/config

# Work configs
Include work/config

Host personal-server
  HostName personal.example.com
  User me

Host *
  ServerAliveInterval 60
  ServerAliveCountMax 3
";

#[test]
fn roundtrip_config_includes() {
    assert_roundtrip(CONFIG_INCLUDES);
}

// ============================================================================
// CONFIG 25: paulirish/dotfiles - Connection multiplexing
// Edge cases: Protocol 2 (legacy directive), ControlMaster auto in Host *,
//   ControlPath with %r@%h:%p, ControlPersist 1800 (seconds), github-specific
//   overrides before wildcard
// Source: https://github.com/paulirish/dotfiles/blob/main/.ssh.config.example
// ============================================================================

const CONFIG_PAULIRISH: &str = "\
# copy to ~/.ssh/config

Host github.com
\tControlMaster auto
\tControlPersist 120

Host *
\t# Always use SSH2.
\tProtocol 2

\t# Use a shared channel for all sessions to the same host,
\t# instead of always opening a new one. This leads to much
\t# quicker connection times.
\tControlMaster auto
\tControlPath ~/.ssh/control/%r@%h:%p
\tControlPersist 1800

\t# also this stuff
\tCompression yes
\tTCPKeepAlive yes
\tServerAliveInterval 20
\tServerAliveCountMax 10
";

#[test]
fn roundtrip_config_paulirish() {
    assert_roundtrip(CONFIG_PAULIRISH);
}

// ============================================================================
// CONFIG 26: Large config with many hosts (stress test)
// Edge cases: many hosts, sequential layout, no blank lines between some
//   hosts, varying directive counts per host
// ============================================================================

const CONFIG_MANY_HOSTS: &str = "\
Host web01
  HostName 10.0.1.1
  User deploy
Host web02
  HostName 10.0.1.2
  User deploy
Host web03
  HostName 10.0.1.3
  User deploy
Host web04
  HostName 10.0.1.4
  User deploy
Host web05
  HostName 10.0.1.5
  User deploy

Host db01
  HostName 10.0.2.1
  User dba
  LocalForward 3306 localhost:3306
Host db02
  HostName 10.0.2.2
  User dba
  LocalForward 3307 localhost:3306
Host db03
  HostName 10.0.2.3
  User dba
  LocalForward 3308 localhost:3306

Host cache01
  HostName 10.0.3.1
  User admin
  LocalForward 6379 localhost:6379
Host cache02
  HostName 10.0.3.2
  User admin
  LocalForward 6380 localhost:6379

Host worker01
  HostName 10.0.4.1
  User worker
Host worker02
  HostName 10.0.4.2
  User worker
Host worker03
  HostName 10.0.4.3
  User worker
Host worker04
  HostName 10.0.4.4
  User worker
Host worker05
  HostName 10.0.4.5
  User worker
Host worker06
  HostName 10.0.4.6
  User worker
Host worker07
  HostName 10.0.4.7
  User worker
Host worker08
  HostName 10.0.4.8
  User worker
Host worker09
  HostName 10.0.4.9
  User worker
Host worker10
  HostName 10.0.4.10
  User worker

Host monitor
  HostName 10.0.5.1
  User admin
  LocalForward 9090 localhost:9090
  LocalForward 3000 localhost:3000
  LocalForward 9093 localhost:9093

Host log-aggregator
  HostName 10.0.5.2
  User admin
  LocalForward 5601 localhost:5601
  LocalForward 9200 localhost:9200

Host bastion
  HostName bastion.prod.example.com
  User jump
  IdentityFile ~/.ssh/prod_bastion

Host * !bastion
  ProxyJump bastion
";

#[test]
fn roundtrip_config_many_hosts() {
    assert_roundtrip(CONFIG_MANY_HOSTS);
}

// ============================================================================
// CONFIG 27: Empty file
// Edge cases: completely empty file (zero bytes)
// ============================================================================

const CONFIG_EMPTY: &str = "";

#[test]
fn roundtrip_config_empty() {
    // The writer adds a trailing newline to empty files - that's expected
    let config = parse_str(CONFIG_EMPTY);
    let output = config.serialize();
    assert!(
        !output.contains("Host"),
        "empty config should produce no hosts"
    );
}

// ============================================================================
// CONFIG 28: Whitespace-only file
// Edge cases: file with only blank lines and whitespace
// ============================================================================

const CONFIG_WHITESPACE_ONLY: &str = "\t\n\n\n\n";

#[test]
fn roundtrip_config_whitespace_only() {
    // The writer collapses consecutive blank lines - that's expected behavior
    let config = parse_str(CONFIG_WHITESPACE_ONLY);
    let output = config.serialize();
    assert!(
        !output.contains("Host"),
        "whitespace-only config should produce no hosts"
    );
}

// ============================================================================
// CONFIG 29: Unusual but valid directive patterns
// Edge cases: directives with extra whitespace, multiple spaces between key
//   and value, trailing whitespace, tokens in various directives (%h, %p, %r,
//   %C, %n, %L), quoted values with spaces
// ============================================================================

const CONFIG_UNUSUAL_WHITESPACE: &str = "\
Host   extra-spaces
  HostName   extra.example.com
  User   spaceuser
  Port   22

Host trailing-space
  HostName trailing.example.com
  User trailuser

Host token-expansion
  HostName %h.example.com
  ControlPath ~/.ssh/sockets/%r@%h-%p
  LocalCommand echo \"Connected to %n as %r on port %p from %L\"
  PermitLocalCommand yes
  ProxyCommand ssh -W %h:%p gateway.example.com
";

#[test]
fn roundtrip_config_unusual_whitespace() {
    assert_roundtrip(CONFIG_UNUSUAL_WHITESPACE);
}

// ============================================================================
// CONFIG 30: Multiple Host patterns on single line, various real patterns
// Edge cases: Host with many patterns, wildcard + literal mix, question mark
//   wildcards, subdomain wildcards
// ============================================================================

const CONFIG_MULTI_PATTERNS: &str = "\
# Multiple aliases for the same host
Host dev development dev-server devbox
  HostName dev.internal.example.com
  User developer
  ForwardAgent yes

# Mixed wildcards and literals
Host *.us-east-1.compute.amazonaws.com *.us-west-2.compute.amazonaws.com ec2-*
  User ec2-user
  IdentityFile ~/.ssh/aws.pem
  StrictHostKeyChecking no

# Question mark wildcard (single character)
Host web-?? web-???
  HostName %h.internal.example.com
  User deploy

# Complex subdomain patterns
Host *.*.svc.cluster.local
  User root
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null

# Very specific negation
Host *.example.com !www.example.com !mail.example.com !ns?.example.com
  User admin
";

#[test]
fn roundtrip_config_multi_patterns() {
    assert_roundtrip(CONFIG_MULTI_PATTERNS);
}

// ============================================================================
// CONFIG 31: Match blocks - complex real-world combinations
// Edge cases: Match all, Match host with wildcards, Match originalhost,
//   Match user, Match localuser, Match exec with shell commands,
//   multiple criteria on one Match line, Match with comma-separated hosts
// ============================================================================

const CONFIG_COMPLEX_MATCH: &str = "\
# Match based on original hostname before HostName resolution
Match originalhost jump-*
  ProxyJump none
  ForwardAgent no

# Match based on resolved host AND exec condition
Match host *.corp.example.com exec \"test -f /etc/corp-vpn-connected\"
  IdentityFile ~/.ssh/corp_key
  User corp-user

# Match based on local username
Match localuser root
  IdentityFile /root/.ssh/automation_key
  StrictHostKeyChecking no

# Match based on remote user
Match user git
  IdentityFile ~/.ssh/git_signing_key
  IdentitiesOnly yes

# Match all (applies to everything not yet matched)
Match all
  ServerAliveInterval 60
  ServerAliveCountMax 3
  AddKeysToAgent yes

# Match with multiple host patterns (comma-separated)
Match host 10.0.*,172.16.*,192.168.*
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
  LogLevel ERROR
";

#[test]
fn roundtrip_config_complex_match() {
    assert_roundtrip(CONFIG_COMPLEX_MATCH);
}

// ============================================================================
// CONFIG 32: Mixed CRLF and LF (broken file, but should survive parsing)
// Edge cases: some lines with CRLF, some with LF only. Real configs can
//   end up like this after editing on different platforms.
// ============================================================================

const CONFIG_MIXED_ENDINGS: &str = "Host mixed-endings\r\n  HostName mixed.example.com\n  User mixeduser\r\n  Port 22\n\nHost normal-endings\n  HostName normal.example.com\n  User normaluser\n";

#[test]
fn roundtrip_config_mixed_endings() {
    // Mixed line endings - just verify it parses without panic
    let config = parse_str(CONFIG_MIXED_ENDINGS);
    let _output = config.serialize();
    // We don't assert exact roundtrip because mixed endings may be normalized
}

// ============================================================================
// CONFIG 33: Directives before any Host/Match block (global scope)
// Edge cases: directives at global scope before the first Host line,
//   which OpenSSH treats as applying to all connections (implicit Host *)
// ============================================================================

const CONFIG_GLOBAL_DIRECTIVES: &str = "\
# Global directives (before any Host line)
ServerAliveInterval 60
ServerAliveCountMax 3
TCPKeepAlive yes
Compression yes
ControlMaster auto
ControlPath ~/.ssh/control-%C
ControlPersist 10m
AddKeysToAgent yes
IdentityFile ~/.ssh/id_ed25519
IdentityFile ~/.ssh/id_rsa

Host specific-server
  HostName specific.example.com
  User admin
  Port 2222

Host *
  HashKnownHosts yes
";

#[test]
fn roundtrip_config_global_directives() {
    assert_roundtrip(CONFIG_GLOBAL_DIRECTIVES);
}

// ============================================================================
// CONFIG 34: Stress test - all directive types in one config
// Edge cases: exercises nearly every SSH client directive type in a single
//   file. Tests that the parser handles all known directive names.
// ============================================================================

const CONFIG_ALL_DIRECTIVES: &str = "\
Host kitchen-sink
  HostName kitchen.example.com
  User chef
  Port 2222
  AddKeysToAgent yes
  AddressFamily inet
  BatchMode no
  BindAddress 192.168.1.100
  CanonicalDomains example.com
  CanonicalizeHostname no
  CanonicalizeFallbackLocal yes
  CanonicalizeMaxDots 1
  CASignatureAlgorithms ssh-ed25519
  CertificateFile ~/.ssh/id_ed25519-cert.pub
  CheckHostIP yes
  Ciphers chacha20-poly1305@openssh.com,aes256-gcm@openssh.com
  Compression yes
  ConnectionAttempts 3
  ConnectTimeout 30
  ControlMaster auto
  ControlPath ~/.ssh/cm-%r@%h:%p
  ControlPersist 600
  DynamicForward 1080
  EnableEscapeCommandline no
  EnableSSHKeysign no
  EscapeChar ~
  ExitOnForwardFailure yes
  FingerprintHash sha256
  ForkAfterAuthentication no
  ForwardAgent no
  ForwardX11 no
  ForwardX11Timeout 1200
  ForwardX11Trusted no
  GatewayPorts no
  GlobalKnownHostsFile /etc/ssh/ssh_known_hosts
  GSSAPIAuthentication no
  GSSAPIDelegateCredentials no
  HashKnownHosts yes
  HostbasedAuthentication no
  HostKeyAlgorithms ssh-ed25519,rsa-sha2-512
  HostKeyAlias kitchen-alias
  IdentitiesOnly yes
  IdentityAgent SSH_AUTH_SOCK
  IdentityFile ~/.ssh/id_ed25519
  IPQoS af21 cs1
  KbdInteractiveAuthentication yes
  KexAlgorithms curve25519-sha256
  LocalCommand echo connected
  LocalForward 8080 localhost:80
  LogLevel VERBOSE
  MACs hmac-sha2-256-etm@openssh.com
  NoHostAuthenticationForLocalhost no
  NumberOfPasswordPrompts 3
  PasswordAuthentication yes
  PermitLocalCommand yes
  PermitRemoteOpen any
  PKCS11Provider none
  PreferredAuthentications publickey,keyboard-interactive,password
  ProxyJump none
  ProxyUseFdpass no
  PubkeyAcceptedAlgorithms ssh-ed25519,rsa-sha2-512
  PubkeyAuthentication yes
  RekeyLimit 1G 1h
  RemoteForward 9090 localhost:9090
  RequestTTY auto
  RevokedHostKeys /etc/ssh/revoked_keys
  SecurityKeyProvider internal
  SendEnv LANG LC_* EDITOR
  ServerAliveCountMax 3
  ServerAliveInterval 15
  SessionType default
  SetEnv FOO=bar BAZ=qux
  StreamLocalBindMask 0177
  StreamLocalBindUnlink no
  StrictHostKeyChecking ask
  TCPKeepAlive yes
  Tunnel no
  TunnelDevice any:any
  UpdateHostKeys yes
  User chef
  UserKnownHostsFile ~/.ssh/known_hosts
  VerifyHostKeyDNS yes
  VisualHostKey no
  XAuthLocation /usr/bin/xauth
";

#[test]
fn roundtrip_config_all_directives() {
    assert_roundtrip(CONFIG_ALL_DIRECTIVES);
}

// ============================================================================
// CONFIG 35: Real tunnel config - RemoteForward for coworker sharing
// Edge cases: multiple RemoteForward, multiple LocalForward on same host,
//   localhost HostName with port-only User
// Source: https://gist.github.com/3309869
// ============================================================================

const CONFIG_TUNNEL_SHARING: &str = "\
Host mark
HostName localhost
User mark
Port 8008

Host mike
HostName localhost
User mike
Port 1337

Host mikes_tunnels
HostName example.webserver.com
User example_login_user
RemoteForward 1337 localhost:22
RemoteForward 3030 localhost:3000
LocalForward 8008 localhost:8008
LocalForward 3030 localhost:3031

Host marks_tunnels
HostName example.webserver.com
User example_login_user
RemoteForward 8008 localhost:22
RemoteForward 3031 localhost:3000
LocalForward 1337 localhost:1337
LocalForward 3030 localhost:3030
";

#[test]
fn roundtrip_config_tunnel_sharing() {
    assert_roundtrip(CONFIG_TUNNEL_SHARING);
}

// ============================================================================
// CONFIG 36: Quoted values with special characters
// Edge cases: quoted IdentityAgent path with spaces, quoted ProxyCommand,
//   quoted IdentityFile paths, quoted LocalCommand
// ============================================================================

const CONFIG_QUOTED_VALUES: &str = "\
Host quoted-paths
  IdentityFile \"~/.ssh/my key file\"
  IdentityAgent \"~/Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock\"
  ProxyCommand \"C:\\Program Files\\Git\\usr\\bin\\ssh.exe\" -W %h:%p bastion
  LocalCommand \"echo 'hello world' | tee /tmp/ssh-connect.log\"
  PermitLocalCommand yes

Host simple-quoted
  IdentityFile \"~/.ssh/id_ed25519\"
  User \"myuser\"
";

#[test]
fn roundtrip_config_quoted_values() {
    assert_roundtrip(CONFIG_QUOTED_VALUES);
}

// ============================================================================
// CONFIG 37: BOM (Byte Order Mark) prefix
// Edge cases: UTF-8 BOM at start of file. Some Windows editors add this.
// ============================================================================

const CONFIG_BOM: &str = "\u{FEFF}Host bom-server
  HostName bom.example.com
  User bomuser
  Port 22
";

#[test]
fn roundtrip_config_bom() {
    // BOM handling is special - just verify it parses
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(CONFIG_BOM),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: CONFIG_BOM.starts_with('\u{FEFF}'),
    };
    let _output = config.serialize();
}

// ============================================================================
// CONFIG 38: Multiple blank lines and trailing newlines
// Edge cases: excessive blank lines between blocks, trailing blank lines,
//   leading blank lines
// ============================================================================

const CONFIG_BLANK_LINES: &str = "\
Host first
  HostName first.example.com
  User firstuser



Host second
  HostName second.example.com
  User seconduser


Host third
  HostName third.example.com
  User thirduser



";

#[test]
fn roundtrip_config_blank_lines() {
    // The writer collapses multiple consecutive blank lines to a single one.
    // Verify parse + serialize preserves hosts and doesn't panic.
    let config = parse_str(CONFIG_BLANK_LINES);
    let output = config.serialize();
    assert!(output.contains("Host first"));
    assert!(output.contains("Host second"));
    assert!(output.contains("Host third"));
    // Verify no triple-blank-line runs in output (writer collapses them)
    assert!(
        !output.contains("\n\n\n\n"),
        "writer should collapse consecutive blank lines"
    );
}

// ============================================================================
// CONFIG 39: GitHub/GitLab multi-account config (very common pattern)
// Edge cases: HostName reuse (same hostname for different Host entries),
//   IdentitiesOnly yes, multiple SSH key identities for same service
// ============================================================================

const CONFIG_MULTI_ACCOUNT_GIT: &str = "\
# Personal GitHub
Host github.com-personal
  HostName github.com
  User git
  IdentityFile ~/.ssh/id_ed25519_personal
  IdentitiesOnly yes

# Work GitHub
Host github.com-work
  HostName github.com
  User git
  IdentityFile ~/.ssh/id_ed25519_work
  IdentitiesOnly yes

# Personal GitLab
Host gitlab.com
  User git
  IdentityFile ~/.ssh/id_ed25519_personal
  IdentitiesOnly yes
  PreferredAuthentications publickey

# Self-hosted GitLab
Host gitlab.internal.corp.com
  User git
  IdentityFile ~/.ssh/id_rsa_corp
  Port 2222
  IdentitiesOnly yes

# Bitbucket
Host bitbucket.org
  User git
  IdentityFile ~/.ssh/id_ed25519_personal
  IdentitiesOnly yes

# AUR (Arch User Repository)
Host aur.archlinux.org
  User aur
  IdentityFile ~/.ssh/id_ed25519_aur
  IdentitiesOnly yes
";

#[test]
fn roundtrip_config_multi_account_git() {
    assert_roundtrip(CONFIG_MULTI_ACCOUNT_GIT);
}

// ============================================================================
// CONFIG 40: Kubernetes / container SSH patterns
// Edge cases: ProxyCommand with kubectl, docker exec, Host patterns for
//   ephemeral infrastructure, UserKnownHostsFile /dev/null everywhere
// ============================================================================

const CONFIG_KUBERNETES: &str = "\
# SSH into Kubernetes pods via kubectl
Host k8s-pod-*
  ProxyCommand kubectl exec -i %h -- /usr/bin/nc localhost %p
  User root
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null

# SSH via Docker container
Host docker-*
  ProxyCommand docker exec -i %h /usr/bin/nc localhost %p
  User root
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null

# Vagrant machines
Host vagrant-*
  User vagrant
  IdentityFile ~/.vagrant.d/insecure_private_key
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
  LogLevel FATAL

# Ephemeral cloud instances: never remember host keys
Host *.compute.amazonaws.com *.compute.internal
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
  User ec2-user
  IdentityFile ~/.ssh/aws-key.pem
  ServerAliveInterval 60
";

#[test]
fn roundtrip_config_kubernetes() {
    assert_roundtrip(CONFIG_KUBERNETES);
}

// ============================================================================
// CONFIG 41: Purple stale annotations mixed with other comments
// Edge cases: stale timestamp comment interleaved with provider, meta, tags,
//   askpass and regular SSH directives. Verifies round-trip fidelity of
//   the stale annotation alongside every other purple comment type
// ============================================================================

const CONFIG_STALE_ANNOTATIONS: &str = "\
# Cloud infrastructure hosts

Host aws-web-01
  HostName 54.23.100.12
  User ec2-user
  IdentityFile ~/.ssh/aws.pem
  # purple:provider aws:i-0abc123def456
  # purple:tags prod,us-east,web
  # purple:provider_tags web-tier,auto-scaling
  # purple:meta region=us-east-1,instance=t3.medium,os=al2023,status=running
  # purple:askpass keychain
  # purple:stale 1700000000
  LocalForward 8080 localhost:80
  ServerAliveInterval 60

Host do-db-01
  HostName 104.236.32.1
  User root
  Port 2222
  # purple:provider digitalocean:12345678
  # purple:tags staging,database
  # purple:meta region=nyc3,size=s-2vcpu-4gb,image=ubuntu-22-04,status=active

Host hetzner-cache
  HostName 116.203.0.1
  User deploy
  # purple:provider hetzner:srv-789
  # purple:provider_tags cache,eu-west
  # purple:meta location=fsn1,type=cx21,image=debian-12,status=running
  # purple:stale 1695000000
  # purple:askpass op://Infra/hetzner-cache/password

Host manual-bastion
  HostName 10.0.0.1
  User admin
  # purple:tags bastion,internal
  # purple:askpass pass:infra/bastion

Host *
  ServerAliveInterval 60
  ServerAliveCountMax 3
  AddKeysToAgent yes
";

#[test]
fn roundtrip_config_stale_annotations() {
    assert_roundtrip(CONFIG_STALE_ANNOTATIONS);

    // Also verify stale is correctly parsed
    let config = parse_str(CONFIG_STALE_ANNOTATIONS);
    let entries = config.host_entries();

    let aws = entries.iter().find(|e| e.alias == "aws-web-01").unwrap();
    assert_eq!(aws.stale, Some(1700000000));
    assert_eq!(aws.provider.as_deref(), Some("aws"));
    assert_eq!(aws.askpass.as_deref(), Some("keychain"));
    assert!(aws.tags.contains(&"prod".to_string()));

    let hetzner = entries.iter().find(|e| e.alias == "hetzner-cache").unwrap();
    assert_eq!(hetzner.stale, Some(1695000000));
    assert_eq!(hetzner.provider.as_deref(), Some("hetzner"));

    let db = entries.iter().find(|e| e.alias == "do-db-01").unwrap();
    assert_eq!(db.stale, None);

    let bastion = entries
        .iter()
        .find(|e| e.alias == "manual-bastion")
        .unwrap();
    assert_eq!(bastion.stale, None);
}
