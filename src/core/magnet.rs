use crate::core::bencode::{self, BencodeValue};
use crate::core::peer::{Message, PeerConnection};
use crate::core::torrent::TorrentFile;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap};

pub const DEFAULT_TRACKERS: &[&str] = &[
    "http://tracker.opentrackr.org:1337/announce",
    "http://open.acgnxtracker.com:80/announce",
    "https://tracker.tamersunion.org:443/announce",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Magnet {
    pub info_hash: [u8; 20],
    pub display_name: Option<String>,
    pub trackers: Vec<String>,
}

pub fn parse_magnet_uri(uri: &str) -> Result<Magnet, String> {
    let query = if let Some(q) = uri.strip_prefix("magnet:?") {
        q
    } else if let Some(q) = uri.strip_prefix("magnet:") {
        q
    } else {
        return Err("not a magnet uri".into());
    };

    let mut info_hash = None;
    let mut display_name = None;
    let mut trackers = Vec::new();

    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (key, val) = match part.split_once('=') {
            Some((k, v)) => (k, v),
            None => continue,
        };

        match key {
            "xt" => {
                let lower_val = val.to_ascii_lowercase();
                let hash_str = if let Some(stripped) = lower_val.strip_prefix("urn:btih:") {
                    stripped
                } else {
                    &lower_val
                };

                let hash = if hash_str.len() == 40 {
                    decode_hex(hash_str)?
                } else if hash_str.len() == 32 {
                    decode_base32(hash_str)?
                } else {
                    return Err(format!("invalid info_hash length in magnet: {}", hash_str.len()));
                };
                info_hash = Some(hash);
            }
            "dn" => {
                display_name = Some(percent_decode(val));
            }
            "tr" => {
                let tracker = percent_decode(val);
                if !tracker.is_empty() && !trackers.contains(&tracker) {
                    trackers.push(tracker);
                }
            }
            _ => {}
        }
    }

    let info_hash = info_hash.ok_or_else(|| "missing xt (urn:btih) in magnet uri".to_string())?;

    Ok(Magnet {
        info_hash,
        display_name,
        trackers,
    })
}

pub fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let bytes_in = input.as_bytes();
    let mut i = 0;
    while i < bytes_in.len() {
        if bytes_in[i] == b'%' && i + 2 < bytes_in.len() {
            if let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                bytes.push(byte);
                i += 3;
                continue;
            }
        }
        if bytes_in[i] == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(bytes_in[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&bytes).to_string()
}

pub fn decode_hex(hex: &str) -> Result<[u8; 20], String> {
    if hex.len() != 40 {
        return Err("hex hash must be 40 chars".into());
    }
    let mut out = [0u8; 20];
    for i in 0..20 {
        let chunk = &hex[i * 2..i * 2 + 2];
        out[i] = u8::from_str_radix(chunk, 16)
            .map_err(|_| format!("invalid hex char in hash: {}", chunk))?;
    }
    Ok(out)
}

pub fn decode_base32(input: &str) -> Result<[u8; 20], String> {
    if input.len() != 32 {
        return Err("base32 hash must be 32 chars".into());
    }
    let mut out = [0u8; 20];
    let mut buffer: u64 = 0;
    let mut bits: u32 = 0;
    let mut out_idx = 0;

    for ch in input.chars() {
        let val = match ch {
            'A'..='Z' => (ch as u8 - b'A') as u64,
            'a'..='z' => (ch as u8 - b'a') as u64,
            '2'..='7' => (ch as u8 - b'2' + 26) as u64,
            _ => return Err(format!("invalid base32 character: {}", ch)),
        };
        buffer = (buffer << 5) | val;
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            if out_idx < 20 {
                out[out_idx] = ((buffer >> bits) & 0xff) as u8;
                out_idx += 1;
            }
        }
    }

    if out_idx != 20 {
        return Err("incomplete base32 decode".into());
    }
    Ok(out)
}

