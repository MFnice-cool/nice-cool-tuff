//! Level.dat reader for Minecraft world previews.
//!
//! Reads NBT-compressed level.dat to extract world metadata:
//! name, seed, game type, version, play time, spawn coordinates.
//! Uses GZip + basic NBT binary parsing (no external NBT crate needed
//! for the small subset of tags we care about).

use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldInfo {
    pub name: String,
    pub seed: i64,
    pub game_type: String,
    pub version_name: String,
    pub last_played: u64,
    pub time: u64,
    pub spawn_x: i32,
    pub spawn_y: i32,
    pub spawn_z: i32,
    pub difficulty: String,
    pub hardcore: bool,
    pub cheats_enabled: bool,
    pub size_bytes: u64,
    pub size_formatted: String,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum NbtTag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    ByteArray(Vec<u8>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    List(Vec<NbtTag>),
    Compound(Vec<(String, NbtTag)>),
    End,
}

/// Reads NBT from disk — tries gzip first, then uncompressed.
pub fn read_nbt_auto(path: &std::path::Path) -> Result<NbtTag, String> {
    let raw = std::fs::read(path).map_err(|e| e.to_string())?;
    if raw.first() == Some(&0x1f) && raw.get(1) == Some(&0x8b) {
        let mut decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(&raw));
        let mut buf = Vec::new();
        decoder.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        return parse_nbt_binary(&buf);
    }
    // Uncompressed (servers.dat) or already decompressed.
    if let Ok(tag) = parse_nbt_binary(&raw) {
        return Ok(tag);
    }
    // Fallback: try gzip anyway (some tools wrap oddly).
    let mut decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(&raw));
    let mut buf = Vec::new();
    match decoder.read_to_end(&mut buf) {
        Ok(_) => parse_nbt_binary(&buf),
        Err(_) => Err("failed to parse NBT (gzip or raw)".into()),
    }
}

/// Reads and decompresses a gzip-compressed NBT file.
fn read_nbt(path: &std::path::Path) -> Result<NbtTag, String> {
    read_nbt_auto(path)
}

/// Minimal NBT binary parser — only handles the tag types used in level.dat.
fn parse_nbt_binary(data: &[u8]) -> Result<NbtTag, String> {
    if data.is_empty() || data[0] != 10 {
        return Err("not a compound".into());
    }
    let mut pos = 1; // skip root type
    let name_len = read_u16(data, &mut pos)? as usize;
    pos += name_len; // skip root compound name
    let (tag, _) = parse_compound(data, &mut pos)?;
    Ok(tag)
}

fn parse_compound(data: &[u8], pos: &mut usize) -> Result<(NbtTag, usize), String> {
    let mut entries = Vec::new();
    loop {
        if *pos >= data.len() {
            break;
        }
        let tag_type = data[*pos];
        *pos += 1;
        if tag_type == 0 {
            break;
        } // TAG_End
        let name_len = read_u16(data, pos)?;
        let name = String::from_utf8_lossy(&data[*pos..*pos + name_len as usize]).to_string();
        *pos += name_len as usize;
        let value = parse_payload(data, pos, tag_type)?;
        entries.push((name, value));
    }
    Ok((NbtTag::Compound(entries), *pos))
}

fn parse_payload(data: &[u8], pos: &mut usize, tag_type: u8) -> Result<NbtTag, String> {
    Ok(match tag_type {
        1 => {
            let v = data[*pos] as i8;
            *pos += 1;
            NbtTag::Byte(v)
        }
        2 => {
            let v = read_i16(data, pos)?;
            NbtTag::Short(v)
        }
        3 => {
            let v = read_i32(data, pos)?;
            NbtTag::Int(v)
        }
        4 => {
            let v = read_i64(data, pos)?;
            NbtTag::Long(v)
        }
        5 => {
            let v =
                f32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
            *pos += 4;
            NbtTag::Float(v)
        }
        6 => {
            let v = f64::from_be_bytes([
                data[*pos],
                data[*pos + 1],
                data[*pos + 2],
                data[*pos + 3],
                data[*pos + 4],
                data[*pos + 5],
                data[*pos + 6],
                data[*pos + 7],
            ]);
            *pos += 8;
            NbtTag::Double(v)
        }
        7 => {
            let len = read_i32(data, pos)? as usize;
            let bytes = data[*pos..*pos + len].to_vec();
            *pos += len;
            NbtTag::ByteArray(bytes)
        }
        8 => {
            let len = read_u16(data, pos)? as usize;
            let s = String::from_utf8_lossy(&data[*pos..*pos + len]).to_string();
            *pos += len;
            NbtTag::String(s)
        }
        9 => {
            let list_type = data[*pos];
            *pos += 1;
            let len = read_i32(data, pos)? as usize;
            let mut items = Vec::new();
            for _ in 0..len {
                items.push(parse_payload(data, pos, list_type)?);
            }
            NbtTag::List(items)
        }
        10 => {
            let (c, _) = parse_compound(data, pos)?;
            c
        }
        11 => {
            let len = read_i32(data, pos)? as usize;
            let mut arr = Vec::new();
            for _ in 0..len {
                arr.push(read_i32(data, pos)?);
            }
            NbtTag::IntArray(arr)
        }
        12 => {
            let len = read_i32(data, pos)? as usize;
            let mut arr = Vec::new();
            for _ in 0..len {
                arr.push(read_i64(data, pos)?);
            }
            NbtTag::LongArray(arr)
        }
        _ => return Err(format!("unknown tag type: {}", tag_type)),
    })
}

