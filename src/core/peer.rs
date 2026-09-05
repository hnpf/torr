#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};

pub const PROTOCOL_STR: &str = "BitTorrent protocol";
pub const PROTOCOL_LEN: u8 = 19;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub reserved: [u8; 8],
}

impl Handshake {
    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        let mut reserved = [0u8; 8];
        reserved[5] |= 0x10;
        Self {
            info_hash,
            peer_id,
            reserved,
        }
    }

    pub fn supports_extended(&self) -> bool {
        (self.reserved[5] & 0x10) != 0
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + PROTOCOL_LEN as usize + 8 + 20 + 20);
        buf.push(PROTOCOL_LEN);
        buf.extend_from_slice(PROTOCOL_STR.as_bytes());
        buf.extend_from_slice(&self.reserved);
        buf.extend_from_slice(&self.info_hash);
        buf.extend_from_slice(&self.peer_id);
        buf
    }

    pub fn decode(data: &[u8]) -> Result<(Self, &[u8]), String> {
        if data.is_empty() {
            return Err("handshake too short".into());
        }

        let pstrlen = data[0];
        if pstrlen != PROTOCOL_LEN {
            return Err(format!("unexpected protocol length: {}", pstrlen));
        }

        let required = 1 + PROTOCOL_LEN as usize + 8 + 20 + 20;
        if data.len() < required {
            return Err("handshake too short".into());
        }

        let protocol = &data[1..1 + PROTOCOL_LEN as usize];
        if protocol != PROTOCOL_STR.as_bytes() {
            return Err("unexpected protocol string".into());
        }

        let reserved_start = 1 + PROTOCOL_LEN as usize;
        let info_start = reserved_start + 8;
        let peer_start = info_start + 20;

        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&data[reserved_start..info_start]);

        let mut info_hash = [0u8; 20];
        info_hash.copy_from_slice(&data[info_start..peer_start]);

        let mut peer_id = [0u8; 20];
        peer_id.copy_from_slice(&data[peer_start..peer_start + 20]);

        Ok((
            Handshake {
                info_hash,
                peer_id,
                reserved,
            },
            &data[peer_start + 20..],
        ))
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), String> {
        writer.write_all(&self.encode()).map_err(|e| e.to_string())
    }

    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, String> {
        let mut header = [0u8; 1];
        reader.read_exact(&mut header).map_err(|e| e.to_string())?;
        if header[0] != PROTOCOL_LEN {
            return Err(format!("unexpected protocol length: {}", header[0]));
        }

        let mut buf = vec![0u8; (PROTOCOL_LEN as usize) + 8 + 20 + 20];
        reader.read_exact(&mut buf).map_err(|e| e.to_string())?;

        let protocol = &buf[..PROTOCOL_LEN as usize];
        if protocol != PROTOCOL_STR.as_bytes() {
            return Err("unexpected protocol string".into());
        }

        let reserved_start = PROTOCOL_LEN as usize;
        let info_start = reserved_start + 8;
        let peer_start = info_start + 20;

        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&buf[reserved_start..info_start]);

        let mut info_hash = [0u8; 20];
        info_hash.copy_from_slice(&buf[info_start..peer_start]);

        let mut peer_id = [0u8; 20];
        peer_id.copy_from_slice(&buf[peer_start..peer_start + 20]);

        Ok(Handshake {
            info_hash,
            peer_id,
            reserved,
        })
    }

    pub fn verify_info_hash(&self, expected_info_hash: &[u8; 20]) -> Result<(), String> {
        if &self.info_hash != expected_info_hash {
            return Err("handshake info_hash does not match expected torrent".into());
        }
        Ok(())
    }
}

pub struct PeerConnection {
    pub stream: TcpStream,
    pub remote_handshake: Handshake,
}

