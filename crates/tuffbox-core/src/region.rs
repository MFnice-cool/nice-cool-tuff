//! Anvil region (.mca) reader/editor for MCA Selector-style world tools.
//!
//! Parses Minecraft region files according to the Anvil format:
//!   - 4096-byte location table  (4 bytes / chunk: 3-byte sector offset + 1-byte
//!     sector count)
//!   - 4096-byte timestamp table (4 bytes / chunk: last-modified epoch seconds)
//!   - chunk data sectors: 4-byte big-endian length + 1-byte compression type
//!     + compressed NBT
//!
//! Dimensions:
//!   - overworld → `region/`
//!   - nether    → `DIM-1/region/`
//!   - end       → `DIM1/region/`

use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// A region file and its 32x32 chunk grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionInfo {
    pub region_x: i32,
    pub region_z: i32,
    pub present: usize,
    pub min_modified: u64,
    pub max_modified: u64,
    pub chunks: Vec<ChunkCell>,
}

/// Compact per-chunk cell for the 2D map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkCell {
    pub present: u8,
    pub last_modified: u64,
    /// Status code: 0 unknown, 1 empty, 2 partial, 3 full.
    pub status: u8,
    /// Players' total time in this chunk (game ticks).
    #[serde(default)]
    pub inhabited_time: u64,
    /// Minecraft DataVersion (0 if unknown).
    #[serde(default)]
    pub data_version: i32,
    /// Dominant / sample biome id (-1 unknown).
    #[serde(default = "default_biome")]
    pub biome_id: i32,
    /// Sampled surface height (-9999 unknown).
    #[serde(default = "default_surface")]
    pub surface_y: i16,
    /// Entity count from chunk NBT (pre-1.17 or leftover).
    #[serde(default)]
    pub entity_count: u16,
    /// Rough structure reference count.
    #[serde(default)]
    pub structure_count: u16,
}

fn default_biome() -> i32 {
    -1
}
fn default_surface() -> i16 {
    -9999
}

/// Extra overlay fields filled while scanning chunk NBT.
#[derive(Debug, Clone, Default)]
pub struct ChunkMetaExtras {
    pub biome_id: i32,
    pub surface_y: i16,
    pub entity_count: u16,
    pub structure_count: u16,
}

/// Full map overview for a world dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldMap {
    pub regions: Vec<RegionInfo>,
    pub min_region_x: i32,
    pub min_region_z: i32,
    pub max_region_x: i32,
    pub max_region_z: i32,
    pub total_present: usize,
    pub region_count: usize,
    /// Dimension id: "overworld" | "nether" | "end".
    #[serde(default = "default_dimension")]
    pub dimension: String,
}

fn default_dimension() -> String {
    "overworld".into()
}

pub const STATUS_UNKNOWN: u8 = 0;
pub const STATUS_EMPTY: u8 = 1;
pub const STATUS_PARTIAL: u8 = 2;
pub const STATUS_FULL: u8 = 3;

const GRID: i32 = 32;
const LOC_ENTRY: usize = 4;
const HEADER_SIZE: usize = 8192;
const SECTOR: usize = 4096;

/// Resolves a data folder (`region` / `entities` / `poi`) for a dimension.
pub fn dimension_data_dir(world_dir: &Path, dimension: &str, kind: &str) -> PathBuf {
    let base = match dimension {
        "nether" | "DIM-1" => world_dir.join("DIM-1"),
        "end" | "DIM1" | "the_end" => world_dir.join("DIM1"),
        _ => world_dir.to_path_buf(),
    };
    base.join(kind)
}

/// Resolves the region folder for a dimension inside a world save.
pub fn dimension_region_dir(world_dir: &Path, dimension: &str) -> PathBuf {
    dimension_data_dir(world_dir, dimension, "region")
}

/// Lists dimensions that have a region folder (possibly empty).
pub fn list_world_dimensions(world_dir: &Path) -> Vec<String> {
    let mut dims = Vec::new();
    for (id, rel) in [
        ("overworld", world_dir.join("region")),
        ("nether", world_dir.join("DIM-1").join("region")),
        ("end", world_dir.join("DIM1").join("region")),
    ] {
        if rel.is_dir() {
            dims.push(id.to_string());
        }
    }
    if dims.is_empty() {
        dims.push("overworld".into());
    }
    dims
}

pub fn normalize_dimension(dimension: Option<&str>) -> &str {
    match dimension.unwrap_or("overworld") {
        "nether" | "DIM-1" => "nether",
        "end" | "DIM1" | "the_end" => "end",
        _ => "overworld",
    }
}

/// Reads the 2D map overview for a world dimension's region folder.
pub fn read_world_map(world_dir: &Path, dimension: Option<&str>) -> Result<WorldMap, String> {
    read_world_map_cached(world_dir, dimension, true)
}

/// Read world map; when `use_cache` is true, reuse per-region JSON tiles keyed by MCA mtime/size.
pub fn read_world_map_cached(
    world_dir: &Path,
    dimension: Option<&str>,
    use_cache: bool,
) -> Result<WorldMap, String> {
    let dim = normalize_dimension(dimension);
    let region_dir = dimension_region_dir(world_dir, dim);
    if !region_dir.is_dir() {
        return Err(format!(
            "no region folder for dimension '{dim}' (not generated yet)"
        ));
    }

    let cache_dir = if use_cache {
        Some(world_map_cache_dir(world_dir, dim)?)
    } else {
        None
    };

    let mut regions: Vec<RegionInfo> = Vec::new();
    let mut min_rx = i32::MAX;
    let mut min_rz = i32::MAX;
    let mut max_rx = i32::MIN;
    let mut max_rz = i32::MIN;
    let mut total_present = 0usize;
    let mut mca_on_disk = 0usize;
    let mut parse_errors: Vec<String> = Vec::new();

    let entries = std::fs::read_dir(&region_dir).map_err(|e| e.to_string())?;
    let mut names: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("mca") {
            names.push(p);
        }
    }
    names.sort();

    for path in names {
        let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let parts: Vec<&str> = file_name.split('.').collect();
        if parts.len() < 3 || parts[0] != "r" {
            continue;
        }
        let rx: i32 = match parts[1].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rz: i32 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        mca_on_disk += 1;
        let info = match load_region_info(&path, rx, rz, cache_dir.as_deref()) {
            Ok(info) => info,
            Err(e) => {
                if parse_errors.len() < 3 {
                    let label = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("region");
                    parse_errors.push(format!("{label}: {e}"));
                }
                continue;
            }
        };
        total_present += info.present;
        min_rx = min_rx.min(rx);
        min_rz = min_rz.min(rz);
        max_rx = max_rx.max(rx);
        max_rz = max_rz.max(rz);
        regions.push(info);
    }

    if regions.is_empty() {
        if mca_on_disk == 0 {
            return Err("no region files found".into());
        }
        let detail = if parse_errors.is_empty() {
            "unknown parse errors".to_string()
        } else {
            parse_errors.join("; ")
        };
        return Err(format!(
            "no region files found ({mca_on_disk} .mca on disk, 0 parsed: {detail})"
        ));
    }

    regions.sort_by(|a, b| (a.region_z, a.region_x).cmp(&(b.region_z, b.region_x)));

    Ok(WorldMap {
        min_region_x: min_rx,
        min_region_z: min_rz,
        max_region_x: max_rx,
        max_region_z: max_rz,
        total_present,
        region_count: regions.len(),
        regions,
        dimension: dim.to_string(),
    })
}

/// Root directory for TuffBox world-map region caches.
pub fn world_map_cache_root() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local)
            .join("TuffBox")
            .join("cache")
            .join("world-map");
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("tuffbox").join("world-map");
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".cache")
        .join("tuffbox")
        .join("world-map")
}

