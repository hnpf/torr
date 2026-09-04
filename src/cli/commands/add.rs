use crate::core::download::DownloadSession;
use crate::core::torrent;
use std::fs;
use std::io::Write;
use std::path::Path;

pub fn run(torrent_path: impl AsRef<Path>, output_path: Option<impl AsRef<Path>>) -> Result<(), String> {
    let data = fs::read(&torrent_path).map_err(|e| e.to_string())?;
    let torrent = torrent::parse(&data)?;

    let dest = match output_path {
        Some(p) => p.as_ref().to_path_buf(),
        None => std::path::PathBuf::from(&torrent.name),
    };

    println!("Downloading {:?} -> {:?}", torrent.name, dest);
    let mut session = DownloadSession::new(torrent, &dest)?;

    let total = session.torrent.pieces.len();
    println!("Starting download ({} pieces, total {} bytes)...", total, session.torrent.length);

    session.download_all(|done, total_pieces| {
        let percent = (done as f64 / total_pieces as f64) * 100.0;
        print!("\rProgress: [{}/{}] {:.1}%", done, total_pieces, percent);
        let _ = std::io::stdout().flush();
    })?;

    println!("\nDownload completed successfully.");
    Ok(())
}