impl PeerConnection {
    pub fn connect<A: ToSocketAddrs>(addr: A, info_hash: [u8; 20], peer_id: [u8; 20]) -> Result<Self, String> {
        let mut stream = TcpStream::connect(addr).map_err(|e| e.to_string())?;
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));
        let outgoing = Handshake::new(info_hash, peer_id);
        outgoing.write_to(&mut stream)?;

        let remote_handshake = Handshake::read_from(&mut stream)?;
        remote_handshake.verify_info_hash(&info_hash)?;

        Ok(PeerConnection { stream, remote_handshake })
    }

    pub fn connect_timeout(
        addr: SocketAddr,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
        timeout: std::time::Duration,
    ) -> Result<Self, String> {
        Self::connect_bound(addr, info_hash, peer_id, None, timeout)
    }

    pub fn connect_bound(
        addr: SocketAddr,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
        bind_ip: Option<std::net::IpAddr>,
        timeout: std::time::Duration,
    ) -> Result<Self, String> {
        let mut stream = crate::core::vpn::connect_bound(addr, bind_ip, timeout)?;
        let outgoing = Handshake::new(info_hash, peer_id);
        outgoing.write_to(&mut stream)?;

        let remote_handshake = Handshake::read_from(&mut stream)?;
        remote_handshake.verify_info_hash(&info_hash)?;

        Ok(PeerConnection { stream, remote_handshake })
    }

    pub fn connect_addrs(addrs: &[SocketAddr], info_hash: [u8; 20], peer_id: [u8; 20]) -> Result<Self, String> {
        let mut last_error: Option<String> = None;
        for &addr in addrs {
            match PeerConnection::connect(addr, info_hash, peer_id) {
                Ok(connection) => return Ok(connection),
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or_else(|| "no peer addresses provided".into()))
    }

    pub fn connect_via_tracker(
        announce_url: &str,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
        port: u16,
        left: i64,
    ) -> Result<Self, String> {
        let addrs = crate::core::tracker::announce_addrs(announce_url, &info_hash, &peer_id, port, left)?;
        if addrs.is_empty() {
            return Err("tracker returned no peer addresses".into());
        }
        PeerConnection::connect_addrs(&addrs, info_hash, peer_id)
    }

    pub fn send_message(&mut self, message: &Message) -> Result<(), String> {
        message.write_to(&mut self.stream)
    }

    pub fn receive_message(&mut self) -> Result<Message, String> {
        Message::read_from(&mut self.stream)
    }

    pub fn receive_bitfield(&mut self) -> Result<Vec<u8>, String> {
        loop {
            match self.receive_message()? {
                Message::Bitfield(bits) => return Ok(bits),
                Message::KeepAlive => continue,
                other => return Err(format!("expected bitfield, got {other:?}")),
            }
        }
    }

    pub fn send_request(&mut self, index: u32, begin: u32, length: u32) -> Result<(), String> {
        self.send_message(&Message::Request { index, begin, length })
    }

    pub fn receive_piece(&mut self) -> Result<(u32, u32, Vec<u8>), String> {
        loop {
            match self.receive_message()? {
                Message::Piece { index, begin, block } => return Ok((index, begin, block)),
                Message::KeepAlive => continue,
                other => return Err(format!("expected piece, got {other:?}")),
            }
        }
    }

    pub fn send_interested(&mut self) -> Result<(), String> {
        self.send_message(&Message::Interested)
    }

    pub fn send_unchoke(&mut self) -> Result<(), String> {
        self.send_message(&Message::Unchoke)
    }

    pub fn send_keepalive(&mut self) -> Result<(), String> {
        self.send_message(&Message::KeepAlive)
    }

    pub fn send_extended(&mut self, ext_id: u8, payload: Vec<u8>) -> Result<(), String> {
        self.send_message(&Message::Extended { ext_id, payload })
    }
}

pub struct PeerState {
    pub connection: PeerConnection,
    pub bitfield: Vec<u8>,
    pub choked: bool,
    pub remote_interested: bool,
    pub local_interested: bool,
}

impl PeerState {
    pub fn new(connection: PeerConnection) -> Self {
        Self {
            connection,
            bitfield: Vec::new(),
            choked: true,
            remote_interested: false,
            local_interested: false,
        }
    }

