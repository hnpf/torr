use crate::core::download::generate_peer_id;
use crate::core::{torrent, tracker};
use std::fs;
use std::path::Path;

pub fn run(torrent_path: impl AsRef<Path>) -> Result<(), String> {
    let data = fs::read(torrent_path).map_err(|e| e.to_string())?;
    let torrent = torrent::parse(&data)?;
    let peer_id = generate_peer_id();

    println!("Announcing to tracker: {}", torrent.announce);
    let peers = tracker::announce_addrs(&torrent.announce, &torrent.info_hash, &peer_id, 6881, torrent.length)?;
    println!("Found {} peers:", peers.len());
    for addr in peers {
        println!("  - {}", addr);
    }

    Ok(())
}
