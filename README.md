# torr
torr is a git-like bittorrent client, no gui, no unnecessary bloat, just porcelain commands over a real wire protocol implementation

---
## plans/todo


| status | feature                      | info                                                                                                                                                                          |
| :----: | ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|   [/]  | **daemon**                   | bg process for cli to talk over unix socket, so ts status can work instantly without needing to spin up a swarm connection                                                    |

---

## why?

Most bittorrent clients are either bloated and hard to read (ahem, qbittorrent) or abandoned single shotted python files. 
With this project, it implements the peer wire protocol and bencode format from scratch, with no external torrent libraries, with a cli that takes inspiration from git's porcelain/plumbing philosophy.

---

## install

not any time soon, planning on including prebuilt binaries WHEN IT'S READY! :)

---

## file architecture


```
core/
  bencode.rs         bencode encoder/decoder (building this first, w/ no lib)
  torrent.rs         .torrent file parsing and magnet link parsing
  tracker.rs         HTTP + UDP tracker announce/scrape
  peer.rs            peer wire protocol (handshake, msgs, keep-alive)
  piece.rs           piece selection algo (rarest first), block requests
  storage.rs         disk i/o, piece verification w/ sha1, sparse file alloc hopefully
  dht.rs             BEP 5 mainline DHT to get trackerless torrents

cli/
  main.rs            arg parsing and command dispatch
  commands/
    add.rs
    status.rs
    verify.rs
    peers.rs
    status.rs
    remove.rs
```
---

## Contributing

Contributions are welcome! Feel free to open issues for bug reports or feature requests, and submit pull requests.

---

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE) or later.
