//! Read Minecraft `servers.dat` (uncompressed or gzip NBT).
//!
//! Format: root compound with a `servers` list of compounds
//! (`name`, `ip`, optional `icon`, `acceptTextures`).

use crate::level_dat::{self, NbtTag};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
    pub icon: Option<String>,
    pub accept_textures: Option<i8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPingResult {
    pub address: String,
    pub online: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
    /// Players online from the Status Response. `None` when the server
    /// answered TCP but not the Java status handshake (Bedrock, filtered).
    #[serde(default)]
    pub players_online: Option<u32>,
    /// Max players from the Status Response (`None` — see above).
    #[serde(default)]
    pub players_max: Option<u32>,
}

/// Lists servers from `project_dir/servers.dat`. Empty vec if missing.
pub fn list_servers(project_dir: &Path) -> Result<Vec<ServerEntry>, String> {
    let path = project_dir.join("servers.dat");
    if !path.is_file() {
        return Ok(vec![]);
    }
    let tag = level_dat::read_nbt_auto(&path)?;
    Ok(extract_servers(&tag))
}

fn extract_servers(tag: &NbtTag) -> Vec<ServerEntry> {
    let entries = match tag {
        NbtTag::Compound(e) => e,
        _ => return vec![],
    };
    let list = entries
        .iter()
        .find(|(k, _)| k == "servers")
        .and_then(|(_, v)| match v {
            NbtTag::List(items) => Some(items),
            _ => None,
        });
    let Some(items) = list else {
        return vec![];
    };

    let mut out = Vec::new();
    for item in items {
        let NbtTag::Compound(fields) = item else {
            continue;
        };
        let name = get_string(fields, "name").unwrap_or_else(|| "Minecraft Server".into());
        let address = get_string(fields, "ip").unwrap_or_default();
        if address.is_empty() {
            continue;
        }
        let icon = get_string(fields, "icon");
        let accept_textures = get_byte(fields, "acceptTextures");
        out.push(ServerEntry {
            name,
            address,
            icon,
            accept_textures,
        });
    }
    out
}

fn get_string(fields: &[(String, NbtTag)], key: &str) -> Option<String> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| match v {
            NbtTag::String(s) => Some(s.clone()),
            _ => None,
        })
}

fn get_byte(fields: &[(String, NbtTag)], key: &str) -> Option<i8> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| match v {
            NbtTag::Byte(b) => Some(*b),
            _ => None,
        })
}

/// Java Edition Server List Ping: TCP connect + handshake/status round-trip.
/// Returns latency plus player counts; when TCP connects but the status
/// handshake fails (Bedrock server, filtered MOTD), the server is still
/// reported online with `players_* = None`.
pub fn ping_server_address(address: &str) -> ServerPingResult {
    let addr = address.trim();
    if addr.is_empty() {
        return ServerPingResult {
            address: address.to_string(),
            online: false,
            latency_ms: None,
            error: Some("empty address".into()),
            players_online: None,
            players_max: None,
        };
    }

    let (host, port) = split_host_port(addr);
    let with_port = if addr.contains(':') {
        addr.to_string()
    } else {
        format!("{addr}:25565")
    };

    let start = Instant::now();
    match with_port.to_socket_addrs() {
        Ok(mut iter) => {
            let Some(sock): Option<SocketAddr> = iter.next() else {
                return ServerPingResult {
                    address: address.to_string(),
                    online: false,
                    latency_ms: None,
                    error: Some("could not resolve host".into()),
                    players_online: None,
                    players_max: None,
                };
            };
            match TcpStream::connect_timeout(&sock, Duration::from_secs(3)) {
                Ok(stream) => {
                    let tcp_ms = start.elapsed().as_millis() as u64;
                    match query_status_players(&stream, &host, port) {
                        Some((online_count, max_count)) => ServerPingResult {
                            address: address.to_string(),
                            online: true,
                            latency_ms: Some(start.elapsed().as_millis() as u64),
                            error: None,
                            players_online: Some(online_count),
                            players_max: Some(max_count),
                        },
                        None => ServerPingResult {
                            address: address.to_string(),
                            online: true,
                            latency_ms: Some(tcp_ms),
                            error: None,
                            players_online: None,
                            players_max: None,
                        },
                    }
                }
                Err(e) => ServerPingResult {
                    address: address.to_string(),
                    online: false,
                    latency_ms: None,
                    error: Some(e.to_string()),
                    players_online: None,
                    players_max: None,
                },
            }
        }
        Err(e) => ServerPingResult {
            address: address.to_string(),
            online: false,
            latency_ms: None,
            error: Some(e.to_string()),
            players_online: None,
            players_max: None,
        },
    }
}

