use crate::core::peer::{PeerConnection, PeerState};
use crate::core::storage::Storage;
use crate::core::torrent::TorrentFile;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn generate_peer_id() -> [u8; 20] {
    let mut id = *b"-TC0001-000000000000";
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(123456789);
    let s = format!("{:012}", nanos);
    id[8..20].copy_from_slice(&s.as_bytes()[..12]);
    id
}

pub struct DownloadSession {
    pub torrent: TorrentFile,
    pub storage: Storage,
    pub completed_pieces: Vec<bool>,
    pub peer_id: [u8; 20],
    pub port: u16,
}

impl DownloadSession {
    pub fn new(torrent: TorrentFile, output_path: impl AsRef<Path>) -> Result<Self, String> {
        let path = output_path.as_ref();
        let mut completed_pieces = vec![false; torrent.pieces.len()];

        let storage = if path.exists() {
            let mut existing = Storage::open(path, torrent.piece_length as u32)?;
            for (idx, hash) in torrent.pieces.iter().enumerate() {
                if existing.verify_piece(idx as u32, hash).is_ok() {
                    completed_pieces[idx] = true;
                }
            }
            existing
        } else {
            Storage::create(path, torrent.piece_length as u32, torrent.length as u64)?
        };

        Ok(Self {
            torrent,
            storage,
            completed_pieces,
            peer_id: generate_peer_id(),
            port: 6881,
        })
    }

    pub fn is_complete(&self) -> bool {
        self.completed_pieces.iter().all(|&done| done)
    }

    pub fn completed_count(&self) -> usize {
        self.completed_pieces.iter().filter(|&&done| done).count()
    }

    pub fn left_bytes(&self) -> i64 {
        let mut left = 0i64;
        for (idx, &done) in self.completed_pieces.iter().enumerate() {
            if !done {
                left += self.torrent.piece_size(idx) as i64;
            }
        }
        left
    }