pub fn build_extended_handshake() -> Vec<u8> {
    let mut m = BTreeMap::new();
    m.insert(b"ut_metadata".to_vec(), BencodeValue::Int(1));
    let mut dict = BTreeMap::new();
    dict.insert(b"m".to_vec(), BencodeValue::Dict(m));
    BencodeValue::Dict(dict).encode()
}

#[derive(Debug, Clone)]
pub struct ExtendedHandshake {
    pub ut_metadata_id: u8,
    pub metadata_size: Option<usize>,
}

pub fn parse_extended_handshake(payload: &[u8]) -> Result<ExtendedHandshake, String> {
    let (decoded, _) = bencode::decode(payload)?;
    let dict = match decoded {
        BencodeValue::Dict(d) => d,
        _ => return Err("extension handshake payload is not a bencode dict".into()),
    };

    let m_dict = match dict.get("m".as_bytes()) {
        Some(BencodeValue::Dict(d)) => d,
        _ => return Err("missing 'm' dictionary in extension handshake".into()),
    };

    let ut_metadata_id = match m_dict.get("ut_metadata".as_bytes()) {
        Some(BencodeValue::Int(id)) if *id > 0 && *id <= 255 => *id as u8,
        _ => return Err("peer does not support ut_metadata".into()),
    };

    let metadata_size = match dict.get("metadata_size".as_bytes()) {
        Some(BencodeValue::Int(size)) if *size > 0 => Some(*size as usize),
        _ => None,
    };

    Ok(ExtendedHandshake {
        ut_metadata_id,
        metadata_size,
    })
}

pub fn build_metadata_request(piece: u32) -> Vec<u8> {
    let mut dict = BTreeMap::new();
    dict.insert(b"msg_type".to_vec(), BencodeValue::Int(0));
    dict.insert(b"piece".to_vec(), BencodeValue::Int(piece as i64));
    BencodeValue::Dict(dict).encode()
}

pub fn parse_metadata_data(payload: &[u8]) -> Result<(u32, Option<usize>, Vec<u8>), String> {
    let (decoded, raw_bytes) = bencode::decode(payload)?;
    let dict = match decoded {
        BencodeValue::Dict(d) => d,
        _ => return Err("metadata piece header is not a dict".into()),
    };

    let msg_type = match dict.get("msg_type".as_bytes()) {
        Some(BencodeValue::Int(t)) => *t,
        _ => return Err("missing msg_type in metadata payload".into()),
    };

    if msg_type == 2 {
        return Err("peer rejected metadata request".into());
    }
    if msg_type != 1 {
        return Err(format!("unexpected metadata msg_type: {}", msg_type));
    }

    let piece = match dict.get("piece".as_bytes()) {
        Some(BencodeValue::Int(p)) => *p as u32,
        _ => return Err("missing piece index in metadata payload".into()),
    };

    let total_size = match dict.get("total_size".as_bytes()) {
        Some(BencodeValue::Int(s)) if *s > 0 => Some(*s as usize),
        _ => None,
    };

    Ok((piece, total_size, raw_bytes.to_vec()))
}

