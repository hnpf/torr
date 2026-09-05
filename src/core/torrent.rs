use crate::core::bencode::BencodeValue;
use sha1::{Sha1, Digest};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentFileInfo {
    pub path: Vec<String>,
    pub length: i64,
}

#[derive(Debug, Clone)]
pub struct TorrentFile {
    pub announce: String,
    pub info_hash: [u8; 20],       // raw sha1 bytes is what trackers + peers want
    pub piece_length: i64,
    pub pieces: Vec<[u8; 20]>,     // sha1 hash per piece chopped from one big blob :sob:
    pub name: String,
    pub length: i64,               // total size across all files
    pub files: Vec<TorrentFileInfo>,
}

impl TorrentFile {
    pub fn is_multi_file(&self) -> bool {
        !self.files.is_empty()
    }

    pub fn piece_size(&self, index: usize) -> usize {
        if index >= self.pieces.len() {
            return 0;
        }
        if index == self.pieces.len() - 1 {
            let rem = self.length % self.piece_length;
            if rem == 0 {
                self.piece_length as usize
            } else {
                rem as usize
            }
        } else {
            self.piece_length as usize
        }
    }
}

pub fn from_info_bytes(info_bytes: &[u8], announce: &str, fallback_name: Option<&str>) -> Result<TorrentFile, String> {
    let (decoded, _) = crate::core::bencode::decode(info_bytes)?;
    let info = match decoded {
        BencodeValue::Dict(d) => d,
        _ => return Err("info must be a dict".into()),
    };

    let mut hasher = Sha1::new();
    hasher.update(info_bytes);
    let info_hash: [u8; 20] = hasher.finalize().into();

    let piece_length = get_int(&info, "piece length")?;
    let name = match get_string(&info, "name") {
        Ok(n) => n,
        Err(_) => fallback_name
            .map(|s| s.to_string())
            .unwrap_or_else(|| info_hash.iter().map(|b| format!("{:02x}", b)).collect()),
    };

    let (length, files) = match info.get("files".as_bytes()) {
        Some(BencodeValue::List(file_list)) => {
            let mut files = Vec::new();
            let mut total_len = 0i64;
            for item in file_list {
                let dict = match item {
                    BencodeValue::Dict(d) => d,
                    _ => return Err("file entry in files list must be a dict".into()),
                };
                let file_len = get_int(dict, "length")?;
                total_len += file_len;

                let path_val = match dict.get("path".as_bytes()) {
                    Some(BencodeValue::List(p)) => p,
                    _ => return Err("file entry missing path list".into()),
                };

                let mut path_segments = Vec::new();
                for seg in path_val {
                    match seg {
                        BencodeValue::Bytes(b) => {
                            let s = String::from_utf8(b.clone()).map_err(|_| "bad utf8 in file path".to_string())?;
                            path_segments.push(s);
                        }
                        _ => return Err("path segment must be bytes".into()),
                    }
                }

                files.push(TorrentFileInfo {
                    path: path_segments,
                    length: file_len,
                });
            }
            (total_len, files)
        }
        _ => {
            let file_len = get_int(&info, "length")?;
            (file_len, Vec::new())
        }
    };

    let pieces_raw = match info.get("pieces".as_bytes()) {
        Some(BencodeValue::Bytes(b)) => b,
        _ => return Err("missing pieces".into()),
    };
    if pieces_raw.len() % 20 != 0 {
        return Err("pieces field length not a multiple of 20".into());
    }
    let pieces: Vec<[u8; 20]> = pieces_raw
        .chunks(20)
        .map(|chunk| chunk.try_into().unwrap())
        .collect();

    Ok(TorrentFile {
        announce: announce.to_string(),
        info_hash,
        piece_length,
        pieces,
        name,
        length,
        files,
    })
}

pub fn parse(data: &[u8]) -> Result<TorrentFile, String> {
    let (decoded, _) = crate::core::bencode::decode(data)?;

    let root = match decoded {
        BencodeValue::Dict(d) => d,
        _ => return Err("torrent file root has to be a dict".into()),
    };

    let announce = get_string(&root, "announce")?;

    let info = match root.get("info".as_bytes()) {
        Some(BencodeValue::Dict(d)) => d,
        _ => return Err("missing info dict".into()),
    };

    let info_value = BencodeValue::Dict(info.clone());
    let info_bytes = info_value.encode();
    from_info_bytes(&info_bytes, &announce, None)
}

pub fn load_bytes(source: &str) -> Result<Vec<u8>, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        use std::io::Read;
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .build();
        let agent = ureq::Agent::new_with_config(config);

        let response = agent.get(source)
            .header("User-Agent", "torr/0.1.0")
            .call()
            .map_err(|e| format!("failed to fetch torrent from '{source}': {e}"))?;
        let mut body = Vec::new();
        response
            .into_body()
            .into_reader()
            .read_to_end(&mut body)
            .map_err(|e| format!("failed to read torrent response: {e}"))?;
        Ok(body)
    } else {
        std::fs::read(source).map_err(|e| format!("failed to read torrent file '{source}': {e}"))
    }
}

pub fn load_source(source: &str) -> Result<TorrentFile, String> {
    if source.starts_with("magnet:") {
        crate::core::magnet::fetch_torrent(source)
    } else {
        let data = load_bytes(source)?;
        parse(&data)
    }
}