fn world_cache_id(world_dir: &Path) -> String {
    let canon = std::fs::canonicalize(world_dir).unwrap_or_else(|_| world_dir.to_path_buf());
    let mut hasher = Sha1::new();
    hasher.update(canon.to_string_lossy().as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

pub fn world_map_cache_dir(world_dir: &Path, dimension: &str) -> Result<PathBuf, String> {
    let dim = normalize_dimension(Some(dimension));
    let dir = world_map_cache_root()
        .join(world_cache_id(world_dir))
        .join(dim);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Delete cached region tiles for a world (all dimensions) or one dimension.
pub fn clear_world_map_cache(world_dir: &Path, dimension: Option<&str>) -> Result<usize, String> {
    let root = world_map_cache_root().join(world_cache_id(world_dir));
    if !root.exists() {
        return Ok(0);
    }
    if let Some(dim) = dimension {
        let dim = normalize_dimension(Some(dim));
        let dir = root.join(dim);
        if !dir.is_dir() {
            return Ok(0);
        }
        let n = remove_dir_files(&dir)?;
        let _ = std::fs::remove_dir(&dir);
        return Ok(n);
    }
    let mut n = 0usize;
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                n += remove_dir_files(&p)?;
                let _ = std::fs::remove_dir_all(&p);
            } else if p.is_file() {
                let _ = std::fs::remove_file(&p);
                n += 1;
            }
        }
    }
    let _ = std::fs::remove_dir(&root);
    Ok(n)
}

fn remove_dir_files(dir: &Path) -> Result<usize, String> {
    let mut n = 0usize;
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
            n += 1;
        } else if p.is_dir() {
            n += remove_dir_files(&p)?;
            let _ = std::fs::remove_dir_all(&p);
        }
    }
    Ok(n)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedRegionTile {
    mca_mtime: u64,
    mca_size: u64,
    info: RegionInfo,
}

fn mca_fingerprint(path: &Path) -> Result<(u64, u64), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok((modified, meta.len()))
}

/// Load region metadata; on cache-path failure retry raw MCA so a bad cache
/// fingerprint cannot hide a readable region file.
fn load_region_info(
    path: &Path,
    region_x: i32,
    region_z: i32,
    cache_dir: Option<&Path>,
) -> Result<RegionInfo, String> {
    if let Some(cdir) = cache_dir {
        match read_region_cached(path, region_x, region_z, cdir) {
            Ok(info) => return Ok(info),
            Err(_) => {
                // Fall through: fingerprint / IO on cache path must not blank the map.
            }
        }
    }
    read_region(path, region_x, region_z)
}

fn read_region_cached(
    path: &Path,
    region_x: i32,
    region_z: i32,
    cache_dir: &Path,
) -> Result<RegionInfo, String> {
    let (mtime, size) = mca_fingerprint(path)?;
    let cache_path = cache_dir.join(format!("r.{}.{}.json", region_x, region_z));
    if cache_path.is_file() {
        if let Ok(text) = std::fs::read_to_string(&cache_path) {
            if let Ok(tile) = serde_json::from_str::<CachedRegionTile>(&text) {
                if tile.mca_mtime == mtime
                    && tile.mca_size == size
                    && tile.info.region_x == region_x
                    && tile.info.region_z == region_z
                    && tile.info.chunks.len() == 1024
                {
                    return Ok(tile.info);
                }
            }
        }
    }
    let info = read_region(path, region_x, region_z)?;
    let tile = CachedRegionTile {
        mca_mtime: mtime,
        mca_size: size,
        info: info.clone(),
    };
    if let Ok(text) = serde_json::to_string(&tile) {
        let _ = std::fs::write(&cache_path, text);
    }
    Ok(info)
}

/// Warm the region metadata cache (same as a cached map read). Returns region count.
pub fn warm_world_map_cache(world_dir: &Path, dimension: Option<&str>) -> Result<usize, String> {
    let map = read_world_map_cached(world_dir, dimension, true)?;
    Ok(map.region_count)
}

fn read_region(path: &Path, region_x: i32, region_z: i32) -> Result<RegionInfo, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    if data.len() < HEADER_SIZE {
        return Err("region file too small".into());
    }

    let mut chunks = Vec::with_capacity(1024);
    let mut present_count = 0usize;
    let mut min_modified = u64::MAX;
    let mut max_modified = 0u64;

    for index in 0..1024 {
        let loc_off = index * LOC_ENTRY;
        let offset = ((data[loc_off] as usize) << 16)
            | ((data[loc_off + 1] as usize) << 8)
            | (data[loc_off + 2] as usize);

        let ts_off = SECTOR + index * LOC_ENTRY;
        let last_modified = read_u32(&data, ts_off) as u64;

        let present = offset != 0;
        let mut status = STATUS_UNKNOWN;
        let mut inhabited_time = 0u64;
        let mut data_version = 0i32;
        let mut biome_id = -1i32;
        let mut surface_y = -9999i16;
        let mut entity_count = 0u16;
        let mut structure_count = 0u16;
        if present {
            present_count += 1;
            if last_modified != 0 {
                min_modified = min_modified.min(last_modified);
                max_modified = max_modified.max(last_modified);
            }
            let meta = read_chunk_meta(&data, offset);
            status = meta.status;
            inhabited_time = meta.inhabited_time;
            data_version = meta.data_version;
            biome_id = meta.biome_id;
            surface_y = meta.surface_y;
            entity_count = meta.entity_count;
            structure_count = meta.structure_count;
        } else if last_modified != 0 {
            min_modified = min_modified.min(last_modified);
            max_modified = max_modified.max(last_modified);
        }

        chunks.push(ChunkCell {
            present: if present { 1 } else { 0 },
            last_modified,
            status,
            inhabited_time,
            data_version,
            biome_id,
            surface_y,
            entity_count,
            structure_count,
        });
    }

    if min_modified == u64::MAX {
        min_modified = 0;
    }

    Ok(RegionInfo {
        region_x,
        region_z,
        present: present_count,
        min_modified,
        max_modified,
        chunks,
    })
}

struct ChunkMeta {
    status: u8,
    inhabited_time: u64,
    data_version: i32,
    biome_id: i32,
    surface_y: i16,
    entity_count: u16,
    structure_count: u16,
}

fn read_chunk_meta(data: &[u8], offset: usize) -> ChunkMeta {
    let unknown = ChunkMeta {
        status: STATUS_UNKNOWN,
        inhabited_time: 0,
        data_version: 0,
        biome_id: -1,
        surface_y: -9999,
        entity_count: 0,
        structure_count: 0,
    };
    let base = offset * SECTOR;
    if base + 5 > data.len() {
        return unknown;
    }
    let data_len = read_u32(data, base) as usize;
    let compression = data[base + 4];
    let payload_start = base + 5;
    let payload_len = data_len.saturating_sub(1);
    if payload_start + payload_len > data.len() || payload_len == 0 {
        return unknown;
    }
    let payload = &data[payload_start..payload_start + payload_len];
    let decompressed = match decompress(compression, payload) {
        Some(d) => d,
        None => return unknown,
    };
    meta_from_nbt(&decompressed)
}

fn meta_from_nbt(nbt: &[u8]) -> ChunkMeta {
    let mut pos = 1usize;
    if nbt.is_empty() || nbt[0] != 10 {
        return ChunkMeta {
            status: STATUS_UNKNOWN,
            inhabited_time: 0,
            data_version: 0,
            biome_id: -1,
            surface_y: -9999,
            entity_count: 0,
            structure_count: 0,
        };
    }
    let name_len = read_u16(nbt, &mut pos) as usize;
    pos += name_len;

    let mut status = STATUS_FULL;
    let mut inhabited_time = 0u64;
    let mut data_version = 0i32;
    let mut saw_status = false;

    let mut p = pos;
    while p < nbt.len() {
        let tag_type = nbt[p];
        p += 1;
        if tag_type == 0 {
            break;
        }
        let nlen = read_u16(nbt, &mut p) as usize;
        if p + nlen > nbt.len() {
            break;
        }
        let name = std::str::from_utf8(&nbt[p..p + nlen]).unwrap_or("");
        p += nlen;

        match (name, tag_type) {
            ("Status", 8) => {
                let s_len = read_u16(nbt, &mut p) as usize;
                if p + s_len <= nbt.len() {
                    let s = String::from_utf8_lossy(&nbt[p..p + s_len]);
                    status = classify_status(&s);
                    saw_status = true;
                    p += s_len;
                } else {
                    break;
                }
            }
            ("InhabitedTime", 4) => {
                if p + 8 <= nbt.len() {
                    inhabited_time = u64::from_be_bytes([
                        nbt[p],
                        nbt[p + 1],
                        nbt[p + 2],
                        nbt[p + 3],
                        nbt[p + 4],
                        nbt[p + 5],
                        nbt[p + 6],
                        nbt[p + 7],
                    ]);
                    p += 8;
                } else {
                    break;
                }
            }
            ("DataVersion", 3) => {
                if p + 4 <= nbt.len() {
                    data_version = i32::from_be_bytes([nbt[p], nbt[p + 1], nbt[p + 2], nbt[p + 3]]);
                    p += 4;
                } else {
                    break;
                }
            }
            _ => {
                if tag_type == 10 {
                    let mut nested = p;
                    let _ = find_string_in_compound(nbt, &mut nested, "\0__never__");
                    p = nested;
                } else {
                    skip_payload(nbt, &mut p, tag_type);
                }
            }
        }
    }

    if !saw_status {
        status = STATUS_FULL;
    }

    let extras = crate::region_edit::extract_overlay_extras(nbt);

    ChunkMeta {
        status,
        inhabited_time,
        data_version,
        biome_id: extras.biome_id,
        surface_y: extras.surface_y,
        entity_count: extras.entity_count,
        structure_count: extras.structure_count,
    }
}

