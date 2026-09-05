use crate::core::piece;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSpan {
    pub path: PathBuf,
    pub length: u64,
    pub offset: u64,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct Storage {
    pub piece_length: u32,
    pub length: u64,
    pub path: PathBuf,
    pub spans: Vec<FileSpan>,
}

#[allow(dead_code)]
impl Storage {
    pub fn from_torrent(torrent: &crate::core::torrent::TorrentFile, path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let spans = if torrent.files.is_empty() {
            vec![FileSpan {
                path: path.clone(),
                length: torrent.length as u64,
                offset: 0,
            }]
        } else {
            let mut spans = Vec::new();
            let mut curr = 0u64;
            for f in &torrent.files {
                let mut p = path.clone();
                for seg in &f.path {
                    p.push(seg);
                }
                spans.push(FileSpan {
                    path: p,
                    length: f.length as u64,
                    offset: curr,
                });
                curr += f.length as u64;
            }
            spans
        };

        for span in &spans {
            if let Some(parent) = span.path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&span.path)
                .map_err(|e| format!("failed to create {:?}: {e}", span.path))?;
            if file.metadata().map(|m| m.len()).unwrap_or(0) < span.length {
                let _ = file.set_len(span.length);
            }
        }

        Ok(Self {
            piece_length: torrent.piece_length as u32,
            length: torrent.length as u64,
            path,
            spans,
        })
    }

    pub fn create(path: impl AsRef<Path>, piece_length: u32, length: u64) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| e.to_string())?;

        file.set_len(length).map_err(|e| e.to_string())?;

        let spans = vec![FileSpan {
            path: path.clone(),
            length,
            offset: 0,
        }];

        Ok(Self { piece_length, length, path, spans })
    }

    pub fn open(path: impl AsRef<Path>, piece_length: u32) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();

        let spans = vec![FileSpan {
            path: path.clone(),
            length,
            offset: 0,
        }];

        Ok(Self { piece_length, length, path, spans })
    }

    pub fn write_block(&mut self, index: u32, begin: u32, data: &[u8]) -> Result<(), String> {
        let piece_start = index as u64 * self.piece_length as u64;
        let global_start = piece_start + begin as u64;
        let global_end = global_start + data.len() as u64;

        if global_end > self.length {
            return Err("write past piece bounds".into());
        }

        let mut data_written = 0usize;
        for span in &self.spans {
            let span_start = span.offset;
            let span_end = span.offset + span.length;

            if global_start < span_end && global_end > span_start {
                let write_start_in_span = global_start.max(span_start) - span_start;
                let overlap_start = global_start.max(span_start);
                let overlap_end = global_end.min(span_end);
                let chunk_len = (overlap_end - overlap_start) as usize;

                if let Some(parent) = span.path.parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                }

                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&span.path)
                    .map_err(|e| format!("failed to open {:?}: {e}", span.path))?;

                file.seek(SeekFrom::Start(write_start_in_span)).map_err(|e| e.to_string())?;
                file.write_all(&data[data_written..data_written + chunk_len]).map_err(|e| e.to_string())?;

                data_written += chunk_len;
            }
        }

        Ok(())
    }

    pub fn write_piece(&mut self, index: u32, data: &[u8]) -> Result<(), String> {
        self.write_block(index, 0, data)
    }

    pub fn read_piece(&mut self, index: u32) -> Result<Vec<u8>, String> {
        let piece_start = index as u64 * self.piece_length as u64;
        if piece_start >= self.length {
            return Err("piece index out of range".into());
        }

        let remaining = self.length - piece_start;
        let length = std::cmp::min(remaining, self.piece_length as u64) as usize;
        let global_end = piece_start + length as u64;
        let mut buffer = vec![0u8; length];
        let mut data_read = 0usize;

        for span in &self.spans {
            let span_start = span.offset;
            let span_end = span.offset + span.length;

            if piece_start < span_end && global_end > span_start {
                let read_start_in_span = piece_start.max(span_start) - span_start;
                let overlap_start = piece_start.max(span_start);
                let overlap_end = global_end.min(span_end);
                let chunk_len = (overlap_end - overlap_start) as usize;

                if let Ok(mut file) = File::open(&span.path) {
                    file.seek(SeekFrom::Start(read_start_in_span)).map_err(|e| e.to_string())?;
                    let _ = file.read_exact(&mut buffer[data_read..data_read + chunk_len]);
                }

                data_read += chunk_len;
            }
        }

        Ok(buffer)
    }

    pub fn verify_piece(&mut self, index: u32, expected_hash: &[u8; 20]) -> Result<(), String> {
        let data = self.read_piece(index)?;
        piece::verify_piece(expected_hash, &data)
    }

    pub fn verify_all(&mut self, piece_hashes: &[[u8; 20]]) -> Result<(), String> {
        for (index, expected_hash) in piece_hashes.iter().enumerate() {
            self.verify_piece(index as u32, expected_hash)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest, Sha1};
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn create_write_and_verify_single_piece() {
        let path = temp_path("tc_storage_single_piece.bin");
        let _ = fs::remove_file(&path);

        let mut storage = Storage::create(&path, 16, 16).unwrap();
        assert_eq!(storage.path, path);

        let data = b"0123456789abcdef";
        storage.write_block(0, 0, data).unwrap();

        let mut hasher = Sha1::new();
        hasher.update(data);
        let expected: [u8; 20] = hasher.finalize().into();

        assert!(storage.verify_piece(0, &expected).is_ok());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn open_reads_existing_file_and_verifies_path() {
        let path = temp_path("tc_storage_open.bin");
        let _ = fs::remove_file(&path);

        let mut created = Storage::create(&path, 4, 10).unwrap();
        created.write_block(0, 0, b"abcd").unwrap();
        let path_clone = path.clone();

        let storage = Storage::open(&path, 4).unwrap();
        assert_eq!(storage.path, path_clone);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn verify_all_checks_every_piece() {
        let path = temp_path("tc_storage_verify_all.bin");
        let _ = fs::remove_file(&path);

        let mut storage = Storage::create(&path, 4, 10).unwrap();
        storage.write_block(0, 0, b"abcd").unwrap();
        storage.write_block(1, 0, b"efgh").unwrap();
        storage.write_block(2, 0, b"ij").unwrap();

        let mut hash0 = Sha1::new();
        hash0.update(b"abcd");
        let mut hash1 = Sha1::new();
        hash1.update(b"efgh");
        let mut hash2 = Sha1::new();
        hash2.update(b"ij");

        let hashes = [hash0.finalize().into(), hash1.finalize().into(), hash2.finalize().into()];
        assert!(storage.verify_all(&hashes).is_ok());

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn multi_file_storage_writes_and_reads_spanning_piece() {
        use crate::core::torrent::{TorrentFile, TorrentFileInfo};

        let temp_dir = std::env::temp_dir().join("torr_multi_storage_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let torrent = TorrentFile {
            announce: "".into(),
            info_hash: [0u8; 20],
            piece_length: 10,
            pieces: vec![[0u8; 20], [0u8; 20]],
            name: "test_multi".into(),
            length: 20,
            files: vec![
                TorrentFileInfo {
                    path: vec!["sub".into(), "file1.txt".into()],
                    length: 6,
                },
                TorrentFileInfo {
                    path: vec!["file2.txt".into()],
                    length: 14,
                },
            ],
        };

        let mut storage = Storage::from_torrent(&torrent, &temp_dir).unwrap();
        assert_eq!(storage.spans.len(), 2);
        let piece0_data = b"0123456789";
        storage.write_piece(0, piece0_data).unwrap();
        let piece1_data = b"abcdefghij";
        storage.write_piece(1, piece1_data).unwrap();
        let file1_content = fs::read(temp_dir.join("sub").join("file1.txt")).unwrap();
        assert_eq!(file1_content, b"012345");

        let file2_content = fs::read(temp_dir.join("file2.txt")).unwrap();
        assert_eq!(file2_content, b"6789abcdefghij");
        let read_p0 = storage.read_piece(0).unwrap();
        assert_eq!(read_p0, piece0_data);

        let read_p1 = storage.read_piece(1).unwrap();
        assert_eq!(read_p1, piece1_data);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
