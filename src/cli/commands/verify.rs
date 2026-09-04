use crate::core::storage::Storage;
use crate::core::torrent;
use std::path::Path;

pub fn run(source: &str, target_file: impl AsRef<Path>) -> Result<(), String> {
    let torrent = torrent::load_source(source)?;

    println!("Verifying file {:?} against torrent...", target_file.as_ref());
    let mut storage = Storage::open(target_file, torrent.piece_length as u32)?;
    let mut valid = 0;
    for (idx, expected_hash) in torrent.pieces.iter().enumerate() {
        if storage.verify_piece(idx as u32, expected_hash).is_ok() {
            valid += 1;
        }
    }

    let percent = (valid as f64 / torrent.pieces.len() as f64) * 100.0;
    println!("Verified: {}/{} pieces valid ({:.1}%)", valid, torrent.pieces.len(), percent);

    if valid == torrent.pieces.len() {
        println!("All pieces match.");
        Ok(())
    } else {
        Err(format!("verification incomplete: {} pieces corrupt or missing", torrent.pieces.len() - valid))
    }
}