fn classify_status(s: &str) -> u8 {
    if s == "empty" || s.is_empty() || s == "minecraft:empty" {
        STATUS_EMPTY
    } else if s.contains("structure_starts")
        || s.contains("structure_references")
        || s.contains("biomes")
        || s.contains("noise")
        || s.contains("surface")
        || s.contains("heightmaps")
        || s.contains("carvers")
        || s.contains("features")
        || s.contains("light")
        || s.contains("spawn")
    {
        STATUS_PARTIAL
    } else {
        STATUS_FULL
    }
}

fn find_string_in_compound(nbt: &[u8], pos: &mut usize, key: &str) -> Option<String> {
    while *pos < nbt.len() {
        let tag_type = nbt[*pos];
        *pos += 1;
        if tag_type == 0 {
            break;
        }
        let name_len = read_u16(nbt, pos) as usize;
        if *pos + name_len > nbt.len() {
            return None;
        }
        let name = String::from_utf8_lossy(&nbt[*pos..*pos + name_len]).to_string();
        *pos += name_len;

        if name == key && tag_type == 8 {
            let s_len = read_u16(nbt, pos) as usize;
            if *pos + s_len > nbt.len() {
                return None;
            }
            let s = String::from_utf8_lossy(&nbt[*pos..*pos + s_len]).to_string();
            *pos += s_len;
            return Some(s);
        }

        if tag_type == 10 {
            if let Some(found) = find_string_in_compound(nbt, pos, key) {
                return Some(found);
            }
        } else {
            skip_payload(nbt, pos, tag_type);
        }
    }
    None
}

fn skip_payload(nbt: &[u8], pos: &mut usize, tag_type: u8) {
    match tag_type {
        1 => *pos += 1,
        2 => *pos += 2,
        3 => *pos += 4,
        4 => *pos += 8,
        5 => *pos += 4,
        6 => *pos += 8,
        7 => {
            let len = read_i32(nbt, pos) as usize;
            *pos += len;
        }
        8 => {
            let len = read_u16(nbt, pos) as usize;
            *pos += len;
        }
        9 => {
            let list_type = nbt.get(*pos).copied().unwrap_or(0);
            *pos += 1;
            let len = read_i32(nbt, pos) as usize;
            for _ in 0..len {
                if list_type == 10 {
                    let mut p = *pos;
                    let _ = find_string_in_compound(nbt, &mut p, "\0__never__");
                    *pos = p;
                } else {
                    skip_payload(nbt, pos, list_type);
                }
            }
        }
        10 => {
            let mut p = *pos;
            let _ = find_string_in_compound(nbt, &mut p, "\0__never__");
            *pos = p;
        }
        11 => {
            let len = read_i32(nbt, pos) as usize;
            *pos += len * 4;
        }
        12 => {
            let len = read_i32(nbt, pos) as usize;
            *pos += len * 8;
        }
        _ => {}
    }
}

fn decompress(compression: u8, payload: &[u8]) -> Option<Vec<u8>> {
    match compression {
        1 => {
            let mut decoder = GzDecoder::new(payload);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).ok()?;
            Some(out)
        }
        2 => {
            let mut decoder = ZlibDecoder::new(payload);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).ok()?;
            Some(out)
        }
        3 => Some(payload.to_vec()),
        4 => {
            #[cfg(feature = "lz4")]
            {
                lz4::block::decompress(payload, None).ok()
            }
            #[cfg(not(feature = "lz4"))]
            {
                None
            }
        }
        _ => None,
    }
}

pub fn decompress_chunk_blob(blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() < 5 {
        return None;
    }
    let data_len = read_u32(blob, 0) as usize;
    let compression = blob[4];
    let payload_len = data_len.saturating_sub(1);
    if 5 + payload_len > blob.len() {
        return None;
    }
    decompress(compression, &blob[5..5 + payload_len])
}

fn compress(compression: u8, nbt: &[u8]) -> Option<Vec<u8>> {
    match compression {
        1 => {
            let mut enc = GzEncoder::new(Vec::new(), Compression::default());
            enc.write_all(nbt).ok()?;
            enc.finish().ok()
        }
        2 => {
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(nbt).ok()?;
            enc.finish().ok()
        }
        3 => Some(nbt.to_vec()),
        4 => {
            #[cfg(feature = "lz4")]
            {
                lz4::block::compress(nbt, None, false).ok()
            }
            #[cfg(not(feature = "lz4"))]
            {
                None
            }
        }
        _ => None,
    }
}

pub fn compress_chunk_blob(compression: u8, nbt: &[u8]) -> Option<Vec<u8>> {
    let compressed = compress(compression, nbt)?;
    let new_len = (1 + compressed.len()) as u32;
    let mut out = Vec::with_capacity(5 + compressed.len());
    out.extend_from_slice(&new_len.to_be_bytes());
    out.push(compression);
    out.extend_from_slice(&compressed);
    Some(out)
}

/// Loads a single chunk blob + timestamp from a region folder.
pub fn load_chunk_raw(
    region_dir: &Path,
    region_x: i32,
    region_z: i32,
    index: usize,
) -> Result<Option<(Vec<u8>, u32)>, String> {
    if index >= 1024 {
        return Ok(None);
    }
    let path = region_dir.join(format!("r.{}.{}.mca", region_x, region_z));
    if !path.is_file() {
        return Ok(None);
    }
    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
    if data.len() < HEADER_SIZE {
        return Ok(None);
    }
    let loc_off = index * LOC_ENTRY;
    let offset = ((data[loc_off] as usize) << 16)
        | ((data[loc_off + 1] as usize) << 8)
        | (data[loc_off + 2] as usize);
    if offset == 0 {
        return Ok(None);
    }
    let ts = read_u32(&data, SECTOR + index * LOC_ENTRY);
    let base = offset * SECTOR;
    if base + 5 > data.len() {
        return Ok(None);
    }
    let data_len = read_u32(&data, base) as usize;
    if data_len == 0 || base + 4 + data_len > data.len() {
        return Ok(None);
    }
    Ok(Some((data[base..base + 4 + data_len].to_vec(), ts)))
}

/// Writes/replaces a single chunk blob inside a region file.
pub fn save_chunk_raw(
    region_dir: &Path,
    region_x: i32,
    region_z: i32,
    index: usize,
    blob: &[u8],
    last_modified: u32,
) -> Result<(), String> {
    if index >= 1024 {
        return Err("bad index".into());
    }
    std::fs::create_dir_all(region_dir).map_err(|e| e.to_string())?;
    let path = region_dir.join(format!("r.{}.{}.mca", region_x, region_z));
    if !path.is_file() {
        create_region_file(&path)?;
    }
    let mut data = std::fs::read(&path).map_err(|e| e.to_string())?;
    if data.len() < HEADER_SIZE {
        create_region_file(&path)?;
        data = std::fs::read(&path).map_err(|e| e.to_string())?;
    }

    let mut used = HashSet::new();
    used.insert(0);
    used.insert(1);
    for i in 0..1024 {
        if i == index {
            continue; // free old sectors of target
        }
        let loc_off = i * LOC_ENTRY;
        let offset = ((data[loc_off] as usize) << 16)
            | ((data[loc_off + 1] as usize) << 8)
            | (data[loc_off + 2] as usize);
        let sc = data[loc_off + 3] as usize;
        if offset != 0 {
            for s in offset..offset + sc {
                used.insert(s);
            }
        }
    }
    let sector_count = (blob.len() + SECTOR - 1) / SECTOR;
    let start = find_free_sectors(&used, 2, sector_count);
    let data_start = start * SECTOR;
    let need = data_start + sector_count * SECTOR;
    if data.len() < need {
        data.resize(need, 0);
    }
    for b in &mut data[data_start..data_start + sector_count * SECTOR] {
        *b = 0;
    }
    data[data_start..data_start + blob.len()].copy_from_slice(blob);
    let loc_off = index * LOC_ENTRY;
    data[loc_off] = ((start >> 16) & 0xFF) as u8;
    data[loc_off + 1] = ((start >> 8) & 0xFF) as u8;
    data[loc_off + 2] = (start & 0xFF) as u8;
    data[loc_off + 3] = sector_count as u8;
    let ts_off = SECTOR + index * LOC_ENTRY;
    data[ts_off..ts_off + 4].copy_from_slice(&last_modified.to_be_bytes());
    std::fs::write(&path, &data).map_err(|e| e.to_string())
}

