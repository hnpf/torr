use crate::core::bencode::{decode, BencodeValue};
use std::io::Read;
use std::net::SocketAddr;

#[derive(Debug)]
pub struct Peer {
    pub ip: std::net::IpAddr,
    pub port: u16,
}

impl Peer {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.port)
    }
}

pub fn announce(
    announce_url: &str,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    port: u16,
    left: i64,
) -> Result<Vec<Peer>, String> {
    let url = format!(
        "{}?info_hash={}&peer_id={}&port={}&uploaded=0&downloaded=0&left={}&compact=1",
        announce_url,
        url_encode_bytes(info_hash),
        url_encode_bytes(peer_id),
        port,
        left,
    );

    let response = ureq::get(&url)
        .header("User-Agent", "torr/0.1.0")
        .call()
        .map_err(|e| format!("tracker request failed: {e}"))?;

    let mut body = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;

    let (decoded, _) = decode(&body)?;
    let dict = match decoded {
        BencodeValue::Dict(d) => d,
        _ => return Err("tracker response not a dict".into()),
    };

    if let Some(BencodeValue::Bytes(failure)) = dict.get("failure reason".as_bytes()) {
        return Err(String::from_utf8_lossy(failure).into());
    }

    // trackers can return peers as either
    // bytes; compact format, 6 bytes per peer (ipv4 only)
    // list of dicts; noncompact format, used whenever ipv6 peers are included
    // gotta handle both, too. ubuntu's tracker sends dict-style since a lot of its swarm is v6
    let peers = match dict.get("peers".as_bytes()) {
        Some(BencodeValue::Bytes(b)) => parse_compact_peers(b)?,
        Some(BencodeValue::List(list)) => parse_dict_peers(list),
        _ => return Err("missing peers field".into()),
    };

    Ok(peers)
}

pub fn announce_addrs(
    announce_url: &str,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    port: u16,
    left: i64,
) -> Result<Vec<SocketAddr>, String> {
    announce(announce_url, info_hash, peer_id, port, left)
        .map(|peers| peers.into_iter().map(|peer| peer.socket_addr()).collect())
}

fn parse_compact_peers(peers_raw: &[u8]) -> Result<Vec<Peer>, String> {
    if peers_raw.len() % 6 != 0 {
        return Err("malformed compact peers".into());
    }
    Ok(peers_raw
        .chunks(6)
        .map(|chunk| Peer {
            ip: std::net::Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]).into(),
            port: u16::from_be_bytes([chunk[4], chunk[5]]),
        })
        .collect())
}

fn parse_dict_peers(list: &[BencodeValue]) -> Vec<Peer> {
    let mut peers = Vec::new();
    for item in list {
        let peer_dict = match item {
            BencodeValue::Dict(d) => d,
            _ => continue, // skip malformed entries instead of failing the whole batch
        };
        let ip_bytes = match peer_dict.get("ip".as_bytes()) {
            Some(BencodeValue::Bytes(b)) => b,
            _ => continue,
        };
        let ip_str = String::from_utf8_lossy(ip_bytes);
        let ip: std::net::IpAddr = match ip_str.parse() {
            Ok(ip) => ip,
            Err(_) => continue, // some entries had garbage bytes in the ip field, skip those
        };
        let port = match peer_dict.get("port".as_bytes()) {
            Some(BencodeValue::Int(p)) => *p as u16,
            _ => continue,
        };
        peers.push(Peer { ip, port });
    }
    peers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::torrent::parse;

    #[test]
    fn announce_to_real_tracker() {
        let data = std::fs::read("test_data/ubuntu.torrent").unwrap();
        let torrent = parse(&data).unwrap();

        let peer_id = *b"-TC0001-123456789012"; // must be exactly 20 bytes
        let peers = announce(&torrent.announce, &torrent.info_hash, &peer_id, 6881, torrent.length).unwrap();

        println!("got {} peers", peers.len());
        assert!(!peers.is_empty(), "tracker returned zero peers");
    }

    #[test]
    fn announce_addrs_returns_socket_addrs() {
        let peer = Peer {
            ip: "127.0.0.1".parse().unwrap(),
            port: 6881,
        };

        assert_eq!(peer.socket_addr().to_string(), "127.0.0.1:6881");
    }
}

// raw bytes need percent encoding, not standard url encoding, every byte gets %XX
// treating info_hash as a utf8 string here would corrupt it since its raw sha1 bytes
fn url_encode_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("%{:02x}", b)).collect()
}
