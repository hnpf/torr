use crate::core::torrent;

pub fn run(source: &str) -> Result<(), String> {
    let torrent = torrent::load_source(source)?;

    let hash_hex: String = torrent.info_hash.iter().map(|b| format!("{:02x}", b)).collect();
    println!("Name:        {}", torrent.name);
    println!("Info Hash:   {}", hash_hex);
    println!("Tracker:     {}", torrent.announce);
    println!("Size:        {} bytes ({:.2} MB)", torrent.length, torrent.length as f64 / (1024.0 * 1024.0));
    println!("Pieces:      {} ({} KB each)", torrent.pieces.len(), torrent.piece_length / 1024);

    Ok(())
}