    pub fn receive_and_update(&mut self) -> Result<Message, String> {
        loop {
            let msg = self.connection.receive_message()?;
            match &msg {
                Message::KeepAlive => continue,
                Message::Choke => {
                    self.choked = true;
                    return Ok(msg);
                }
                Message::Unchoke => {
                    self.choked = false;
                    return Ok(msg);
                }
                Message::Interested => {
                    self.remote_interested = true;
                    return Ok(msg);
                }
                Message::NotInterested => {
                    self.remote_interested = false;
                    return Ok(msg);
                }
                Message::Bitfield(bits) => {
                    self.bitfield = bits.clone();
                    return Ok(msg);
                }
                Message::Have(index) => {
                    self.set_piece(*index as usize, true);
                    return Ok(msg);
                }
                _ => return Ok(msg),
            }
        }
    }

    pub fn wait_for_unchoke(&mut self) -> Result<(), String> {
        while self.choked {
            self.receive_and_update()?;
        }
        Ok(())
    }

    pub fn set_interested(&mut self) -> Result<(), String> {
        self.local_interested = true;
        self.connection.send_interested()
    }

    pub fn set_not_interested(&mut self) -> Result<(), String> {
        self.local_interested = false;
        self.connection.send_message(&Message::NotInterested)
    }

    pub fn request_block(&mut self, index: u32, begin: u32, length: u32) -> Result<Vec<u8>, String> {
        if !self.local_interested {
            self.set_interested()?;
        }
        self.connection.send_request(index, begin, length)?;
        loop {
            let msg = self.receive_and_update()?;
            match msg {
                Message::Piece { index: i, begin: b, block } => {
                    if i == index && b == begin {
                        return Ok(block);
                    }
                }
                Message::Choke => {
                    return Err("peer choked during transfer".into());
                }
                _ => {}
            }
        }
    }

    pub fn download_piece(&mut self, index: u32, piece_length: u32) -> Result<Vec<u8>, String> {
        if !self.local_interested {
            self.set_interested()?;
        }

        let ranges = crate::core::piece::block_ranges(piece_length as usize);
        let total_blocks = ranges.len();
        let mut blocks: HashMap<u32, Vec<u8>> = HashMap::with_capacity(total_blocks);

        let max_pipeline = 16.min(total_blocks);
        let mut requested_idx = 0;

        while requested_idx < max_pipeline && requested_idx < total_blocks {
            let r = &ranges[requested_idx];
            self.connection.send_request(index, r.begin, r.length)?;
            requested_idx += 1;
        }

        while blocks.len() < total_blocks {
            let msg = self.receive_and_update()?;
            match msg {
                Message::Piece { index: i, begin, block } if i == index => {
                    blocks.insert(begin, block);
                    if requested_idx < total_blocks {
                        let r = &ranges[requested_idx];
                        self.connection.send_request(index, r.begin, r.length)?;
                        requested_idx += 1;
                    }
                }
                Message::Choke => {
                    return Err("peer choked during transfer".into());
                }
                _ => {}
            }
        }

        crate::core::piece::assemble_piece(piece_length as usize, &blocks)
    }

    pub fn download_and_verify_piece(
        &mut self,
        index: u32,
        piece_length: u32,
        expected_hash: &[u8; 20],
    ) -> Result<Vec<u8>, String> {
        let piece = self.download_piece(index, piece_length)?;
        crate::core::piece::verify_piece(expected_hash, &piece)?;
        Ok(piece)
    }

    pub fn has_piece(&self, index: usize) -> bool {
        let byte = index / 8;
        let bit = 7 - (index % 8);
        self.bitfield.get(byte).map_or(false, |byte| (byte >> bit) & 1 == 1)
    }

    pub fn piece_count(&self) -> usize {
        self.bitfield.len() * 8
    }