pub fn delete_single_chunk(
    region_dir: &Path,
    region_x: i32,
    region_z: i32,
    index: usize,
) -> Result<(), String> {
    if index >= 1024 || !region_dir.is_dir() {
        return Ok(());
    }
    let path = region_dir.join(format!("r.{}.{}.mca", region_x, region_z));
    if !path.is_file() {
        return Ok(());
    }
    let mut data = std::fs::read(&path).map_err(|e| e.to_string())?;
    if data.len() < HEADER_SIZE {
        return Ok(());
    }
    let loc_off = index * LOC_ENTRY;
    for b in &mut data[loc_off..loc_off + LOC_ENTRY] {
        *b = 0;
    }
    let ts_off = SECTOR + index * LOC_ENTRY;
    for b in &mut data[ts_off..ts_off + 4] {
        *b = 0;
    }
    std::fs::write(&path, &data).map_err(|e| e.to_string())
}

/// Rewrites root-level TAG_Int fields `xPos` / `zPos` in chunk NBT.
fn set_root_int(nbt: &mut [u8], key: &str, value: i32) -> bool {
    if nbt.is_empty() || nbt[0] != 10 {
        return false;
    }
    let mut pos = 1usize;
    let name_len = read_u16(nbt, &mut pos) as usize;
    pos += name_len;
    while pos < nbt.len() {
        let tag_type = nbt[pos];
        pos += 1;
        if tag_type == 0 {
            break;
        }
        let nlen = read_u16(nbt, &mut pos) as usize;
        if pos + nlen > nbt.len() {
            return false;
        }
        let name = &nbt[pos..pos + nlen];
        pos += nlen;
        let name_str = std::str::from_utf8(name).unwrap_or("");
        if name_str == key && tag_type == 3 {
            if pos + 4 > nbt.len() {
                return false;
            }
            nbt[pos..pos + 4].copy_from_slice(&value.to_be_bytes());
            return true;
        }
        if tag_type == 10 {
            // Skip nested compound without rewriting.
            let mut p = pos;
            let _ = find_string_in_compound(nbt, &mut p, "\0__never__");
            pos = p;
        } else {
            skip_payload(nbt, &mut pos, tag_type);
        }
    }
    false
}

fn rewrite_chunk_coords(
    chunk_blob: &[u8],
    cx: i32,
    cz: i32,
    src_cx: i32,
    src_cz: i32,
) -> Option<Vec<u8>> {
    if chunk_blob.len() < 5 {
        return None;
    }
    let data_len = read_u32(chunk_blob, 0) as usize;
    let compression = chunk_blob[4];
    let payload_start = 5;
    let payload_len = data_len.saturating_sub(1);
    if payload_start + payload_len > chunk_blob.len() || payload_len == 0 {
        return None;
    }
    let payload = &chunk_blob[payload_start..payload_start + payload_len];
    let mut nbt = decompress(compression, payload)?;
    let _ = set_root_int(&mut nbt, "xPos", cx);
    let _ = set_root_int(&mut nbt, "zPos", cz);
    let block_dx = (cx - src_cx) * 16;
    let block_dz = (cz - src_cz) * 16;
    if block_dx != 0 || block_dz != 0 {
        offset_entity_positions(
            &mut nbt,
            block_dx as f64,
            block_dz as f64,
            block_dx,
            block_dz,
        );
    }
    let compressed = compress(compression, &nbt)?;
    let new_len = (1 + compressed.len()) as u32;
    let mut out = Vec::with_capacity(5 + compressed.len());
    out.extend_from_slice(&new_len.to_be_bytes());
    out.push(compression);
    out.extend_from_slice(&compressed);
    Some(out)
}

/// Offsets entity/block-entity coordinates inside chunk NBT (MCA Selector paste).
fn offset_entity_positions(nbt: &mut [u8], dx: f64, dz: f64, block_dx: i32, block_dz: i32) {
    if nbt.is_empty() || nbt[0] != 10 {
        return;
    }
    let mut pos = 1usize;
    let name_len = read_u16(nbt, &mut pos) as usize;
    pos += name_len;
    offset_in_compound(nbt, &mut pos, dx, dz, block_dx, block_dz, false);
}

fn offset_in_compound(
    nbt: &mut [u8],
    pos: &mut usize,
    dx: f64,
    dz: f64,
    block_dx: i32,
    block_dz: i32,
    in_entity_list: bool,
) {
    while *pos < nbt.len() {
        let tag_type = nbt[*pos];
        *pos += 1;
        if tag_type == 0 {
            break;
        }
        let nlen = read_u16(nbt, pos) as usize;
        if *pos + nlen > nbt.len() {
            return;
        }
        let name = {
            let s = std::str::from_utf8(&nbt[*pos..*pos + nlen])
                .unwrap_or("")
                .to_string();
            *pos += nlen;
            s
        };

        let is_entity_container = matches!(
            name.as_str(),
            "Entities"
                | "entities"
                | "TileEntities"
                | "tile_entities"
                | "block_entities"
                | "blockEntities"
        );

        match tag_type {
            9 => {
                // List
                let list_type = nbt.get(*pos).copied().unwrap_or(0);
                *pos += 1;
                let len = read_i32(nbt, pos) as usize;
                if name == "Pos" && list_type == 6 && len >= 3 && in_entity_list {
                    // Double list: x, y, z
                    if *pos + 24 <= nbt.len() {
                        let x =
                            f64::from_be_bytes(nbt[*pos..*pos + 8].try_into().unwrap_or([0; 8]));
                        let z = f64::from_be_bytes(
                            nbt[*pos + 16..*pos + 24].try_into().unwrap_or([0; 8]),
                        );
                        nbt[*pos..*pos + 8].copy_from_slice(&(x + dx).to_be_bytes());
                        nbt[*pos + 16..*pos + 24].copy_from_slice(&(z + dz).to_be_bytes());
                    }
                    *pos += len * 8;
                } else if list_type == 10 {
                    for _ in 0..len {
                        offset_in_compound(
                            nbt,
                            pos,
                            dx,
                            dz,
                            block_dx,
                            block_dz,
                            in_entity_list || is_entity_container,
                        );
                    }
                } else {
                    for _ in 0..len {
                        skip_payload(nbt, pos, list_type);
                    }
                }
            }
            10 => {
                offset_in_compound(
                    nbt,
                    pos,
                    dx,
                    dz,
                    block_dx,
                    block_dz,
                    in_entity_list || is_entity_container,
                );
            }
            3 if in_entity_list && (name == "x" || name == "TileX") => {
                if *pos + 4 <= nbt.len() {
                    let v = i32::from_be_bytes([
                        nbt[*pos],
                        nbt[*pos + 1],
                        nbt[*pos + 2],
                        nbt[*pos + 3],
                    ]);
                    nbt[*pos..*pos + 4].copy_from_slice(&(v + block_dx).to_be_bytes());
                }
                *pos += 4;
            }
            3 if in_entity_list && (name == "z" || name == "TileZ") => {
                if *pos + 4 <= nbt.len() {
                    let v = i32::from_be_bytes([
                        nbt[*pos],
                        nbt[*pos + 1],
                        nbt[*pos + 2],
                        nbt[*pos + 3],
                    ]);
                    nbt[*pos..*pos + 4].copy_from_slice(&(v + block_dz).to_be_bytes());
                }
                *pos += 4;
            }
            _ => skip_payload(nbt, pos, tag_type),
        }
    }
}