    pub fn download_all<F>(&mut self, mut on_progress: F) -> Result<(), String>
    where
        F: FnMut(usize, usize),
    {
        if self.is_complete() {
            return Ok(());
        }

        let peers = crate::core::tracker::announce_addrs(
            &self.torrent.announce,
            &self.torrent.info_hash,
            &self.peer_id,
            self.port,
            self.left_bytes(),
        )?;

        if peers.is_empty() {
            return Err("tracker returned no peers".into());
        }

        for addr in peers {
            if self.is_complete() {
                break;
            }

            let connection = match PeerConnection::connect(addr, self.torrent.info_hash, self.peer_id) {
                Ok(conn) => conn,
                Err(_) => continue,
            };

            let mut peer = PeerState::new(connection);
            if peer.set_interested().is_err() {
                continue;
            }

            if peer.wait_for_unchoke().is_err() {
                continue;
            }

            for idx in 0..self.completed_pieces.len() {
                if self.completed_pieces[idx] {
                    continue;
                }

                if !peer.bitfield.is_empty() && !peer.has_piece(idx) {
                    continue;
                }

                let piece_size = self.torrent.piece_size(idx);
                match peer.download_and_verify_piece(
                    idx as u32,
                    piece_size as u32,
                    &self.torrent.pieces[idx],
                ) {
                    Ok(data) => {
                        if self.storage.write_piece(idx as u32, &data).is_err() {
                            break;
                        }
                        self.completed_pieces[idx] = true;
                        on_progress(self.completed_count(), self.torrent.pieces.len());
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }

        if !self.is_complete() {
            return Err(format!(
                "download incomplete: {}/{} pieces downloaded",
                self.completed_count(),
                self.torrent.pieces.len()
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest, Sha1};
    use std::fs;

    #[test]
    fn peer_id_starts_with_client_prefix_and_is_20_bytes() {
        let id = generate_peer_id();
        assert_eq!(id.len(), 20);
        assert_eq!(&id[..8], b"-TC0001-");
    }

    #[test]
    fn session_resume_detects_already_downloaded_pieces() {
        let dir = std::env::temp_dir();
        let file_path = dir.join("tc_session_resume_test.bin");
        let _ = fs::remove_file(&file_path);

        let piece_data = b"0123456789abcdef";
        let mut hasher = Sha1::new();
        hasher.update(piece_data);
        let piece_hash: [u8; 20] = hasher.finalize().into();

        let torrent = TorrentFile {
            announce: "http://127.0.0.1:8080/announce".into(),
            info_hash: [0x12; 20],
            piece_length: 16,
            pieces: vec![piece_hash],
            name: "test.bin".into(),
            length: 16,
        };

        let mut session = DownloadSession::new(torrent.clone(), &file_path).unwrap();
        assert_eq!(session.completed_count(), 0);
        session.storage.write_piece(0, piece_data).unwrap();

        let resumed = DownloadSession::new(torrent, &file_path).unwrap();
        assert_eq!(resumed.completed_count(), 1);
        assert!(resumed.is_complete());

        let _ = fs::remove_file(&file_path);
    }

    #[test]
    fn download_all_end_to_end_with_mock_tracker_and_peer() {
        use crate::core::peer::{Handshake, Message, PROTOCOL_LEN};
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let dir = std::env::temp_dir();
        let file_path = dir.join("tc_e2e_download_test.bin");
        let _ = fs::remove_file(&file_path);

        let piece_data = b"0123456789abcdef";
        let mut hasher = Sha1::new();
        hasher.update(piece_data);
        let piece_hash: [u8; 20] = hasher.finalize().into();

        let info_hash = [0x55u8; 20];
        let peer_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let peer_addr = peer_listener.local_addr().unwrap();

        let tracker_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let tracker_addr = tracker_listener.local_addr().unwrap();
        let announce_url = format!("http://127.0.0.1:{}/announce", tracker_addr.port());

        let torrent = TorrentFile {
            announce: announce_url,
            info_hash,
            piece_length: 16,
            pieces: vec![piece_hash],
            name: "test.bin".into(),
            length: 16,
        };

        let tracker_server = std::thread::spawn(move || {
            let (mut socket, _) = tracker_listener.accept().unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                if line == "\r\n" || line == "\n" {
                    break;
                }
                line.clear();
            }

            let peers_bytes = [127, 0, 0, 1, (peer_addr.port() >> 8) as u8, (peer_addr.port() & 0xff) as u8];
            let mut body = Vec::new();
            body.extend_from_slice(b"d8:intervali1800e5:peers6:");
            body.extend_from_slice(&peers_bytes);
            body.extend_from_slice(b"e");

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n",
                body.len()
            );
            socket.write_all(response.as_bytes()).unwrap();
            socket.write_all(&body).unwrap();
        });

        let peer_server = std::thread::spawn(move || {
            let (mut socket, _) = peer_listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, *b"-TC0001-999999999999");
            socket.write_all(&response.encode()).unwrap();

            let mut interested_buf = [0u8; 5];
            socket.read_exact(&mut interested_buf).unwrap();

            socket.write_all(&Message::Unchoke.encode()).unwrap();

            let mut req_buf = [0u8; 4 + 1 + 12];
            socket.read_exact(&mut req_buf).unwrap();

            let piece_msg = Message::Piece {
                index: 0,
                begin: 0,
                block: piece_data.to_vec(),
            };
            socket.write_all(&piece_msg.encode()).unwrap();
        });

        let mut session = DownloadSession::new(torrent, &file_path).unwrap();
        let mut progress_calls = 0;
        session
            .download_all(|done, total| {
                progress_calls += 1;
                assert_eq!(done, 1);
                assert_eq!(total, 1);
            })
            .unwrap();

        assert_eq!(progress_calls, 1);
        assert!(session.is_complete());

        let read_back = fs::read(&file_path).unwrap();
        assert_eq!(read_back, piece_data);

        tracker_server.join().unwrap();
        peer_server.join().unwrap();
        let _ = fs::remove_file(&file_path);
    }
}