/// Split `host[:port]` for the handshake packet. Handles `[v6]:port`;
/// a bare value without a numeric `:port` tail keeps port 25565.
fn split_host_port(addr: &str) -> (String, u16) {
    let addr = addr.trim();
    if let Some(rest) = addr.strip_prefix('[') {
        if let Some((host, port_part)) = rest.split_once("]:") {
            if let Ok(port) = port_part.parse::<u16>() {
                return (host.to_string(), port);
            }
        }
        let host = addr.trim_start_matches('[').trim_end_matches(']');
        return (host.to_string(), 25565);
    }
    if addr.chars().filter(|c| *c == ':').count() == 1 {
        if let Some((host, port_part)) = addr.rsplit_once(':') {
            if let Ok(port) = port_part.parse::<u16>() {
                return (host.to_string(), port);
            }
        }
    }
    (addr.to_string(), 25565)
}

/// Handshake (next state = status) + status request, then parse the
/// Status Response JSON (`players.online/max`). `None` on any protocol
/// failure — the caller falls back to the TCP-only result.
fn query_status_players(stream: &TcpStream, host: &str, port: u16) -> Option<(u32, u32)> {
    use std::io::{Read, Write};
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok()?;

    // Handshake packet (id 0x00): protocol, host, port, next state = 1 (status).
    // Servers answer status requests regardless of the protocol version.
    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0);
    write_varint(&mut handshake, 767);
    write_mc_string(&mut handshake, host);
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, 1);

    let mut frame = Vec::new();
    write_varint(&mut frame, handshake.len() as i32);
    frame.extend_from_slice(&handshake);
    // Status request packet: length 1, id 0x00, no fields.
    frame.push(1);
    frame.push(0);
    let mut writer = stream;
    writer.write_all(&frame).ok()?;

    let mut reader = stream;
    let packet_len = read_varint(&mut reader)?;
    if packet_len <= 0 || packet_len > 2_000_000 {
        return None;
    }
    if read_varint(&mut reader)? != 0 {
        return None;
    }
    let json_len = read_varint(&mut reader)?;
    if json_len <= 0 || json_len > 1_000_000 {
        return None;
    }
    let mut buf = vec![0u8; json_len as usize];
    reader.read_exact(&mut buf).ok()?;
    parse_status_players(&buf)
}

fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
            buf.push(b);
        } else {
            buf.push(b);
            break;
        }
    }
}

fn write_mc_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

fn read_varint(reader: &mut impl std::io::Read) -> Option<i32> {
    let mut num: i32 = 0;
    let mut shift = 0u32;
    let mut byte = [0u8; 1];
    loop {
        reader.read_exact(&mut byte).ok()?;
        let b = byte[0];
        num |= ((b & 0x7f) as i32) << shift;
        if b & 0x80 == 0 {
            return Some(num);
        }
        shift += 7;
        if shift >= 35 {
            return None;
        }
    }
}

fn parse_status_players(json_bytes: &[u8]) -> Option<(u32, u32)> {
    let v: serde_json::Value = serde_json::from_slice(json_bytes).ok()?;
    let players = v.get("players")?;
    let online = u32::try_from(players.get("online")?.as_u64()?).ok()?;
    let max = u32::try_from(players.get("max")?.as_u64()?).ok()?;
    Some((online, max))
}

/// Append a server to servers.dat (creates file if missing).
pub fn add_server(
    project_dir: &Path,
    name: &str,
    address: &str,
) -> Result<Vec<ServerEntry>, String> {
    let name = name.trim();
    let address = address.trim();
    if name.is_empty() || address.is_empty() {
        return Err("name and address required".into());
    }
    let mut servers = list_servers(project_dir)?;
    if servers
        .iter()
        .any(|s| s.address.eq_ignore_ascii_case(address))
    {
        return Err("server already exists".into());
    }
    servers.push(ServerEntry {
        name: name.to_string(),
        address: address.to_string(),
        icon: None,
        accept_textures: None,
    });
    write_servers(project_dir, &servers)?;
    Ok(servers)
}