fn world_chunk_coords(rx: i32, rz: i32, index: usize) -> (i32, i32) {
    let lx = (index % 32) as i32;
    let lz = (index / 32) as i32;
    (rx * GRID + lx, rz * GRID + lz)
}

fn region_and_index(cx: i32, cz: i32) -> (i32, i32, usize) {
    let rx = cx.div_euclid(GRID);
    let rz = cz.div_euclid(GRID);
    let lx = cx.rem_euclid(GRID) as usize;
    let lz = cz.rem_euclid(GRID) as usize;
    (rx, rz, lz * 32 + lx)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkData {
    pub region_x: i32,
    pub region_z: i32,
    pub index: usize,
    pub data: Vec<u8>,
    pub last_modified: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkClipboard {
    pub source_world: String,
    pub chunks: Vec<ChunkData>,
    pub bounds: (i32, i32, i32, i32),
    /// Separate entities/*.mca payload (1.17+), if present.
    #[serde(default)]
    pub entities: Vec<ChunkData>,
    /// Separate poi/*.mca payload, if present.
    #[serde(default)]
    pub poi: Vec<ChunkData>,
}

fn extract_chunks_from_dir(
    region_dir: &Path,
    selections: &[(i32, i32, Vec<usize>)],
) -> Result<Vec<ChunkData>, String> {
    let mut chunks = Vec::new();
    if !region_dir.is_dir() {
        return Ok(chunks);
    }
    for (rx, rz, indices) in selections {
        if indices.is_empty() {
            continue;
        }
        let path = region_dir.join(format!("r.{}.{}.mca", rx, rz));
        if !path.is_file() {
            continue;
        }
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        if data.len() < HEADER_SIZE {
            continue;
        }
        for &index in indices {
            if index >= 1024 {
                continue;
            }
            let loc_off = index * LOC_ENTRY;
            let offset = ((data[loc_off] as usize) << 16)
                | ((data[loc_off + 1] as usize) << 8)
                | (data[loc_off + 2] as usize);
            if offset == 0 {
                continue;
            }
            let ts_off = SECTOR + index * LOC_ENTRY;
            let last_modified = read_u32(&data, ts_off);
            let base = offset * SECTOR;
            if base + 5 > data.len() {
                continue;
            }
            let data_len = read_u32(&data, base) as usize;
            if data_len == 0 || base + 4 + data_len > data.len() {
                continue;
            }
            chunks.push(ChunkData {
                region_x: *rx,
                region_z: *rz,
                index,
                data: data[base..base + 4 + data_len].to_vec(),
                last_modified,
            });
        }
    }
    Ok(chunks)
}

pub fn copy_world_chunks(
    world_dir: &Path,
    world_name: &str,
    selections: &[(i32, i32, Vec<usize>)],
    dimension: Option<&str>,
) -> Result<ChunkClipboard, String> {
    let dim = normalize_dimension(dimension);
    let region_dir = dimension_data_dir(world_dir, dim, "region");
    if !region_dir.is_dir() {
        return Err("no region folder".into());
    }

    let chunks = extract_chunks_from_dir(&region_dir, selections)?;
    if chunks.is_empty() {
        return Err("no chunks selected".into());
    }

    let mut min_rx = i32::MAX;
    let mut min_rz = i32::MAX;
    let mut max_rx = i32::MIN;
    let mut max_rz = i32::MIN;
    for c in &chunks {
        min_rx = min_rx.min(c.region_x);
        min_rz = min_rz.min(c.region_z);
        max_rx = max_rx.max(c.region_x);
        max_rz = max_rz.max(c.region_z);
    }

    let entities =
        extract_chunks_from_dir(&dimension_data_dir(world_dir, dim, "entities"), selections)
            .unwrap_or_default();
    let poi = extract_chunks_from_dir(&dimension_data_dir(world_dir, dim, "poi"), selections)
        .unwrap_or_default();

    Ok(ChunkClipboard {
        source_world: world_name.to_string(),
        chunks,
        bounds: (min_rx, min_rz, max_rx, max_rz),
        entities,
        poi,
    })
}

fn paste_layer_into_dir(
    region_dir: &Path,
    layer_chunks: &[ChunkData],
    offset_x: i32,
    offset_z: i32,
    overwrite: bool,
) -> Result<usize, String> {
    if layer_chunks.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(region_dir).map_err(|e| e.to_string())?;

    let mut by_region: HashMap<(i32, i32), Vec<(usize, Vec<u8>, u32)>> = HashMap::new();
    for chunk in layer_chunks {
        let (src_cx, src_cz) = world_chunk_coords(chunk.region_x, chunk.region_z, chunk.index);
        let dst_cx = src_cx + offset_x;
        let dst_cz = src_cz + offset_z;
        let (trx, trz, tindex) = region_and_index(dst_cx, dst_cz);

        let blob = if offset_x != 0 || offset_z != 0 {
            rewrite_chunk_coords(&chunk.data, dst_cx, dst_cz, src_cx, src_cz)
                .unwrap_or_else(|| chunk.data.clone())
        } else {
            chunk.data.clone()
        };

        by_region
            .entry((trx, trz))
            .or_default()
            .push((tindex, blob, chunk.last_modified));
    }

    let mut pasted = 0usize;
    for ((target_rx, target_rz), chunks) in &by_region {
        let path = region_dir.join(format!("r.{}.{}.mca", target_rx, target_rz));
        if !path.is_file() {
            create_region_file(&path)?;
        }
        let mut data = std::fs::read(&path).map_err(|e| e.to_string())?;
        if data.len() < HEADER_SIZE {
            create_region_file(&path)?;
            data = std::fs::read(&path).map_err(|e| e.to_string())?;
        }

        let mut used_sectors = HashSet::new();
        used_sectors.insert(0);
        used_sectors.insert(1);
        for i in 0..1024 {
            let loc_off = i * LOC_ENTRY;
            let offset = ((data[loc_off] as usize) << 16)
                | ((data[loc_off + 1] as usize) << 8)
                | (data[loc_off + 2] as usize);
            let sector_count = data[loc_off + 3] as usize;
            if offset != 0 {
                for s in offset..offset + sector_count {
                    used_sectors.insert(s);
                }
            }
        }
        let mut next_free_sector = used_sectors.iter().copied().max().unwrap_or(1) + 1;

        for (index, chunk_blob, last_modified) in chunks {
            if *index >= 1024 {
                continue;
            }
            let loc_off = index * LOC_ENTRY;
            let existing = ((data[loc_off] as usize) << 16)
                | ((data[loc_off + 1] as usize) << 8)
                | (data[loc_off + 2] as usize);
            if !overwrite && existing != 0 {
                continue;
            }
            let ts_off = SECTOR + index * LOC_ENTRY;
            let chunk_data_len = chunk_blob.len();
            let sector_count = (chunk_data_len + SECTOR - 1) / SECTOR;
            let start_sector = find_free_sectors(&used_sectors, next_free_sector, sector_count);
            let data_start = start_sector * SECTOR;
            let required_len = data_start + sector_count * SECTOR;
            if data.len() < required_len {
                data.resize(required_len, 0);
            }
            for b in &mut data[data_start..data_start + sector_count * SECTOR] {
                *b = 0;
            }
            data[data_start..data_start + chunk_data_len].copy_from_slice(chunk_blob);
            data[loc_off] = ((start_sector >> 16) & 0xFF) as u8;
            data[loc_off + 1] = ((start_sector >> 8) & 0xFF) as u8;
            data[loc_off + 2] = (start_sector & 0xFF) as u8;
            data[loc_off + 3] = sector_count as u8;
            data[ts_off..ts_off + 4].copy_from_slice(&last_modified.to_be_bytes());
            for s in start_sector..start_sector + sector_count {
                used_sectors.insert(s);
            }
            next_free_sector = start_sector + sector_count;
            pasted += 1;
        }
        std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    }
    Ok(pasted)
}

/// Pastes clipboard chunks with **chunk-coordinate** offsets (not region).
/// Rewrites NBT `xPos`/`zPos` and entity/block-entity positions.
pub fn paste_world_chunks(
    world_dir: &Path,
    clipboard: &ChunkClipboard,
    offset_x: i32,
    offset_z: i32,
    dimension: Option<&str>,
) -> Result<usize, String> {
    paste_world_chunks_ex(world_dir, clipboard, offset_x, offset_z, dimension, true)
}

/// Like [`paste_world_chunks`], with overwrite control (MCA Selector "Overwrite existing").
pub fn paste_world_chunks_ex(
    world_dir: &Path,
    clipboard: &ChunkClipboard,
    offset_x: i32,
    offset_z: i32,
    dimension: Option<&str>,
    overwrite: bool,
) -> Result<usize, String> {
    let dim = normalize_dimension(dimension);
    let mut pasted = paste_layer_into_dir(
        &dimension_data_dir(world_dir, dim, "region"),
        &clipboard.chunks,
        offset_x,
        offset_z,
        overwrite,
    )?;
    pasted += paste_layer_into_dir(
        &dimension_data_dir(world_dir, dim, "entities"),
        &clipboard.entities,
        offset_x,
        offset_z,
        overwrite,
    )?;
    pasted += paste_layer_into_dir(
        &dimension_data_dir(world_dir, dim, "poi"),
        &clipboard.poi,
        offset_x,
        offset_z,
        overwrite,
    )?;
    Ok(pasted)
}

/// Options for importing chunks from another world / export folder.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub offset_x: i32,
    pub offset_z: i32,
    /// When false, skip destinations that already have a chunk.
    pub overwrite: bool,
    /// Keep only these sections (MCA Selector syntax: `all`, `:-4`, `0:4`). Empty = all.
    pub sections: Option<String>,
    /// Shift imported sections vertically by this many section units (×16 blocks).
    pub y_offset: i32,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            offset_x: 0,
            offset_z: 0,
            overwrite: true,
            sections: None,
            y_offset: 0,
        }
    }
}

