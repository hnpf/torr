use crate::core::download::DownloadSession;
use crate::core::torrent;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn run(source: &str, output_location: Option<&str>) -> Result<(), String> {
    let torrent = torrent::load_source(source)?;
    let dest = resolve_destination(output_location, &torrent.name);

    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

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

pub fn resolve_destination(location: Option<&str>, torrent_name: &str) -> PathBuf {
    match location {
        Some(loc) => {
            let path = Path::new(loc);
            if path.is_dir() || loc == "." || loc == ".." || loc.ends_with('/') {
                path.join(torrent_name)
            } else {
                path.to_path_buf()
            }
        }
        None => PathBuf::from(torrent_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_destination_handles_dot_directory() {
        let dest = resolve_destination(Some("."), "ubuntu.iso");
        assert_eq!(dest, PathBuf::from("./ubuntu.iso"));
    }

    #[test]
    fn resolve_destination_handles_none() {
        let dest = resolve_destination(None, "ubuntu.iso");
        assert_eq!(dest, PathBuf::from("ubuntu.iso"));
    }

    #[test]
    fn resolve_destination_handles_custom_filename() {
        let dest = resolve_destination(Some("custom_name.iso"), "ubuntu.iso");
        assert_eq!(dest, PathBuf::from("custom_name.iso"));
    }
}
