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
    pub bind_interface: Option<crate::core::vpn::InterfaceInfo>,
}

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

pub struct PiecePicker {
    pub completed: Vec<bool>,
    pub in_flight: HashSet<usize>,
    pub completed_count: usize,
    pub total_pieces: usize,
}

impl PiecePicker {
    pub fn new(completed: Vec<bool>) -> Self {
        let completed_count = completed.iter().filter(|&&c| c).count();
        let total_pieces = completed.len();
        Self {
            completed,
            in_flight: HashSet::new(),
            completed_count,
            total_pieces,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.completed_count == self.total_pieces
    }

    pub fn pick_piece(&mut self, peer_has_piece: impl Fn(usize) -> bool) -> Option<usize> {
        if self.is_complete() {
            return None;
        }
        for idx in 0..self.total_pieces {
            if !self.completed[idx] && !self.in_flight.contains(&idx) && peer_has_piece(idx) {
                self.in_flight.insert(idx);
                return Some(idx);
            }
        }
        None
    }

    pub fn mark_completed(&mut self, index: usize) {
        if index < self.total_pieces && !self.completed[index] {
            self.completed[index] = true;
            self.in_flight.remove(&index);
            self.completed_count += 1;
        }
    }

    pub fn cancel_piece(&mut self, index: usize) {
        self.in_flight.remove(&index);
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ProgressInfo {
    pub completed_pieces: usize,
    pub total_pieces: usize,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bps: f64,
    pub eta_seconds: Option<u64>,
    pub active_peers: usize,
}

impl DownloadSession {
    pub fn new(torrent: TorrentFile, output_path: impl AsRef<Path>) -> Result<Self, String> {
        let path = output_path.as_ref();
        let mut storage = Storage::from_torrent(&torrent, path)?;
        let mut completed_pieces = vec![false; torrent.pieces.len()];

        for (idx, hash) in torrent.pieces.iter().enumerate() {
            if storage.verify_piece(idx as u32, hash).is_ok() {
                completed_pieces[idx] = true;
            }
        }

        Ok(Self {
            torrent,
            storage,
            completed_pieces,
            peer_id: generate_peer_id(),
            port: 6881,
            bind_interface: None,
        })
    }

    pub fn set_bind_interface(&mut self, iface: Option<crate::core::vpn::InterfaceInfo>) {
        self.bind_interface = iface;
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

    pub fn download_all<F>(&mut self, on_progress: F) -> Result<(), String>
    where
        F: FnMut(&ProgressInfo),
    {
        if self.is_complete() {
            return Ok(());
        }

        let mut all_trackers = vec![self.torrent.announce.clone()];
        for def_tr in crate::core::magnet::DEFAULT_TRACKERS {
            if !all_trackers.iter().any(|t| t == def_tr) {
                all_trackers.push(def_tr.to_string());
            }
        }

        let mut all_peers = Vec::new();
        for tr in &all_trackers {
            if let Ok(peers) = crate::core::tracker::announce_addrs(
                tr,
                &self.torrent.info_hash,
                &self.peer_id,
                self.port,
                self.left_bytes(),
            ) {
                for p in peers {
                    if !all_peers.contains(&p) {
                        all_peers.push(p);
                    }
                }
            }
        }

        if all_peers.is_empty() {
            return Err("no peers available from any tracker".into());
        }

        self.download_with_peers(&all_peers, on_progress)
    }

    pub fn download_with_peers<F>(&mut self, peers: &[std::net::SocketAddr], mut on_progress: F) -> Result<(), String>
    where
        F: FnMut(&ProgressInfo),
    {
        let picker = Arc::new(Mutex::new(PiecePicker::new(self.completed_pieces.clone())));
        let path = self.storage.path.clone();
        let dummy_storage = Storage::from_torrent(&self.torrent, &path)?;
        let prev_storage = std::mem::replace(&mut self.storage, dummy_storage);
        let storage = Arc::new(Mutex::new(prev_storage));
        let peer_queue = Arc::new(Mutex::new(peers.to_vec()));
        let active_peers = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel::<usize>();
        let shutdown = Arc::new(AtomicBool::new(false));

        let bind_ip = self.bind_interface.as_ref().and_then(|i| i.ip);
        let vpn_monitor = self.bind_interface.as_ref().map(crate::core::vpn::VpnMonitor::new);

        let max_workers = std::cmp::min(peers.len(), 24).max(1);
        let mut handles = Vec::new();

        for _ in 0..max_workers {
            let picker = Arc::clone(&picker);
            let storage = Arc::clone(&storage);
            let peer_queue = Arc::clone(&peer_queue);
            let active_peers = Arc::clone(&active_peers);
            let tx = tx.clone();
            let shutdown = Arc::clone(&shutdown);
            let torrent = self.torrent.clone();
            let peer_id = self.peer_id;

            let handle = std::thread::spawn(move || {
                while !shutdown.load(Ordering::Relaxed) {
                    let addr = {
                        let mut q = peer_queue.lock().unwrap();
                        q.pop()
                    };

                    let addr = match addr {
                        Some(a) => a,
                        None => break,
                    };

                    let conn = match PeerConnection::connect_bound(
                        addr,
                        torrent.info_hash,
                        peer_id,
                        bind_ip,
                        std::time::Duration::from_millis(3500),
                    ) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    let _ = conn.stream.set_read_timeout(Some(std::time::Duration::from_secs(15)));
                    let _ = conn.stream.set_write_timeout(Some(std::time::Duration::from_secs(15)));

                    let mut peer = PeerState::new(conn);
                    if peer.set_interested().is_err() {
                        continue;
                    }

                    active_peers.fetch_add(1, Ordering::SeqCst);

                    let mut choked_wait_start = std::time::Instant::now();
                    while !shutdown.load(Ordering::Relaxed) {
                        if peer.choked {
                            if choked_wait_start.elapsed() > std::time::Duration::from_secs(20) {
                                break;
                            }
                            if peer.receive_and_update().is_err() {
                                break;
                            }
                            continue;
                        }

                        choked_wait_start = std::time::Instant::now();

                        let piece_idx = {
                            let mut p = picker.lock().unwrap();
                            if p.is_complete() {
                                None
                            } else {
                                p.pick_piece(|idx| peer.bitfield.is_empty() || peer.has_piece(idx))
                            }
                        };

                        let idx = match piece_idx {
                            Some(i) => i,
                            None => {
                                if picker.lock().unwrap().is_complete() {
                                    break;
                                }
                                if peer.receive_and_update().is_err() {
                                    break;
                                }
                                continue;
                            }
                        };

                        let piece_size = torrent.piece_size(idx);
                        match peer.download_and_verify_piece(idx as u32, piece_size as u32, &torrent.pieces[idx]) {
                            Ok(data) => {
                                let write_ok = {
                                    let mut s = storage.lock().unwrap();
                                    s.write_piece(idx as u32, &data).is_ok()
                                };

                                if write_ok {
                                    picker.lock().unwrap().mark_completed(idx);
                                    let _ = tx.send(piece_size);
                                } else {
                                    picker.lock().unwrap().cancel_piece(idx);
                                    break;
                                }
                            }
                            Err(_) => {
                                picker.lock().unwrap().cancel_piece(idx);
                                if !peer.choked {
                                    break;
                                }
                            }
                        }
                    }

                    active_peers.fetch_sub(1, Ordering::SeqCst);
                }
            });

            handles.push(handle);
        }

        drop(tx);

        let start_time = Instant::now();
        let total_bytes = self.torrent.length as u64;
        let mut session_bytes = 0u64;

        loop {
            if let Some(ref monitor) = vpn_monitor {
                if !monitor.is_healthy() {
                    eprintln!("\n[Killswitch is active!] VPN '{}' went down! Pausing torrent transfers to prevent any leak...", monitor.interface_name);
                    while !monitor.is_healthy() && !shutdown.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    if monitor.is_healthy() {
                        eprintln!("[Killswitch] VPN is restored. Now resuming download...");
                    }
                }
            }

            let is_done = picker.lock().unwrap().is_complete();
            if is_done {
                shutdown.store(true, Ordering::Relaxed);
                break;
            }

            match rx.recv_timeout(std::time::Duration::from_millis(150)) {
                Ok(piece_size) => {
                    session_bytes += piece_size as u64;
                    while let Ok(extra) = rx.try_recv() {
                        session_bytes += extra as u64;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }

            let (completed_count, left_bytes) = {
                let p = picker.lock().unwrap();
                let left: u64 = (0..p.total_pieces)
                    .filter(|&i| !p.completed[i])
                    .map(|i| self.torrent.piece_size(i) as u64)
                    .sum();
                (p.completed_count, left)
            };

            let elapsed = start_time.elapsed().as_secs_f64();
            let speed_bps = if elapsed > 0.05 {
                session_bytes as f64 / elapsed
            } else {
                0.0
            };

            let downloaded_bytes = total_bytes.saturating_sub(left_bytes);
            let eta_seconds = if speed_bps > 500.0 && left_bytes > 0 {
                Some((left_bytes as f64 / speed_bps) as u64)
            } else {
                None
            };

            on_progress(&ProgressInfo {
                completed_pieces: completed_count,
                total_pieces: self.torrent.pieces.len(),
                downloaded_bytes,
                total_bytes,
                speed_bps,
                eta_seconds,
                active_peers: active_peers.load(Ordering::SeqCst),
            });

            if active_peers.load(Ordering::SeqCst) == 0 && peer_queue.lock().unwrap().is_empty() {
                break;
            }
        }

        shutdown.store(true, Ordering::Relaxed);
        for h in handles {
            let _ = h.join();
        }

        let final_completed = picker.lock().unwrap().completed.clone();
        self.completed_pieces = final_completed;
        if let Ok(s) = Arc::try_unwrap(storage) {
            self.storage = s.into_inner().unwrap();
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
            files: vec![],
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
            files: vec![],
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
            .download_all(|info| {
                progress_calls += 1;
                assert_eq!(info.completed_pieces, 1);
                assert_eq!(info.total_pieces, 1);
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

    #[test]
    fn piece_picker_manages_in_flight_and_completion() {
        let mut picker = PiecePicker::new(vec![false, false, true]);
        assert_eq!(picker.completed_count, 1);
        assert!(!picker.is_complete());
        let picked = picker.pick_piece(|idx| idx == 1);
        assert_eq!(picked, Some(1));
        assert!(picker.in_flight.contains(&1));
        let picked_again = picker.pick_piece(|idx| idx == 1);
        assert_eq!(picked_again, None);
        picker.cancel_piece(1);
        assert!(!picker.in_flight.contains(&1));
        let picked0 = picker.pick_piece(|idx| idx == 0).unwrap();
        assert_eq!(picked0, 0);
        picker.mark_completed(0);
        assert_eq!(picker.completed_count, 2);
        let picked1 = picker.pick_piece(|idx| idx == 1).unwrap();
        assert_eq!(picked1, 1);
        picker.mark_completed(1);
        assert!(picker.is_complete());
    }

    #[test]
    fn download_with_peers_downloads_multiple_pieces_concurrently() {
        use crate::core::peer::{Handshake, Message, PROTOCOL_LEN};
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let dir = std::env::temp_dir();
        let file_path = dir.join("torr_multi_peer_test.bin");
        let _ = fs::remove_file(&file_path);

        let piece0_data = b"0123456789abcdef";
        let piece1_data = b"fedcba9876543210";

        let mut hash0 = Sha1::new();
        hash0.update(piece0_data);
        let mut hash1 = Sha1::new();
        hash1.update(piece1_data);

        let info_hash = [0x77u8; 20];
        let torrent = TorrentFile {
            announce: "http://127.0.0.1:8080/announce".into(),
            info_hash,
            piece_length: 16,
            pieces: vec![hash0.finalize().into(), hash1.finalize().into()],
            name: "test_multi.bin".into(),
            length: 32,
            files: vec![],
        };

        let listener_peer1 = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr_peer1 = listener_peer1.local_addr().unwrap();

        let listener_peer2 = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr_peer2 = listener_peer2.local_addr().unwrap();

        let s1 = std::thread::spawn(move || {
            let (mut socket, _) = listener_peer1.accept().unwrap();
            let mut buf = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            if socket.read_exact(&mut buf).is_err() {
                return;
            }
            let _ = socket.write_all(&Handshake::new(info_hash, *b"-TR0001-111111111111").encode());

            let mut interested = [0u8; 5];
            if socket.read_exact(&mut interested).is_err() {
                return;
            }
            let _ = socket.write_all(&Message::Unchoke.encode());

            loop {
                let mut req = [0u8; 4 + 1 + 12];
                if socket.read_exact(&mut req).is_err() {
                    break;
                }
                if let Ok((Message::Request { index, .. }, _)) = Message::decode(&req) {
                    let data = if index == 0 { piece0_data } else { piece1_data };
                    let piece_msg = Message::Piece { index, begin: 0, block: data.to_vec() };
                    if socket.write_all(&piece_msg.encode()).is_err() {
                        break;
                    }
                }
            }
        });

        let s2 = std::thread::spawn(move || {
            let (mut socket, _) = listener_peer2.accept().unwrap();
            let mut buf = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            if socket.read_exact(&mut buf).is_err() {
                return;
            }
            let _ = socket.write_all(&Handshake::new(info_hash, *b"-TR0001-222222222222").encode());

            let mut interested = [0u8; 5];
            if socket.read_exact(&mut interested).is_err() {
                return;
            }
            let _ = socket.write_all(&Message::Unchoke.encode());

            loop {
                let mut req = [0u8; 4 + 1 + 12];
                if socket.read_exact(&mut req).is_err() {
                    break;
                }
                if let Ok((Message::Request { index, .. }, _)) = Message::decode(&req) {
                    let data = if index == 0 { piece0_data } else { piece1_data };
                    let piece_msg = Message::Piece { index, begin: 0, block: data.to_vec() };
                    if socket.write_all(&piece_msg.encode()).is_err() {
                        break;
                    }
                }
            }
        });

        let mut session = DownloadSession::new(torrent, &file_path).unwrap();
        session.download_with_peers(&[addr_peer1, addr_peer2], |_| {}).unwrap();

        assert!(session.is_complete());
        let content = fs::read(&file_path).unwrap();
        assert_eq!(&content[..16], piece0_data);
        assert_eq!(&content[16..], piece1_data);

        s1.join().unwrap();
        s2.join().unwrap();
        let _ = fs::remove_file(&file_path);
    }
}
