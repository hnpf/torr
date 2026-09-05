use crate::cli::progress::format_bytes;
use crate::core::torrent;

pub fn run(source: &str) -> Result<(), String> {
    let torrent = torrent::load_source(source)?;

    let hash_hex: String = torrent.info_hash.iter().map(|b| format!("{:02x}", b)).collect();
    println!("Name:        {}", torrent.name);
    println!("Info Hash:   {}", hash_hex);
    println!("Tracker:     {}", torrent.announce);
    println!("Size:        {} ({} bytes)", format_bytes(torrent.length as u64), torrent.length);
    println!("Pieces:      {} ({} KB each)", torrent.pieces.len(), torrent.piece_length / 1024);

    if torrent.is_multi_file() {
        println!("Files:       {} files", torrent.files.len());
        for f in &torrent.files {
            println!("  - {} ({})", f.path.join("/"), format_bytes(f.length as u64));
        }
    }

    Ok(())
}