fn selection_index_set(selections: &[(i32, i32, Vec<usize>)]) -> HashSet<(i32, i32, usize)> {
    let mut set = HashSet::new();
    for (rx, rz, indices) in selections {
        for &i in indices {
            set.insert((*rx, *rz, i));
        }
    }
    set
}

fn filter_layer_by_dest(
    layer: &[ChunkData],
    offset_x: i32,
    offset_z: i32,
    dest: &HashSet<(i32, i32, usize)>,
) -> Vec<ChunkData> {
    layer
        .iter()
        .filter(|c| {
            let (sx, sz) = world_chunk_coords(c.region_x, c.region_z, c.index);
            let (rx, rz, idx) = region_and_index(sx + offset_x, sz + offset_z);
            dest.contains(&(rx, rz, idx))
        })
        .cloned()
        .collect()
}

/// Import chunks from `source_world` into `target_world` (MCA Selector Chunk Import).
///
/// - `source_selections` empty → all present chunks in the source dimension
/// - `target_only_selection` Some → only paste where destination falls in selection
pub fn import_world_chunks(
    target_world: &Path,
    source_world: &Path,
    source_selections: &[(i32, i32, Vec<usize>)],
    source_dimension: Option<&str>,
    target_dimension: Option<&str>,
    opts: &ImportOptions,
    target_only_selection: Option<&[(i32, i32, Vec<usize>)]>,
) -> Result<usize, String> {
    let src_dim = normalize_dimension(source_dimension);
    let dst_dim = normalize_dimension(target_dimension);

    let sels: Vec<(i32, i32, Vec<usize>)> = if source_selections.is_empty() {
        let refs = crate::region_edit::list_present_chunks(source_world, Some(src_dim))?;
        crate::region_edit::chunk_refs_to_selections(&refs)
    } else {
        source_selections.to_vec()
    };

    let mut clipboard = copy_world_chunks(source_world, "import", &sels, Some(src_dim))?;

    if let Some(dest_sels) = target_only_selection {
        let dest = selection_index_set(dest_sels);
        clipboard.chunks =
            filter_layer_by_dest(&clipboard.chunks, opts.offset_x, opts.offset_z, &dest);
        clipboard.entities =
            filter_layer_by_dest(&clipboard.entities, opts.offset_x, opts.offset_z, &dest);
        clipboard.poi = filter_layer_by_dest(&clipboard.poi, opts.offset_x, opts.offset_z, &dest);
        if clipboard.chunks.is_empty() {
            return Err("no imported chunks land in the target selection".into());
        }
    }

    let need_transform = opts.y_offset != 0
        || opts
            .sections
            .as_ref()
            .map(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case("all") && s != ":")
            .unwrap_or(false);
    if need_transform {
        let sections = opts.sections.as_deref();
        for layer in [
            &mut clipboard.chunks,
            &mut clipboard.entities,
            &mut clipboard.poi,
        ] {
            for chunk in layer.iter_mut() {
                if let Ok(blob) = crate::region_edit::transform_import_chunk_blob(
                    &chunk.data,
                    sections,
                    opts.y_offset,
                ) {
                    chunk.data = blob;
                }
            }
        }
    }

    paste_world_chunks_ex(
        target_world,
        &clipboard,
        opts.offset_x,
        opts.offset_z,
        Some(dst_dim),
        opts.overwrite,
    )
}

fn create_region_file(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = vec![0u8; HEADER_SIZE];
    std::fs::write(path, &data).map_err(|e| e.to_string())
}

fn find_free_sectors(used: &HashSet<usize>, start: usize, count: usize) -> usize {
    let mut candidate = start.max(2);
    loop {
        let mut all_free = true;
        for s in candidate..candidate + count {
            if used.contains(&s) {
                all_free = false;
                candidate = s + 1;
                break;
            }
        }
        if all_free {
            return candidate;
        }
    }
}

fn delete_chunks_in_dir(
    region_dir: &Path,
    selections: &[(i32, i32, Vec<usize>)],
) -> Result<usize, String> {
    if !region_dir.is_dir() {
        return Ok(0);
    }
    let mut cleared = 0usize;
    for (rx, rz, indices) in selections {
        if indices.is_empty() {
            continue;
        }
        let path = region_dir.join(format!("r.{}.{}.mca", rx, rz));
        if !path.is_file() {
            continue;
        }
        let mut data = std::fs::read(&path).map_err(|e| e.to_string())?;
        if data.len() < HEADER_SIZE {
            continue;
        }
        for &index in indices {
            if index >= 1024 {
                continue;
            }
            let loc_off = index * LOC_ENTRY;
            for b in &mut data[loc_off..loc_off + LOC_ENTRY] {
                *b = 0;
            }
            let ts_off = SECTOR + index * LOC_ENTRY;
            for b in &mut data[ts_off..ts_off + 4] {
                *b = 0;
            }
            cleared += 1;
        }
        std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    }
    Ok(cleared)
}

pub fn delete_world_chunks(
    world_dir: &Path,
    selections: &[(i32, i32, Vec<usize>)],
    dimension: Option<&str>,
) -> Result<usize, String> {
    let dim = normalize_dimension(dimension);
    let region_dir = dimension_data_dir(world_dir, dim, "region");
    if !region_dir.is_dir() {
        return Err("no region folder".into());
    }
    // Count from region layer only (entities/poi are side-car deletes).
    let cleared = delete_chunks_in_dir(&region_dir, selections)?;
    let _ = delete_chunks_in_dir(&dimension_data_dir(world_dir, dim, "entities"), selections);
    let _ = delete_chunks_in_dir(&dimension_data_dir(world_dir, dim, "poi"), selections);
    Ok(cleared)
}

fn purge_dir(region_dir: &Path) -> Result<usize, String> {
    if !region_dir.is_dir() {
        return Ok(0);
    }
    let entries = std::fs::read_dir(region_dir).map_err(|e| e.to_string())?;
    let mut rewritten = 0usize;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("mca") {
            continue;
        }
        if compact_region_file(&path)? {
            rewritten += 1;
        }
    }
    Ok(rewritten)
}

/// Compacts all region/entities/poi files in a dimension.
pub fn purge_world_regions(world_dir: &Path, dimension: Option<&str>) -> Result<usize, String> {
    let dim = normalize_dimension(dimension);
    let mut rewritten = purge_dir(&dimension_data_dir(world_dir, dim, "region"))?;
    rewritten += purge_dir(&dimension_data_dir(world_dir, dim, "entities"))?;
    rewritten += purge_dir(&dimension_data_dir(world_dir, dim, "poi"))?;
    Ok(rewritten)
}

