# torr

`torr` is a git-like bittorrent client, has no gui, no unnecessary bloat and distractions, just porcelain commands over a true wire protocol implementation!

---

## install

### quick install (script)

```bash
curl -sSL https://raw.githubusercontent.com/hnpf/torr/main/install.sh | bash
```

Or from source:

```bash
git clone https://github.com/hnpf/torr.git
cd tc
./install.sh
```

*(This builds `torr` in release mode and copies the binary to `~/.local/bin/torr`)*

---

## usage

### download a torrent (from URL, file, or magnet link)

```bash
# download straight to current directory
torr https://torrentsite.com/file.torrent

# download directly from a magnet link
torr 'magnet:?xt=urn:btih:dafc8c076ca2f3ed376eeae7c76a0d6be2415c45&dn=ubuntu.iso'

# specify output directory with -l
torr -l . https://torrentsite.com/file.torrent
torr -l ~/Downloads 'magnet:?xt=urn:btih:...'
torr -l ~/Downloads ubuntu.torrent

# explicit download command
torr download -l . https://torrentsite.com/file.torrent
```

### inspect torrent metadata

```bash
torr status https://torrentsite.com/file.torrent
```

### list swarm peers from tracker

```bash
torr peers ubuntu.torrent
```

### verify downloaded file integrity

```bash
torr verify ubuntu.torrent ubuntu-26.04-desktop-amd64.iso
```

---

### inspect network interfaces and detected VPNs

```bash
torr vpn
```

### download with VPN binding & killswitch protection

```bash
# auto-detect and bind to any active VPN tunnel
torr --vpn https://torrentsite.com/file.torrent
torr -l ~/Downloads --vpn 'magnet:?xt=urn:btih:...'

# bind explicitly to a specific interface
torr --bind proton0 ubuntu.torrent
torr -b wg0 'magnet:?xt=urn:btih:...'
```

---

## commands & options

| command / flag | description |
|---|---|
| `torr <source>` | Download directly from a `.torrent` file, HTTP(S) URL, or magnet link |
| `torr -l <dir>` | Specify target download directory or output path |
| `torr --vpn` | Auto-detect active VPN (WireGuard, OpenVPN, Proton, Mullvad, etc.) and bind |
| `torr -b, --bind <iface>` | Bind peer connections to a specific network interface with killswitch |
| `torr vpn` | List all network interfaces, IPs, and detected VPN connections |
| `torr status <source>` | Show torrent name, size, piece count, tracker URL, and info hash |
| `torr peers <source>` | Announce to tracker and list reachable peer IPs/ports |
| `torr verify <source> <file>` | Validate downloaded piece hashes against the torrent specification |
| `torr open <source>` | Desktop/xdg-open launcher (auto-launches in terminal with menu) |
| `torr -h, --help` | Show usage instructions |

---

## architecture

```
src/
├── core/
│   ├── bencode.rs     Bencode recursive parser and encoder
│   ├── torrent.rs     .torrent parsing, info-hash hashing, URL fetching
│   ├── tracker.rs     HTTP tracker announce and peer decoding (compact & dictionary)
│   ├── peer.rs        Peer wire protocol (handshake, bitfield, messages, unchoke)
│   ├── piece.rs       Block slicing (16 KiB blocks), assembly, SHA1 verification
│   ├── storage.rs     Disk I/O, piece writing, and file verification
│   ├── download.rs    Download orchestrator, peer manager, resume support
│   ├── magnet.rs      BEP 9 / BEP 10 magnet link resolution & metadata exchange
│   └── vpn.rs         VPN auto-detection, low-level socket binding & killswitch
├── cli/
│   ├── commands/      Command handlers (add, status, peers, verify, open, vpn)
│   ├── progress.rs    ANSI terminal download progress bars & speed calculation
│   └── mod.rs
└── main.rs            CLI flags and entrypoint dispatch
```

---

## roadmap

- [x] Full bencode encode/decode engine
- [x] Single-file `.torrent` parsing & SHA1 info-hash calculation
- [x] HTTP tracker announcing (compact IPv4 & dictionary IPv6/v4)
- [x] Peer wire protocol (handshake, bitfields, choke/unchoke negotiation, block requests)
- [x] Concurrent multi-peer swarm worker pool
- [x] Storage manager with sparse piece writes and verification
- [x] End-to-end download coordinator with automatic resume
- [x] Fetch `.torrent` directly from HTTP/HTTPS links
- [x] Multi-file torrent support & cross-file boundary spans
- [x] Magnet link support (`magnet:?xt=urn:btih:...`)
- [x] VPN interface binding with automatic killswitch
- [ ] BEP 5 Mainline DHT for trackerless torrents
- [ ] Background daemon mode with socket IPC

---

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE) or later.