pub fn fetch_metadata_from_peer(
    conn: &mut PeerConnection,
    info_hash: &[u8; 20],
) -> Result<Vec<u8>, String> {
    conn.send_extended(0, build_extended_handshake())?;

    let mut ext_handshake: Option<ExtendedHandshake> = None;
    let timeout_start = std::time::Instant::now();
    while ext_handshake.is_none() {
        if timeout_start.elapsed() > std::time::Duration::from_secs(10) {
            return Err("timeout waiting for extension handshake".into());
        }
        match conn.receive_message()? {
            Message::Extended { ext_id: 0, payload } => {
                ext_handshake = Some(parse_extended_handshake(&payload)?);
            }
            Message::KeepAlive | Message::Bitfield(_) | Message::Have(_) | Message::Unchoke | Message::Choke => {
                continue;
            }
            _ => continue,
        }
    }

    let ext = ext_handshake.unwrap();
    let peer_ext_id = ext.ut_metadata_id;

    let mut metadata_size = ext.metadata_size;
    let mut pieces: HashMap<u32, Vec<u8>> = HashMap::new();

    conn.send_extended(peer_ext_id, build_metadata_request(0))?;

    let req_timeout = std::time::Duration::from_secs(15);
    let start = std::time::Instant::now();

    while pieces.get(&0).is_none() {
        if start.elapsed() > req_timeout {
            return Err("timeout waiting for metadata piece 0".into());
        }
        match conn.receive_message()? {
            Message::Extended { ext_id, payload } if ext_id == 1 || ext_id == peer_ext_id => {
                let (piece, total_size, data) = parse_metadata_data(&payload)?;
                if piece == 0 {
                    if metadata_size.is_none() {
                        metadata_size = total_size;
                    }
                    pieces.insert(piece, data);
                }
            }
            _ => continue,
        }
    }

    let total_bytes = metadata_size.ok_or_else(|| "unknown metadata size".to_string())?;
    let num_pieces = (total_bytes + 16383) / 16384;

    for p in 1..num_pieces as u32 {
        conn.send_extended(peer_ext_id, build_metadata_request(p))?;
        let piece_start = std::time::Instant::now();
        while pieces.get(&p).is_none() {
            if piece_start.elapsed() > req_timeout {
                return Err(format!("timeout waiting for metadata piece {}", p));
            }
            match conn.receive_message()? {
                Message::Extended { ext_id, payload } if ext_id == 1 || ext_id == peer_ext_id => {
                    let (piece, _, data) = parse_metadata_data(&payload)?;
                    pieces.insert(piece, data);
                }
                _ => continue,
            }
        }
    }

    let mut assembled = Vec::with_capacity(total_bytes);
    for p in 0..num_pieces as u32 {
        let chunk = pieces.get(&p).ok_or_else(|| format!("missing metadata piece {}", p))?;
        assembled.extend_from_slice(chunk);
    }
    if assembled.len() > total_bytes {
        assembled.truncate(total_bytes);
    }

    let mut hasher = Sha1::new();
    hasher.update(&assembled);
    let hash: [u8; 20] = hasher.finalize().into();

    if &hash != info_hash {
        return Err("assembled metadata sha1 mismatch".into());
    }

    Ok(assembled)
}