/// Exports selected chunks into `dest_dir` as a mini-world (`region`/`entities`/`poi`).
pub fn export_world_chunks(
    world_dir: &Path,
    selections: &[(i32, i32, Vec<usize>)],
    dimension: Option<&str>,
    dest_dir: &Path,
) -> Result<usize, String> {
    let dim = normalize_dimension(dimension);
    let clipboard = copy_world_chunks(world_dir, "export", selections, Some(dim))?;
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    // Paste at offset 0 into dest_dir treated as a world root.
    let pasted = paste_world_chunks(dest_dir, &clipboard, 0, 0, Some("overworld"))?;
    // Also write a tiny marker so the folder is identifiable.
    let _ = std::fs::write(
        dest_dir.join("tuffbox-export.txt"),
        format!(
            "Exported {} chunk(s) from dimension {}\n",
            clipboard.chunks.len(),
            dim
        ),
    );
    Ok(pasted)
}

/// Swaps exactly two selected chunks (region/entities/poi).
pub fn swap_world_chunks(
    world_dir: &Path,
    a: (i32, i32, usize),
    b: (i32, i32, usize),
    dimension: Option<&str>,
) -> Result<(), String> {
    let dim = normalize_dimension(dimension);
    if a == b {
        return Err("cannot swap a chunk with itself".into());
    }
    let selections = vec![(a.0, a.1, vec![a.2]), (b.0, b.1, vec![b.2])];
    let clipboard = copy_world_chunks(world_dir, "swap", &selections, Some(dim))?;
    if clipboard.chunks.len() < 2 {
        return Err("both chunks must be present to swap".into());
    }

    // Build two single-chunk clipboards with swapped coordinates.
    let mut chunk_a = clipboard
        .chunks
        .iter()
        .find(|c| c.region_x == a.0 && c.region_z == a.1 && c.index == a.2)
        .cloned()
        .ok_or("chunk A missing")?;
    let mut chunk_b = clipboard
        .chunks
        .iter()
        .find(|c| c.region_x == b.0 && c.region_z == b.1 && c.index == b.2)
        .cloned()
        .ok_or("chunk B missing")?;

    let (ax, az) = world_chunk_coords(a.0, a.1, a.2);
    let (bx, bz) = world_chunk_coords(b.0, b.1, b.2);

    // Rewrite A→B location and B→A.
    let a_at_b =
        rewrite_chunk_coords(&chunk_a.data, bx, bz, ax, az).unwrap_or_else(|| chunk_a.data.clone());
    let b_at_a =
        rewrite_chunk_coords(&chunk_b.data, ax, az, bx, bz).unwrap_or_else(|| chunk_b.data.clone());

    chunk_a.data = b_at_a;
    chunk_a.region_x = a.0;
    chunk_a.region_z = a.1;
    chunk_a.index = a.2;

    chunk_b.data = a_at_b;
    chunk_b.region_x = b.0;
    chunk_b.region_z = b.1;
    chunk_b.index = b.2;

    // Delete originals then paste swapped.
    delete_world_chunks(world_dir, &selections, Some(dim))?;

    let mut ent = Vec::new();
    let mut poi = Vec::new();
    for src in &clipboard.entities {
        let (tx, tz, tindex, ox, oz) =
            if src.region_x == a.0 && src.region_z == a.1 && src.index == a.2 {
                (b.0, b.1, b.2, ax, az)
            } else if src.region_x == b.0 && src.region_z == b.1 && src.index == b.2 {
                (a.0, a.1, a.2, bx, bz)
            } else {
                continue;
            };
        let (dst_cx, dst_cz) = world_chunk_coords(tx, tz, tindex);
        let mut c = src.clone();
        c.data = rewrite_chunk_coords(&src.data, dst_cx, dst_cz, ox, oz)
            .unwrap_or_else(|| src.data.clone());
        c.region_x = tx;
        c.region_z = tz;
        c.index = tindex;
        ent.push(c);
    }
    for src in &clipboard.poi {
        let (tx, tz, tindex, ox, oz) =
            if src.region_x == a.0 && src.region_z == a.1 && src.index == a.2 {
                (b.0, b.1, b.2, ax, az)
            } else if src.region_x == b.0 && src.region_z == b.1 && src.index == b.2 {
                (a.0, a.1, a.2, bx, bz)
            } else {
                continue;
            };
        let (dst_cx, dst_cz) = world_chunk_coords(tx, tz, tindex);
        let mut c = src.clone();
        c.data = rewrite_chunk_coords(&src.data, dst_cx, dst_cz, ox, oz)
            .unwrap_or_else(|| src.data.clone());
        c.region_x = tx;
        c.region_z = tz;
        c.index = tindex;
        poi.push(c);
    }

    let swap_clip = ChunkClipboard {
        source_world: "swap".into(),
        chunks: vec![chunk_a, chunk_b],
        bounds: (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)),
        entities: ent,
        poi,
    };
    paste_world_chunks(world_dir, &swap_clip, 0, 0, Some(dim))?;
    Ok(())
}

fn compact_region_file(path: &Path) -> Result<bool, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    if data.len() < HEADER_SIZE {
        return Ok(false);
    }

    let mut new_data = vec![0u8; HEADER_SIZE];
    let mut next_sector = 2usize;
    let mut any = false;

    for index in 0..1024 {
        let loc_off = index * LOC_ENTRY;
        let offset = ((data[loc_off] as usize) << 16)
            | ((data[loc_off + 1] as usize) << 8)
            | (data[loc_off + 2] as usize);
        let sector_count = data[loc_off + 3] as usize;
        if offset == 0 || sector_count == 0 {
            continue;
        }
        let src = offset * SECTOR;
        let len = sector_count * SECTOR;
        if src + len > data.len() {
            continue;
        }

        let dst = next_sector * SECTOR;
        let needed = dst + len;
        if new_data.len() < needed {
            new_data.resize(needed, 0);
        }
        new_data[dst..dst + len].copy_from_slice(&data[src..src + len]);

        new_data[loc_off] = ((next_sector >> 16) & 0xFF) as u8;
        new_data[loc_off + 1] = ((next_sector >> 8) & 0xFF) as u8;
        new_data[loc_off + 2] = (next_sector & 0xFF) as u8;
        new_data[loc_off + 3] = sector_count as u8;

        let ts_off = SECTOR + index * LOC_ENTRY;
        new_data[ts_off..ts_off + 4].copy_from_slice(&data[ts_off..ts_off + 4]);

        next_sector += sector_count;
        any = true;
    }

    if !any {
        // All empty — delete the region file.
        let _ = std::fs::remove_file(path);
        return Ok(true);
    }

    if new_data.len() != data.len() || new_data != data {
        std::fs::write(path, &new_data).map_err(|e| e.to_string())?;
        return Ok(true);
    }
    Ok(false)
}

fn read_u16(data: &[u8], pos: &mut usize) -> u16 {
    if *pos + 2 > data.len() {
        return 0;
    }
    let v = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    v
}

fn read_u32(data: &[u8], pos: usize) -> u32 {
    if pos + 4 > data.len() {
        return 0;
    }
    u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

fn read_i32(data: &[u8], pos: &mut usize) -> i32 {
    if *pos + 4 > data.len() {
        return 0;
    }
    let v = i32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    v
}

/// Color mode for world map PNG export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapColorMode {
    Status,
    Date,
    Inhabited,
    Biome,
    Height,
}

impl MapColorMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "date" | "modified" | "timestamp" => Self::Date,
            "inhabited" | "inhabitedtime" => Self::Inhabited,
            "biome" | "biomes" => Self::Biome,
            "height" | "surface" | "y" => Self::Height,
            _ => Self::Status,
        }
    }
}

fn heat_rgb(t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let r = (40.0 + t * 200.0) as u8;
    let g = (90.0 + t * 120.0) as u8;
    let b = (200.0 - t * 170.0) as u8;
    (r, g, b)
}