    fn set_piece(&mut self, index: usize, value: bool) {
        let byte = index / 8;
        let bit = 7 - (index % 8);
        if self.bitfield.len() <= byte {
            self.bitfield.resize(byte + 1, 0);
        }
        if value {
            self.bitfield[byte] |= 1 << bit;
        } else {
            self.bitfield[byte] &= !(1 << bit);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request { index: u32, begin: u32, length: u32 },
    Piece { index: u32, begin: u32, block: Vec<u8> },
    Cancel { index: u32, begin: u32, length: u32 },
    Port(u16),
    Extended { ext_id: u8, payload: Vec<u8> },
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Message::KeepAlive => 0u32.to_be_bytes().to_vec(),
            Message::Choke => [1u32.to_be_bytes().as_ref(), &[0u8]].concat(),
            Message::Unchoke => [1u32.to_be_bytes().as_ref(), &[1u8]].concat(),
            Message::Interested => [1u32.to_be_bytes().as_ref(), &[2u8]].concat(),
            Message::NotInterested => [1u32.to_be_bytes().as_ref(), &[3u8]].concat(),
            Message::Have(piece_index) => {
                let mut buf = Vec::with_capacity(9);
                buf.extend_from_slice(&5u32.to_be_bytes());
                buf.push(4);
                buf.extend_from_slice(&piece_index.to_be_bytes());
                buf
            }
            Message::Bitfield(bits) => {
                let len = 1 + bits.len() as u32;
                let mut buf = Vec::with_capacity(4 + len as usize);
                buf.extend_from_slice(&len.to_be_bytes());
                buf.push(5);
                buf.extend_from_slice(bits);
                buf
            }
            Message::Request { index, begin, length } => {
                let mut buf = Vec::with_capacity(4 + 1 + 12);
                buf.extend_from_slice(&13u32.to_be_bytes());
                buf.push(6);
                buf.extend_from_slice(&index.to_be_bytes());
                buf.extend_from_slice(&begin.to_be_bytes());
                buf.extend_from_slice(&length.to_be_bytes());
                buf
            }
            Message::Piece { index, begin, block } => {
                let len = 9 + block.len() as u32;
                let mut buf = Vec::with_capacity(4 + len as usize);
                buf.extend_from_slice(&len.to_be_bytes());
                buf.push(7);
                buf.extend_from_slice(&index.to_be_bytes());
                buf.extend_from_slice(&begin.to_be_bytes());
                buf.extend_from_slice(block);
                buf
            }
            Message::Cancel { index, begin, length } => {
                let mut buf = Vec::with_capacity(4 + 1 + 12);
                buf.extend_from_slice(&13u32.to_be_bytes());
                buf.push(8);
                buf.extend_from_slice(&index.to_be_bytes());
                buf.extend_from_slice(&begin.to_be_bytes());
                buf.extend_from_slice(&length.to_be_bytes());
                buf
            }
            Message::Port(port) => {
                let mut buf = Vec::with_capacity(4 + 1 + 2);
                buf.extend_from_slice(&3u32.to_be_bytes());
                buf.push(9);
                buf.extend_from_slice(&port.to_be_bytes());
                buf
            }
            Message::Extended { ext_id, payload } => {
                let len = 2 + payload.len() as u32;
                let mut buf = Vec::with_capacity(4 + len as usize);
                buf.extend_from_slice(&len.to_be_bytes());
                buf.push(20);
                buf.push(*ext_id);
                buf.extend_from_slice(payload);
                buf
            }
        }
    }