fn read_u16(data: &[u8], pos: &mut usize) -> Result<u16, String> {
    if *pos + 2 > data.len() {
        return Err("EOF".into());
    }
    let v = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    Ok(v)
}
fn read_i16(data: &[u8], pos: &mut usize) -> Result<i16, String> {
    Ok(read_u16(data, pos)? as i16)
}
fn read_i32(data: &[u8], pos: &mut usize) -> Result<i32, String> {
    if *pos + 4 > data.len() {
        return Err("EOF".into());
    }
    let v = i32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Ok(v)
}
fn read_i64(data: &[u8], pos: &mut usize) -> Result<i64, String> {
    if *pos + 8 > data.len() {
        return Err("EOF".into());
    }
    let v = i64::from_be_bytes([
        data[*pos],
        data[*pos + 1],
        data[*pos + 2],
        data[*pos + 3],
        data[*pos + 4],
        data[*pos + 5],
        data[*pos + 6],
        data[*pos + 7],
    ]);
    *pos += 8;
    Ok(v)
}

/// Extracts world info from a parsed level.dat compound.
pub(crate) fn extract_world_info(tag: &NbtTag) -> Option<WorldInfo> {
    if let NbtTag::Compound(entries) = tag {
        let data = find_compound(entries, "Data");
        let data_entries = if let Some(NbtTag::Compound(d)) = data {
            d
        } else {
            entries
        };

        let name = get_string(data_entries, "LevelName").unwrap_or("World");
        let seed = get_long(data_entries, "RandomSeed").unwrap_or(0);
        let game_type = match get_int(data_entries, "GameType").unwrap_or(1) {
            0 => "survival",
            1 => "creative",
            2 => "adventure",
            3 => "spectator",
            _ => "survival",
        };
        let version_name = "unknown";
        let last_played = get_long(data_entries, "LastPlayed").unwrap_or(0) as u64;
        let time = get_long(data_entries, "Time").unwrap_or(0) as u64;
        let spawn_x = get_int(data_entries, "SpawnX").unwrap_or(0);
        let spawn_y = get_int(data_entries, "SpawnY").unwrap_or(64);
        let spawn_z = get_int(data_entries, "SpawnZ").unwrap_or(0);
        let difficulty = match get_int(data_entries, "Difficulty").unwrap_or(2) {
            0 => "peaceful",
            1 => "easy",
            2 => "normal",
            3 => "hard",
            _ => "normal",
        };
        let hardcore = get_byte(data_entries, "hardcore").unwrap_or(0) != 0;
        let cheats = get_byte(data_entries, "allowCommands").unwrap_or(0) != 0;

        Some(WorldInfo {
            name: name.to_string(),
            seed,
            game_type: game_type.into(),
            version_name: version_name.into(),
            last_played,
            time,
            spawn_x,
            spawn_y,
            spawn_z,
            difficulty: difficulty.into(),
            hardcore,
            cheats_enabled: cheats,
            size_bytes: 0,
            size_formatted: String::new(),
        })
    } else {
        None
    }
}

fn find_compound<'a>(entries: &'a [(String, NbtTag)], key: &str) -> Option<&'a NbtTag> {
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}
fn get_string<'a>(entries: &'a [(String, NbtTag)], key: &str) -> Option<&'a str> {
    if let Some(NbtTag::String(s)) = find_compound(entries, key) {
        Some(s)
    } else {
        None
    }
}
fn get_int(entries: &[(String, NbtTag)], key: &str) -> Option<i32> {
    match find_compound(entries, key)? {
        NbtTag::Int(v) => Some(*v),
        NbtTag::Byte(v) => Some(*v as i32),
        _ => None,
    }
}
fn get_long(entries: &[(String, NbtTag)], key: &str) -> Option<i64> {
    match find_compound(entries, key)? {
        NbtTag::Long(v) => Some(*v),
        NbtTag::Int(v) => Some(*v as i64),
        _ => None,
    }
}
fn get_byte(entries: &[(String, NbtTag)], key: &str) -> Option<i8> {
    if let Some(NbtTag::Byte(v)) = find_compound(entries, key) {
        Some(*v)
    } else {
        None
    }
}

/// Reads world info from a saves/<world>/level.dat path.
pub fn read_world_info(world_dir: &std::path::Path) -> Result<WorldInfo, String> {
    let level_dat = world_dir.join("level.dat");
    if !level_dat.is_file() {
        return Err("level.dat not found".into());
    }
    let tag = read_nbt(&level_dat)?;
    let mut info = extract_world_info(&tag).ok_or("failed to extract world info")?;
    // Calculate size
    fn dir_size(d: &std::path::Path, s: &mut u64) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dir_size(&p, s);
            } else if let Ok(m) = p.metadata() {
                *s += m.len();
            }
        }
    }
    dir_size(world_dir, &mut info.size_bytes);
    info.size_formatted = if info.size_bytes < 1048576 {
        format!("{:.1} KB", info.size_bytes as f64 / 1024.0)
    } else if info.size_bytes < 1073741824 {
        format!("{:.1} MB", info.size_bytes as f64 / 1048576.0)
    } else {
        format!("{:.2} GB", info.size_bytes as f64 / 1073741824.0)
    };
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nbt_compound_parser() {
        let data = [10u8, 0, 3, 68, 97, 116, 1, 0, 1, 65, 42, 0];
        assert!(parse_nbt_binary(&data).is_ok());
    }
}