pub fn fetch_torrent(magnet_uri: &str) -> Result<TorrentFile, String> {
    let magnet = parse_magnet_uri(magnet_uri)?;

    let mut trackers = Vec::new();
    for tr in &magnet.trackers {
        if tr.starts_with("http://") || tr.starts_with("https://") {
            trackers.push(tr.clone());
        }
    }
    for default_tr in DEFAULT_TRACKERS {
        if !trackers.iter().any(|t| t == default_tr) {
            trackers.push(default_tr.to_string());
        }
    }

    if trackers.is_empty() {
        return Err("no valid http/https trackers available for magnet link".into());
    }

    let peer_id = crate::core::download::generate_peer_id();
    let port = 6881;

    let hex_hash: String = magnet.info_hash.iter().map(|b| format!("{:02x}", b)).collect();
    println!("Resolving magnet link (info_hash: {})...", hex_hash);

    let mut last_err = String::from("could not find any peers with metadata");

    for tr in &trackers {
        println!("Announcing to tracker: {}", tr);
        let addrs = match crate::core::tracker::announce_addrs(tr, &magnet.info_hash, &peer_id, port, 0) {
            Ok(a) if !a.is_empty() => a,
            Ok(_) => {
                println!("  Tracker returned 0 peers. Trying next...");
                continue;
            }
            Err(e) => {
                println!("  Tracker failed ({}). Trying next...", e);
                last_err = format!("tracker announce error: {}", e);
                continue;
            }
        };

        println!("Found {} peers from tracker. Contacting swarm for metadata...", addrs.len());

        for addr in addrs {
            let conn_res = PeerConnection::connect_timeout(
                addr,
                magnet.info_hash,
                peer_id,
                std::time::Duration::from_secs(4),
            );

            let mut conn = match conn_res {
                Ok(c) => c,
                Err(_) => continue,
            };

            if !conn.remote_handshake.supports_extended() {
                continue;
            }

            match fetch_metadata_from_peer(&mut conn, &magnet.info_hash) {
                Ok(info_bytes) => {
                    println!("Metadata downloaded and verified successfully.");
                    return crate::core::torrent::from_info_bytes(
                        &info_bytes,
                        tr,
                        magnet.display_name.as_deref(),
                    );
                }
                Err(e) => {
                    last_err = format!("failed to get metadata from peer {}: {}", addr, e);
                }
            }
        }
    }

    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parse_magnet_hex_uri() {
        let uri = "magnet:?xt=urn:btih:dafc8c076ca2f3ed376eeae7c76a0d6be2415c45&dn=ubuntu-26.04-desktop-amd64.iso&tr=http%3A%2F%2Ftorrent.ubuntu.com%3A6969%2Fannounce";
        let magnet = parse_magnet_uri(uri).unwrap();

        let expected_hash = decode_hex("dafc8c076ca2f3ed376eeae7c76a0d6be2415c45").unwrap();
        assert_eq!(magnet.info_hash, expected_hash);
        assert_eq!(magnet.display_name, Some("ubuntu-26.04-desktop-amd64.iso".to_string()));
        assert_eq!(magnet.trackers, vec!["http://torrent.ubuntu.com:6969/announce".to_string()]);
    }

    #[test]
    fn parse_magnet_base32_uri() {
        let uri = "magnet:?xt=urn:btih:3k6iyb3mule62n3o53t4owvnepneclcf&dn=test+file";
        let magnet = parse_magnet_uri(uri).unwrap();
        assert_eq!(magnet.display_name, Some("test file".to_string()));
        assert_eq!(magnet.info_hash.len(), 20);
    }

    #[test]
    fn parse_magnet_multiple_trackers() {
        let uri = "magnet:?xt=urn:btih:dafc8c076ca2f3ed376eeae7c76a0d6be2415c45&tr=http%3A%2F%2Ftracker1.com%2Fannounce&tr=http%3A%2F%2Ftracker2.com%2Fannounce";
        let magnet = parse_magnet_uri(uri).unwrap();
        assert_eq!(magnet.trackers.len(), 2);
        assert_eq!(magnet.trackers[0], "http://tracker1.com/announce");
        assert_eq!(magnet.trackers[1], "http://tracker2.com/announce");
    }

    #[test]
    fn parse_magnet_missing_xt_returns_err() {
        let uri = "magnet:?dn=only_name";
        assert!(parse_magnet_uri(uri).is_err());
    }

    #[test]
    fn percent_decode_handles_spaces_and_special_chars() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("foo+bar"), "foo bar");
        assert_eq!(percent_decode("http%3A%2F%2Fexample.com"), "http://example.com");
    }

    #[test]
    fn extended_handshake_roundtrip() {
        let encoded = build_extended_handshake();
        let handshake = parse_extended_handshake(&encoded).unwrap();
        assert_eq!(handshake.ut_metadata_id, 1);
        assert_eq!(handshake.metadata_size, None);

        let mut m = BTreeMap::new();
        m.insert(b"ut_metadata".to_vec(), BencodeValue::Int(2));
        let mut dict = BTreeMap::new();
        dict.insert(b"m".to_vec(), BencodeValue::Dict(m));
        dict.insert(b"metadata_size".to_vec(), BencodeValue::Int(16384));
        let custom_encoded = BencodeValue::Dict(dict).encode();

        let parsed = parse_extended_handshake(&custom_encoded).unwrap();
        assert_eq!(parsed.ut_metadata_id, 2);
        assert_eq!(parsed.metadata_size, Some(16384));
    }

    #[test]
    fn metadata_request_and_data_roundtrip() {
        let req = build_metadata_request(3);
        let (decoded, _) = bencode::decode(&req).unwrap();
        if let BencodeValue::Dict(d) = decoded {
            assert_eq!(d.get("msg_type".as_bytes()), Some(&BencodeValue::Int(0)));
            assert_eq!(d.get("piece".as_bytes()), Some(&BencodeValue::Int(3)));
        } else {
            panic!("expected dict");
        }

        let mut header = BTreeMap::new();
        header.insert(b"msg_type".to_vec(), BencodeValue::Int(1));
        header.insert(b"piece".to_vec(), BencodeValue::Int(0));
        header.insert(b"total_size".to_vec(), BencodeValue::Int(5));
        let mut payload = BencodeValue::Dict(header).encode();
        payload.extend_from_slice(b"hello");

        let (piece, total_size, data) = parse_metadata_data(&payload).unwrap();
        assert_eq!(piece, 0);
        assert_eq!(total_size, Some(5));
        assert_eq!(data, b"hello");
    }

    #[test]
    fn fetch_metadata_from_mock_peer_success() {
        let info_dict_bytes = b"d6:lengthi1024e4:name4:test12:piece lengthi512e6:pieces20:01234567890123456789e";
        let mut hasher = Sha1::new();
        hasher.update(info_dict_bytes);
        let info_hash: [u8; 20] = hasher.finalize().into();

        let peer_id = *b"-TC0001-mockpeer0001";
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();

        let info_clone = info_dict_bytes.to_vec();
        let server_thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let remote = crate::core::peer::Handshake::read_from(&mut stream).unwrap();
            let mut local = crate::core::peer::Handshake::new(remote.info_hash, *b"-SRV001-serverpeer00");
            local.reserved[5] |= 0x10;
            local.write_to(&mut stream).unwrap();

            // Read client extended handshake
            let client_ext = Message::read_from(&mut stream).unwrap();
            if let Message::Extended { ext_id: 0, .. } = client_ext {
                // Send server extended handshake advertising ut_metadata ID 3 and metadata_size
                let mut m = BTreeMap::new();
                m.insert(b"ut_metadata".to_vec(), BencodeValue::Int(3));
                let mut d = BTreeMap::new();
                d.insert(b"m".to_vec(), BencodeValue::Dict(m));
                d.insert(b"metadata_size".to_vec(), BencodeValue::Int(info_clone.len() as i64));
                let srv_ext = BencodeValue::Dict(d).encode();
                Message::Extended { ext_id: 0, payload: srv_ext }.write_to(&mut stream).unwrap();
            }

            // Read client request for piece 0
            let req_msg = Message::read_from(&mut stream).unwrap();
            if let Message::Extended { ext_id: 3, payload } = req_msg {
                let (val, _) = bencode::decode(&payload).unwrap();
                if let BencodeValue::Dict(req_dict) = val {
                    assert_eq!(req_dict.get("msg_type".as_bytes()), Some(&BencodeValue::Int(0)));
                    assert_eq!(req_dict.get("piece".as_bytes()), Some(&BencodeValue::Int(0)));

                    // Reply with data piece
                    let mut data_header = BTreeMap::new();
                    data_header.insert(b"msg_type".to_vec(), BencodeValue::Int(1));
                    data_header.insert(b"piece".to_vec(), BencodeValue::Int(0));
                    data_header.insert(b"total_size".to_vec(), BencodeValue::Int(info_clone.len() as i64));
                    let mut piece_payload = BencodeValue::Dict(data_header).encode();
                    piece_payload.extend_from_slice(&info_clone);

                    Message::Extended { ext_id: 1, payload: piece_payload }.write_to(&mut stream).unwrap();
                }
            }
        });

        let mut conn = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        let metadata = fetch_metadata_from_peer(&mut conn, &info_hash).unwrap();
        assert_eq!(metadata, info_dict_bytes);

        server_thread.join().unwrap();
    }
}