    pub fn decode(data: &[u8]) -> Result<(Self, &[u8]), String> {
        if data.len() < 4 {
            return Err("message too short".into());
        }

        let len = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
        if data.len() < 4 + len {
            return Err("message length mismatch".into());
        }

        if len == 0 {
            return Ok((Message::KeepAlive, &data[4..]));
        }

        let id = data[4];
        let payload = &data[5..4 + len];

        let msg = match id {
            0 => Message::Choke,
            1 => Message::Unchoke,
            2 => Message::Interested,
            3 => Message::NotInterested,
            4 => {
                if payload.len() != 4 {
                    return Err("have payload wrong size".into());
                }
                let index = u32::from_be_bytes(payload.try_into().unwrap());
                Message::Have(index)
            }
            5 => Message::Bitfield(payload.to_vec()),
            6 => {
                if payload.len() != 12 {
                    return Err("request payload wrong size".into());
                }
                let index = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                let begin = u32::from_be_bytes(payload[4..8].try_into().unwrap());
                let length = u32::from_be_bytes(payload[8..12].try_into().unwrap());
                Message::Request { index, begin, length }
            }
            7 => {
                if payload.len() < 8 {
                    return Err("piece payload too short".into());
                }
                let index = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                let begin = u32::from_be_bytes(payload[4..8].try_into().unwrap());
                let block = payload[8..].to_vec();
                Message::Piece { index, begin, block }
            }
            8 => {
                if payload.len() != 12 {
                    return Err("cancel payload wrong size".into());
                }
                let index = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                let begin = u32::from_be_bytes(payload[4..8].try_into().unwrap());
                let length = u32::from_be_bytes(payload[8..12].try_into().unwrap());
                Message::Cancel { index, begin, length }
            }
            9 => {
                if payload.len() != 2 {
                    return Err("port payload wrong size".into());
                }
                let port = u16::from_be_bytes(payload.try_into().unwrap());
                Message::Port(port)
            }
            20 => {
                if payload.is_empty() {
                    return Err("extended message missing ext_id".into());
                }
                let ext_id = payload[0];
                let payload = payload[1..].to_vec();
                Message::Extended { ext_id, payload }
            }
            _ => return Err(format!("unknown message id: {}", id)),
        };

        Ok((msg, &data[4 + len..]))
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), String> {
        writer.write_all(&self.encode()).map_err(|e| e.to_string())
    }

    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, String> {
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf).map_err(|e| e.to_string())?;
        let len = u32::from_be_bytes(len_buf);

        if len == 0 {
            return Ok(Message::KeepAlive);
        }