/// Remove server by address (case-insensitive).
pub fn remove_server(project_dir: &Path, address: &str) -> Result<Vec<ServerEntry>, String> {
    let mut servers = list_servers(project_dir)?;
    let before = servers.len();
    servers.retain(|s| !s.address.eq_ignore_ascii_case(address));
    if servers.len() == before {
        return Err("server not found".into());
    }
    write_servers(project_dir, &servers)?;
    Ok(servers)
}

fn write_servers(project_dir: &Path, servers: &[ServerEntry]) -> Result<(), String> {
    let path = project_dir.join("servers.dat");
    let bytes = encode_servers_nbt(servers)?;
    let mut f = fs::File::create(&path).map_err(|e| e.to_string())?;
    f.write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(())
}

fn encode_servers_nbt(servers: &[ServerEntry]) -> Result<Vec<u8>, String> {
    // Uncompressed root compound named "" with list "servers".
    let mut buf = Vec::new();
    buf.push(10); // TAG_Compound
    write_string(&mut buf, "");
    // TAG_List "servers"
    buf.push(9);
    write_string(&mut buf, "servers");
    buf.push(10); // list of compounds
    buf.extend_from_slice(&(servers.len() as i32).to_be_bytes());
    for s in servers {
        // compound fields
        buf.push(8); // string name
        write_string(&mut buf, "name");
        write_string(&mut buf, &s.name);
        buf.push(8); // string ip
        write_string(&mut buf, "ip");
        write_string(&mut buf, &s.address);
        if let Some(ref icon) = s.icon {
            buf.push(8);
            write_string(&mut buf, "icon");
            write_string(&mut buf, icon);
        }
        if let Some(at) = s.accept_textures {
            buf.push(1);
            write_string(&mut buf, "acceptTextures");
            buf.push(at as u8);
        }
        buf.push(0); // TAG_End of compound
    }
    buf.push(0); // TAG_End of root
    Ok(buf)
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    buf.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip() {
        for v in [0, 1, 2, 127, 128, 255, 767, 2147483647] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            let back = read_varint(&mut buf.as_slice()).unwrap();
            assert_eq!(back, v, "varint roundtrip for {v}");
        }
        assert_eq!(read_varint(&mut [0x80u8, 0x80, 0x80, 0x80, 0x80].as_slice()), None);
    }

    #[test]
    fn split_host_port_cases() {
        assert_eq!(split_host_port("mc.hypixel.net"), ("mc.hypixel.net".into(), 25565));
        assert_eq!(split_host_port("play.example.com:25570"), ("play.example.com".into(), 25570));
        assert_eq!(split_host_port("127.0.0.1:25565"), ("127.0.0.1".into(), 25565));
        assert_eq!(split_host_port("[::1]:25566"), ("::1".into(), 25566));
        assert_eq!(split_host_port("::1"), ("::1".into(), 25565));
        assert_eq!(split_host_port("  example.com  "), ("example.com".into(), 25565));
    }

    #[test]
    fn parse_status_players_cases() {
        let json = br#"{"description":{"text":"Hi"},"players":{"max":100,"online":7,"sample":[]},"version":{"name":"1.21.1","protocol":767}}"#;
        assert_eq!(parse_status_players(json), Some((7, 100)));
        assert_eq!(parse_status_players(br#"{"players":{"max":20}}"#), None);
        assert_eq!(parse_status_players(b"not json"), None);
    }

    #[test]
    fn roundtrip_servers_dat() {
        let dir = std::env::temp_dir().join(format!("tuffbox_srv_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        add_server(&dir, "Local", "127.0.0.1:25565").unwrap();
        add_server(&dir, "Hypixel", "mc.hypixel.net").unwrap();
        let listed = list_servers(&dir).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "Local");
        remove_server(&dir, "127.0.0.1:25565").unwrap();
        assert_eq!(list_servers(&dir).unwrap().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
