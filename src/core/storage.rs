use crate::core::piece;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[derive(Debug)]
pub struct Storage {
    file: File,
    pub piece_length: u32,
    pub length: u64,
    pub path: PathBuf,
}

#[allow(dead_code)]
impl Storage {
    pub fn create(path: impl AsRef<Path>, piece_length: u32, length: u64) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| e.to_string())?;

        file.set_len(length).map_err(|e| e.to_string())?;

        Ok(Self { file, piece_length, length, path })
    }

    pub fn open(path: impl AsRef<Path>, piece_length: u32) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();

        Ok(Self { file, piece_length, length, path })
    }

    pub fn write_block(&mut self, index: u32, begin: u32, data: &[u8]) -> Result<(), String> {
        let piece_start = index as u64 * self.piece_length as u64;
        if piece_start >= self.length {
            return Err("piece index out of range".into());
        }

        let max_block = std::cmp::min(self.piece_length as u64, self.length - piece_start);
        if begin as u64 + data.len() as u64 > max_block {
            return Err("write past piece bounds".into());
        }

        let offset = piece_start + begin as u64;
        self.file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        self.file.write_all(data).map_err(|e| e.to_string())?;
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
        let mut buffer = vec![0u8; length];

        self.file.seek(SeekFrom::Start(piece_start)).map_err(|e| e.to_string())?;
        self.file.read_exact(&mut buffer).map_err(|e| e.to_string())?;
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
}