        let mut payload = vec![0u8; len as usize];
        reader.read_exact(&mut payload).map_err(|e| e.to_string())?;
        let mut full = Vec::with_capacity(4 + payload.len());
        full.extend_from_slice(&len_buf);
        full.extend_from_slice(&payload);
        Message::decode(&full).map(|(msg, _)| msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_encode_decode_roundtrip() {
        let info_hash = [0x11u8; 20];
        let peer_id = *b"-TC0001-123456789012";
        let handshake = Handshake::new(info_hash, peer_id);
        let encoded = handshake.encode();
        let (decoded, rest) = Handshake::decode(&encoded).unwrap();

        assert_eq!(rest.len(), 0);
        assert_eq!(decoded.info_hash, info_hash);
        assert_eq!(decoded.peer_id, peer_id);
        assert!(decoded.supports_extended());
        assert_eq!(decoded.reserved[5], 0x10);
    }

    #[test]
    fn message_encode_decode_roundtrip() {
        let messages = vec![
            Message::KeepAlive,
            Message::Choke,
            Message::Unchoke,
            Message::Interested,
            Message::NotInterested,
            Message::Have(42),
            Message::Bitfield(vec![0b10101010, 0b11001100]),
            Message::Request { index: 1, begin: 0, length: 16384 },
            Message::Piece { index: 1, begin: 0, block: vec![1, 2, 3, 4] },
            Message::Cancel { index: 1, begin: 0, length: 16384 },
            Message::Port(6881),
            Message::Extended { ext_id: 1, payload: vec![1, 2, 3, 4] },
        ];

        let mut buf = Vec::new();
        for message in &messages {
            buf.extend_from_slice(&message.encode());
        }

        let mut rest = &buf[..];
        for expected in messages {
            let (decoded, remaining) = Message::decode(rest).unwrap();
            assert_eq!(decoded, expected);
            rest = remaining;
        }
        assert!(rest.is_empty());
    }

    #[test]
    fn peer_connection_performs_handshake_and_verifies_info_hash() {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0x22u8; 20];
        let peer_id = *b"-TC0001-123456789012";

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();
        });

        let connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        assert_eq!(connection.remote_handshake.info_hash, info_hash);
        assert_eq!(connection.remote_handshake.peer_id, peer_id);

        server.join().unwrap();
    }

    #[test]
    fn peer_connection_receive_message_after_handshake() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0x33u8; 20];
        let peer_id = *b"-TC0001-123456789012";

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();
            socket.write_all(&Message::Unchoke.encode()).unwrap();
        });

        let mut connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        let message = connection.receive_message().unwrap();
        assert_eq!(message, Message::Unchoke);

        server.join().unwrap();
    }

    #[test]
    fn peer_connection_receive_bitfield_after_handshake() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0x77u8; 20];
        let peer_id = *b"-TC0001-123456789012";
        let bitfield = vec![0b10101010, 0b11001100];
        let bitfield_for_server = bitfield.clone();

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();
            socket.write_all(&Message::Bitfield(bitfield_for_server).encode()).unwrap();
        });

        let mut connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        let received = connection.receive_bitfield().unwrap();
        assert_eq!(received, bitfield);

        server.join().unwrap();
    }

    #[test]
    fn peer_state_updates_bitfield_and_have_messages() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0x88u8; 20];
        let peer_id = *b"-TC0001-123456789012";

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();
            socket.write_all(&Message::Bitfield(vec![0b00000000]).encode()).unwrap();
            socket.write_all(&Message::Have(3).encode()).unwrap();
        });

        let connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        let mut state = PeerState::new(connection);
        let _ = state.receive_and_update().unwrap();
        let msg = state.receive_and_update().unwrap();

        assert_eq!(msg, Message::Have(3));
        assert!(!state.has_piece(0));
        assert!(state.has_piece(3));
        assert_eq!(state.piece_count(), 8);

        server.join().unwrap();
    }

    #[test]
    fn peer_state_request_block_roundtrip() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0x99u8; 20];
        let peer_id = *b"-TC0001-123456789012";
        let piece_index = 1u32;
        let begin = 0u32;
        let block = vec![0xde, 0xad, 0xbe, 0xef];
        let block_clone = block.clone();

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();

            let mut interested_buf = [0u8; 5];
            socket.read_exact(&mut interested_buf).unwrap();
            assert_eq!(&interested_buf[0..4], &[0, 0, 0, 1]);
            assert_eq!(interested_buf[4], 2);

            let mut request_buf = [0u8; 4 + 1 + 12];
            socket.read_exact(&mut request_buf).unwrap();
            assert_eq!(request_buf[4], 6);

            let piece_msg = Message::Piece { index: piece_index, begin, block: block_clone };
            socket.write_all(&piece_msg.encode()).unwrap();
        });

        let connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        let mut state = PeerState::new(connection);
        let result = state.request_block(piece_index, begin, block.len() as u32).unwrap();

        assert_eq!(result, block);
        server.join().unwrap();
    }

    #[test]
    fn peer_state_download_piece_pipeline_assembles_blocks() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0xaau8; 20];
        let peer_id = *b"-TC0001-123456789012";
        let piece_index = 0u32;
        let piece_length = 20_000u32;
        let part1 = vec![1u8; 16_384];
        let part2 = vec![2u8; 3_616];

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();

            let mut interested_buf = [0u8; 5];
            socket.read_exact(&mut interested_buf).unwrap();
            assert_eq!(interested_buf[4], 2);

            let mut request1 = [0u8; 4 + 1 + 12];
            socket.read_exact(&mut request1).unwrap();
            assert_eq!(request1[4], 6);
            let piece_msg1 = Message::Piece { index: piece_index, begin: 0, block: part1.clone() };
            socket.write_all(&piece_msg1.encode()).unwrap();

            let mut request2 = [0u8; 4 + 1 + 12];
            socket.read_exact(&mut request2).unwrap();
            assert_eq!(request2[4], 6);
            let piece_msg2 = Message::Piece { index: piece_index, begin: 16_384, block: part2.clone() };
            socket.write_all(&piece_msg2.encode()).unwrap();
        });

        let connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        let mut state = PeerState::new(connection);
        let result = state.download_piece(piece_index, piece_length).unwrap();

        assert_eq!(result.len(), 20_000);
        assert!(result[..16_384].iter().all(|&b| b == 1));
        assert!(result[16_384..].iter().all(|&b| b == 2));
        server.join().unwrap();
    }

    #[test]
    fn peer_state_download_and_verify_piece_roundtrip() {
        use sha1::Digest;
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0xbbu8; 20];
        let peer_id = *b"-TC0001-123456789012";
        let piece_index = 0u32;
        let piece_length = 4u32;
        let block = vec![0x10, 0x20, 0x30, 0x40];

        let mut hasher = sha1::Sha1::new();
        hasher.update(&block);
        let expected_hash: [u8; 20] = hasher.finalize().into();
        let block_clone = block.clone();

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();

            let mut interested_buf = [0u8; 5];
            socket.read_exact(&mut interested_buf).unwrap();
            assert_eq!(interested_buf[4], 2);

            let mut request_buf = [0u8; 4 + 1 + 12];
            socket.read_exact(&mut request_buf).unwrap();
            assert_eq!(request_buf[4], 6);
            let piece_msg = Message::Piece { index: piece_index, begin: 0, block: block_clone };
            socket.write_all(&piece_msg.encode()).unwrap();
        });

        let connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        let mut state = PeerState::new(connection);
        let result = state.download_and_verify_piece(piece_index, piece_length, &expected_hash).unwrap();

        assert_eq!(result, block);
        server.join().unwrap();
    }

    #[test]
    fn peer_connection_send_keepalive_writes_zero_length_message() {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0x44u8; 20];
        let peer_id = *b"-TC0001-123456789012";

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();

            let mut keepalive = [0u8; 4];
            socket.read_exact(&mut keepalive).unwrap();
            assert_eq!(keepalive, [0, 0, 0, 0]);
        });

        let mut connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        connection.send_keepalive().unwrap();

        server.join().unwrap();
    }

    #[test]
    fn peer_connection_connect_addrs_tries_multiple_addresses() {
        use std::io::Read;
        use std::net::{SocketAddr, TcpListener};

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0x55u8; 20];
        let peer_id = *b"-TC0001-123456789012";

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();
        });

        let addrs = vec!["127.0.0.1:0".parse::<SocketAddr>().unwrap(), addr];
        let connection = PeerConnection::connect_addrs(&addrs, info_hash, peer_id).unwrap();
        assert_eq!(connection.remote_handshake.info_hash, info_hash);

        server.join().unwrap();
    }

    #[test]
    fn peer_connection_connect_via_tracker_uses_tracker_peer_addresses() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let tracker_addr = listener.local_addr().unwrap();
        let peer_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let peer_addr = peer_listener.local_addr().unwrap();

        let info_hash = [0x66u8; 20];
        let peer_id = *b"-TC0001-123456789012";
        let announce_url = format!("http://127.0.0.1:{}/announce", tracker_addr.port());

        let tracker_server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
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
            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();
        });

        let connection = PeerConnection::connect_via_tracker(&announce_url, info_hash, peer_id, 6881, 0).unwrap();
        assert_eq!(connection.remote_handshake.info_hash, info_hash);

        tracker_server.join().unwrap();
        peer_server.join().unwrap();
    }

    #[test]
    fn peer_state_wait_for_unchoke_blocks_until_unchoke_message() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let info_hash = [0xccu8; 20];
        let peer_id = *b"-TC0001-123456789012";

        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = vec![0u8; 1 + PROTOCOL_LEN as usize + 8 + 20 + 20];
            socket.read_exact(&mut buffer).unwrap();

            let response = Handshake::new(info_hash, peer_id);
            socket.write_all(&response.encode()).unwrap();

            std::thread::sleep(std::time::Duration::from_millis(10));
            socket.write_all(&Message::Unchoke.encode()).unwrap();
        });

        let connection = PeerConnection::connect(addr, info_hash, peer_id).unwrap();
        let mut state = PeerState::new(connection);
        assert!(state.choked);
        state.wait_for_unchoke().unwrap();
        assert!(!state.choked);

        server.join().unwrap();
    }
}