fn biome_hue_rgb(id: i32) -> (u8, u8, u8) {
    let h = ((id as u32) ^ 0x9e3779b9).wrapping_mul(0x85ebca6b);
    let hue = (h % 360) as f32;
    let s = 0.55f32;
    let v = 0.55f32;
    let c = v * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (rp, gp, bp) = if hue < 60.0 {
        (c, x, 0.0)
    } else if hue < 120.0 {
        (x, c, 0.0)
    } else if hue < 180.0 {
        (0.0, c, x)
    } else if hue < 240.0 {
        (0.0, x, c)
    } else if hue < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((rp + m) * 255.0) as u8,
        ((gp + m) * 255.0) as u8,
        ((bp + m) * 255.0) as u8,
    )
}

fn chunk_rgb(
    cell: &ChunkCell,
    mode: MapColorMode,
    min_mod: u64,
    max_mod: u64,
    min_inh: u64,
    max_inh: u64,
    min_surf: i16,
    max_surf: i16,
) -> (u8, u8, u8) {
    if cell.present == 0 {
        return (21, 23, 28);
    }
    match mode {
        MapColorMode::Date => {
            let span = (max_mod.saturating_sub(min_mod)).max(1) as f32;
            let t = (cell.last_modified.saturating_sub(min_mod) as f32) / span;
            heat_rgb(t)
        }
        MapColorMode::Inhabited => {
            let span = (max_inh.saturating_sub(min_inh)).max(1) as f32;
            let t = (cell.inhabited_time.saturating_sub(min_inh) as f32) / span;
            heat_rgb(t)
        }
        MapColorMode::Biome => {
            if cell.biome_id < 0 {
                (26, 28, 34)
            } else {
                biome_hue_rgb(cell.biome_id)
            }
        }
        MapColorMode::Height => {
            if cell.surface_y == -9999 {
                (21, 23, 28)
            } else {
                let span = (max_surf as i32 - min_surf as i32).max(1) as f32;
                let t = (cell.surface_y as i32 - min_surf as i32) as f32 / span;
                heat_rgb(t)
            }
        }
        MapColorMode::Status => match cell.status {
            STATUS_EMPTY => (59, 66, 82),
            STATUS_PARTIAL => (176, 137, 104),
            STATUS_FULL => {
                let span = (max_mod.saturating_sub(min_mod)).max(1) as f32;
                let t = (cell.last_modified.saturating_sub(min_mod) as f32) / span;
                (
                    (27.0 + t * 12.0) as u8,
                    (120.0 + t * 60.0) as u8,
                    (70.0 + t * 60.0) as u8,
                )
            }
            _ => (74, 140, 90),
        },
    }
}

fn png_crc(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = if crc & 1 != 0 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

fn png_chunk(out: &mut Vec<u8>, typ: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(data);
    let mut crc_buf = Vec::with_capacity(4 + data.len());
    crc_buf.extend_from_slice(typ);
    crc_buf.extend_from_slice(data);
    out.extend_from_slice(&png_crc(&crc_buf).to_be_bytes());
}

fn encode_png_rgb(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    if rgb.len() != (width as usize) * (height as usize) * 3 {
        return Err("rgb buffer size mismatch".into());
    }
    let mut raw = Vec::with_capacity((width as usize * 3 + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0);
        let start = y * width as usize * 3;
        raw.extend_from_slice(&rgb[start..start + width as usize * 3]);
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw).map_err(|e| e.to_string())?;
    let compressed = enc.finish().map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &compressed);
    png_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Render a world dimension (or selection) to a PNG file.
///
/// `pixels_per_chunk` is typically 1–8. When `selections` is non-empty, only those
/// chunks are drawn (others stay dark).
pub fn render_world_map_png(
    world_dir: &Path,
    dimension: Option<&str>,
    selections: &[(i32, i32, Vec<usize>)],
    color_mode: MapColorMode,
    pixels_per_chunk: u32,
    out_path: &Path,
) -> Result<(u32, u32), String> {
    let ppc = pixels_per_chunk.clamp(1, 16);
    let map = read_world_map(world_dir, dimension)?;
    if map.regions.is_empty() || map.total_present == 0 {
        return Err("no chunks to render".into());
    }

    let sel_set: Option<HashSet<(i32, i32, usize)>> = if selections.is_empty() {
        None
    } else {
        Some(selection_index_set(selections))
    };

    let mut min_mod = u64::MAX;
    let mut max_mod = 0u64;
    let mut min_inh = u64::MAX;
    let mut max_inh = 0u64;
    let mut min_surf = i16::MAX;
    let mut max_surf = i16::MIN;
    for region in &map.regions {
        for cell in &region.chunks {
            if cell.present == 0 {
                continue;
            }
            min_mod = min_mod.min(cell.last_modified);
            max_mod = max_mod.max(cell.last_modified);
            min_inh = min_inh.min(cell.inhabited_time);
            max_inh = max_inh.max(cell.inhabited_time);
            if cell.surface_y != -9999 {
                min_surf = min_surf.min(cell.surface_y);
                max_surf = max_surf.max(cell.surface_y);
            }
        }
    }
    if min_mod == u64::MAX {
        min_mod = 0;
    }
    if min_inh == u64::MAX {
        min_inh = 0;
    }
    if min_surf == i16::MAX {
        min_surf = 0;
        max_surf = 0;
    }

    let region_w = (map.max_region_x - map.min_region_x + 1) as u32;
    let region_h = (map.max_region_z - map.min_region_z + 1) as u32;
    let width = region_w * 32 * ppc;
    let height = region_h * 32 * ppc;
    if width == 0 || height == 0 {
        return Err("empty map bounds".into());
    }
    let mut rgb = vec![0u8; (width as usize) * (height as usize) * 3];
    for px in rgb.chunks_exact_mut(3) {
        px[0] = 21;
        px[1] = 23;
        px[2] = 28;
    }

    let put_px = |rgb: &mut [u8], x: u32, y: u32, color: (u8, u8, u8)| {
        if x >= width || y >= height {
            return;
        }
        let i = ((y as usize) * width as usize + x as usize) * 3;
        rgb[i] = color.0;
        rgb[i + 1] = color.1;
        rgb[i + 2] = color.2;
    };

    for region in &map.regions {
        let base_cx = (region.region_x - map.min_region_x) * 32;
        let base_cz = (region.region_z - map.min_region_z) * 32;
        for (index, cell) in region.chunks.iter().enumerate() {
            if cell.present == 0 {
                continue;
            }
            if let Some(ref set) = sel_set {
                if !set.contains(&(region.region_x, region.region_z, index)) {
                    continue;
                }
            }
            let color = chunk_rgb(
                cell, color_mode, min_mod, max_mod, min_inh, max_inh, min_surf, max_surf,
            );
            let lx = (index % 32) as i32;
            let lz = (index / 32) as i32;
            let px0 = ((base_cx + lx) as u32) * ppc;
            let py0 = ((base_cz + lz) as u32) * ppc;
            for dy in 0..ppc {
                for dx in 0..ppc {
                    put_px(&mut rgb, px0 + dx, py0 + dy, color);
                }
            }
        }
    }

    let png = encode_png_rgb(width, height, &rgb)?;
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(out_path, png).map_err(|e| e.to_string())?;
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_entry_stride_is_four() {
        assert_eq!(LOC_ENTRY, 4);
        // 1024 entries * 4 = 4096 location table
        assert_eq!(1024 * LOC_ENTRY, SECTOR);
    }

    #[test]
    fn region_and_index_roundtrip() {
        for cx in -40..40 {
            for cz in -40..40 {
                let (rx, rz, idx) = region_and_index(cx, cz);
                let (ox, oz) = world_chunk_coords(rx, rz, idx);
                assert_eq!((ox, oz), (cx, cz));
            }
        }
    }

    #[test]
    fn empty_map_error_reports_mca_count_when_parse_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let world = dir.path().join("world");
        let region = world.join("region");
        std::fs::create_dir_all(&region).expect("mkdir");
        std::fs::write(region.join("r.0.0.mca"), b"too small").expect("write mca");
        let err = read_world_map_cached(&world, Some("overworld"), false).unwrap_err();
        assert!(
            err.contains("1 .mca on disk") && err.contains("0 parsed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn empty_region_folder_keeps_simple_message() {
        let dir = tempfile::tempdir().expect("tempdir");
        let world = dir.path().join("world");
        std::fs::create_dir_all(world.join("region")).expect("mkdir");
        let err = read_world_map_cached(&world, Some("overworld"), false).unwrap_err();
        assert_eq!(err, "no region files found");
    }
}