fn get_string(dict: &BTreeMap<Vec<u8>, BencodeValue>, key: &str) -> Result<String, String> {
    match dict.get(key.as_bytes()) {
        Some(BencodeValue::Bytes(b)) => String::from_utf8(b.clone()).map_err(|_| "bad utf8".into()),
        _ => Err(format!("missing or bad field: {key}")),
    }
}

fn get_int(dict: &BTreeMap<Vec<u8>, BencodeValue>, key: &str) -> Result<i64, String> {
    match dict.get(key.as_bytes()) {
        Some(BencodeValue::Int(i)) => Ok(*i),
        _ => Err(format!("missing or bad field: {key}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_torrent_correct_hash() {
        let data = std::fs::read("test_data/ubuntu.torrent").unwrap();
        let torrent = parse(&data).unwrap();

        let hash_hex: String = torrent.info_hash.iter()
            .map(|b| format!("{:02x}", b))
            .collect();

        assert_eq!(hash_hex, "dafc8c076ca2f3ed376eeae7c76a0d6be2415c45");
        assert_eq!(torrent.name, "ubuntu-26.04-desktop-amd64.iso");
        assert_eq!(torrent.piece_length, 256 * 1024); // 256 KiB in bytes
        assert_eq!(torrent.pieces.len(), 24868);
        assert_eq!(torrent.piece_size(0), 256 * 1024);
        assert_eq!(torrent.piece_size(24867), (torrent.length % (256 * 1024)) as usize);
    }

    #[test]
    fn from_info_bytes_creates_valid_torrent_file() {
        let mut info_dict = BTreeMap::new();
        info_dict.insert(b"name".to_vec(), BencodeValue::Bytes(b"sample.iso".to_vec()));
        info_dict.insert(b"piece length".to_vec(), BencodeValue::Int(16384));
        info_dict.insert(b"pieces".to_vec(), BencodeValue::Bytes(vec![0x42u8; 20]));
        info_dict.insert(b"length".to_vec(), BencodeValue::Int(16384));

        let encoded_info = BencodeValue::Dict(info_dict).encode();
        let torrent = from_info_bytes(&encoded_info, "http://tracker.test/announce", None).unwrap();

        assert_eq!(torrent.name, "sample.iso");
        assert_eq!(torrent.length, 16384);
        assert_eq!(torrent.piece_length, 16384);
        assert_eq!(torrent.pieces.len(), 1);
        assert_eq!(torrent.announce, "http://tracker.test/announce");
    }

    #[test]
    fn load_source_fetches_from_http_url() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let torrent_data = std::fs::read("test_data/ubuntu.torrent").unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let data_clone = torrent_data.clone();

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                if line == "\r\n" || line == "\n" {
                    break;
                }
                line.clear();
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/x-bittorrent\r\n\r\n",
                data_clone.len()
            );
            socket.write_all(response.as_bytes()).unwrap();
            socket.write_all(&data_clone).unwrap();
        });

        let url = format!("http://127.0.0.1:{}/ubuntu.torrent", addr.port());
        let torrent = load_source(&url).unwrap();
        assert_eq!(torrent.name, "ubuntu-26.04-desktop-amd64.iso");

        server.join().unwrap();
    }

    #[test]
    fn parses_multi_file_torrent_correctly() {
        let mut info_dict = BTreeMap::new();
        info_dict.insert(b"name".to_vec(), BencodeValue::Bytes(b"my_dataset".to_vec()));
        info_dict.insert(b"piece length".to_vec(), BencodeValue::Int(32));
        info_dict.insert(b"pieces".to_vec(), BencodeValue::Bytes(vec![0u8; 20]));

        let mut file1 = BTreeMap::new();
        file1.insert(b"length".to_vec(), BencodeValue::Int(20));
        file1.insert(
            b"path".to_vec(),
            BencodeValue::List(vec![
                BencodeValue::Bytes(b"docs".to_vec()),
                BencodeValue::Bytes(b"readme.txt".to_vec()),
            ]),
        );

        let mut file2 = BTreeMap::new();
        file2.insert(b"length".to_vec(), BencodeValue::Int(12));
        file2.insert(
            b"path".to_vec(),
            BencodeValue::List(vec![BencodeValue::Bytes(b"data.bin".to_vec())]),
        );

        info_dict.insert(
            b"files".to_vec(),
            BencodeValue::List(vec![BencodeValue::Dict(file1), BencodeValue::Dict(file2)]),
        );

        let mut root = BTreeMap::new();
        root.insert(b"announce".to_vec(), BencodeValue::Bytes(b"http://tracker.example.com".to_vec()));
        root.insert(b"info".to_vec(), BencodeValue::Dict(info_dict));

        let encoded = BencodeValue::Dict(root).encode();
        let parsed = parse(&encoded).unwrap();

        assert!(parsed.is_multi_file());
        assert_eq!(parsed.name, "my_dataset");
        assert_eq!(parsed.length, 32);
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(parsed.files[0].path, vec!["docs", "readme.txt"]);
        assert_eq!(parsed.files[0].length, 20);
        assert_eq!(parsed.files[1].path, vec!["data.bin"]);
        assert_eq!(parsed.files[1].length, 12);
    }
}
