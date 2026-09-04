use crate::core::torrent;
use std::fs;
use std::path::Path;

pub fn run(torrent_path: impl AsRef<Path>) -> Result<(), String> {
    let data = fs::read(torrent_path).map_err(|e| e.to_string())?;
    let torrent = torrent::parse(&data)?;

    let hash_hex: String = torrent.info_hash.iter().map(|b| format!("{:02x}", b)).collect();
    println!("Name:        {}", torrent.name);
    println!("Info Hash:   {}", hash_hex);
    println!("Tracker:     {}", torrent.announce);
    println!("Size:        {} bytes ({:.2} MB)", torrent.length, torrent.length as f64 / (1024.0 * 1024.0));
    println!("Pieces:      {} ({} KB each)", torrent.pieces.len(), torrent.piece_length / 1024);

    Ok(())
}
