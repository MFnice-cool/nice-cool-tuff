//! MCA Selector-style NBT tree editor and bulk NBT Changer.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

use crate::region::{
    compress_chunk_blob, decompress_chunk_blob, dimension_data_dir, load_chunk_raw,
    normalize_dimension, save_chunk_raw, ChunkMetaExtras,
};

/// JSON NBT node for the Chunk Editor UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NbtNode {
    pub tag_type: u8,
    pub name: String,
    /// Scalar / array payload as JSON; compounds/lists use `children`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NbtNode>,
    /// For TAG_List: element type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_type: Option<u8>,
}

/// Payload returned by read_chunk_editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkEditorData {
    pub region_x: i32,
    pub region_z: i32,
    pub index: usize,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub layer: String,
    pub root: NbtNode,
}

/// Bulk NBT Changer fields (only `Some` / true are applied).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NbtChangeRequest {
    pub inhabited_time: Option<i64>,
    pub status: Option<String>,
    pub data_version: Option<i32>,
    pub light_populated: Option<u8>,
    pub last_update: Option<i64>,
    /// Fill entire chunk biomes with this name or numeric id (e.g. `plains` / `1`).
    pub biome: Option<String>,
    /// Delete sections by Y range, MCA Selector syntax: `all`, `:-4`, `0:4`, `3`, `:-4,0:2`.
    pub delete_sections: Option<String>,
    /// Comma-separated `from=to` block name replacements in section palettes.
    pub replace_blocks: Option<String>,
    /// Delete named structure references (comma-separated).
    pub delete_structure_refs: Option<String>,
    /// Remove `below_zero_retrogen` and keep Status full (1.18+).
    #[serde(default)]
    pub prevent_retrogen: bool,
    /// Force 1.18+ chunk blending by writing/resetting `blending_data`.
    #[serde(default)]
    pub force_blend: bool,
    /// If true, clear Entities / entities / TileEntities / block_entities lists.
    #[serde(default)]
    pub delete_entities: bool,
    /// Fix Status=empty when terrain sections exist → full.
    #[serde(default)]
    pub fix_status: bool,
    /// Force-create missing tags when possible.
    #[serde(default)]
    pub force: bool,
}

/// Advanced content filter (scans chunk NBT).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedChunkFilter {
    /// Comma-separated substrings matched against entity `id` / `Id`.
    pub entity_names: Option<String>,
    /// Comma-separated substrings matched against structure keys.
    pub structure_names: Option<String>,
    /// Comma-separated block names matched against section palettes.
    pub palette_names: Option<String>,
    pub min_entities: Option<u32>,
    pub max_entities: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkRef {
    pub region_x: i32,
    pub region_z: i32,
    pub index: usize,
}

fn read_u16(data: &[u8], pos: &mut usize) -> u16 {
    if *pos + 2 > data.len() {
        return 0;
    }
    let v = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    v
}

fn read_i32(data: &[u8], pos: &mut usize) -> i32 {
    if *pos + 4 > data.len() {
        return 0;
    }
    let v = i32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    v
}

fn parse_payload(data: &[u8], pos: &mut usize, tag_type: u8, name: String) -> Option<NbtNode> {
    match tag_type {
        1 => {
            let v = *data.get(*pos)? as i8;
            *pos += 1;
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(v)),
                children: vec![],
                list_type: None,
            })
        }
        2 => {
            let v = i16::from_be_bytes([*data.get(*pos)?, *data.get(*pos + 1)?]);
            *pos += 2;
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(v)),
                children: vec![],
                list_type: None,
            })
        }
        3 => {
            let v = read_i32(data, pos);
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(v)),
                children: vec![],
                list_type: None,
            })
        }
        4 => {
            if *pos + 8 > data.len() {
                return None;
            }
            let v = i64::from_be_bytes(data[*pos..*pos + 8].try_into().ok()?);
            *pos += 8;
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(v)),
                children: vec![],
                list_type: None,
            })
        }
        5 => {
            if *pos + 4 > data.len() {
                return None;
            }
            let v = f32::from_be_bytes(data[*pos..*pos + 4].try_into().ok()?);
            *pos += 4;
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(v)),
                children: vec![],
                list_type: None,
            })
        }
        6 => {
            if *pos + 8 > data.len() {
                return None;
            }
            let v = f64::from_be_bytes(data[*pos..*pos + 8].try_into().ok()?);
            *pos += 8;
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(v)),
                children: vec![],
                list_type: None,
            })
        }
        7 => {
            let len = read_i32(data, pos) as usize;
            if *pos + len > data.len() {
                return None;
            }
            let arr: Vec<i8> = data[*pos..*pos + len].iter().map(|b| *b as i8).collect();
            *pos += len;
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(arr)),
                children: vec![],
                list_type: None,
            })
        }
        8 => {
            let len = read_u16(data, pos) as usize;
            if *pos + len > data.len() {
                return None;
            }
            let s = String::from_utf8_lossy(&data[*pos..*pos + len]).to_string();
            *pos += len;
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(s)),
                children: vec![],
                list_type: None,
            })
        }
        9 => {
            let list_type = *data.get(*pos)?;
            *pos += 1;
            let len = read_i32(data, pos) as usize;
            let mut children = Vec::with_capacity(len.min(4096));
            for i in 0..len {
                if list_type == 10 {
                    children.push(parse_compound_body(data, pos, format!("[{i}]"))?);
                } else {
                    children.push(parse_payload(data, pos, list_type, format!("[{i}]"))?);
                }
            }
            Some(NbtNode {
                tag_type,
                name,
                value: None,
                children,
                list_type: Some(list_type),
            })
        }
        10 => parse_compound_body(data, pos, name),
        11 => {
            let len = read_i32(data, pos) as usize;
            if *pos + len * 4 > data.len() {
                return None;
            }
            let mut arr = Vec::with_capacity(len.min(65536));
            for _ in 0..len {
                arr.push(read_i32(data, pos));
            }
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(arr)),
                children: vec![],
                list_type: None,
            })
        }
        12 => {
            let len = read_i32(data, pos) as usize;
            if *pos + len * 8 > data.len() {
                return None;
            }
            let mut arr = Vec::with_capacity(len.min(65536));
            for _ in 0..len {
                let v = i64::from_be_bytes(data[*pos..*pos + 8].try_into().ok()?);
                *pos += 8;
                arr.push(v);
            }
            Some(NbtNode {
                tag_type,
                name,
                value: Some(json!(arr)),
                children: vec![],
                list_type: None,
            })
        }
        _ => None,
    }
}

fn parse_compound_body(data: &[u8], pos: &mut usize, name: String) -> Option<NbtNode> {
    let mut children = Vec::new();
    loop {
        let tag_type = *data.get(*pos)?;
        *pos += 1;
        if tag_type == 0 {
            break;
        }
        let nlen = read_u16(data, pos) as usize;
        if *pos + nlen > data.len() {
            return None;
        }
        let child_name = String::from_utf8_lossy(&data[*pos..*pos + nlen]).to_string();
        *pos += nlen;
        children.push(parse_payload(data, pos, tag_type, child_name)?);
    }
    Some(NbtNode {
        tag_type: 10,
        name,
        value: None,
        children,
        list_type: None,
    })
}

fn parse_root(nbt: &[u8]) -> Option<NbtNode> {
    if nbt.is_empty() || nbt[0] != 10 {
        return None;
    }
    let mut pos = 1usize;
    let nlen = read_u16(nbt, &mut pos) as usize;
    if pos + nlen > nbt.len() {
        return None;
    }
    let name = String::from_utf8_lossy(&nbt[pos..pos + nlen]).to_string();
    pos += nlen;
    parse_compound_body(nbt, &mut pos, name)
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = (bytes.len() as u16).to_be_bytes();
    out.extend_from_slice(&len);
    out.extend_from_slice(bytes);
}

fn write_payload(out: &mut Vec<u8>, node: &NbtNode) -> Result<(), String> {
    match node.tag_type {
        1 => {
            let v = node.value.as_ref().and_then(|v| v.as_i64()).unwrap_or(0) as i8;
            out.push(v as u8);
        }
        2 => {
            let v = node.value.as_ref().and_then(|v| v.as_i64()).unwrap_or(0) as i16;
            out.extend_from_slice(&v.to_be_bytes());
        }
        3 => {
            let v = node.value.as_ref().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            out.extend_from_slice(&v.to_be_bytes());
        }
        4 => {
            let v = node.value.as_ref().and_then(|v| v.as_i64()).unwrap_or(0);
            out.extend_from_slice(&v.to_be_bytes());
        }
        5 => {
            let v = node.value.as_ref().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            out.extend_from_slice(&v.to_be_bytes());
        }
        6 => {
            let v = node.value.as_ref().and_then(|v| v.as_f64()).unwrap_or(0.0);
            out.extend_from_slice(&v.to_be_bytes());
        }
        7 => {
            let arr = node
                .value
                .as_ref()
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            out.extend_from_slice(&(arr.len() as i32).to_be_bytes());
            for item in arr {
                out.push(item.as_i64().unwrap_or(0) as u8);
            }
        }
        8 => {
            let s = node
                .value
                .as_ref()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            write_string(out, &s);
        }
        9 => {
            let list_type = node.list_type.unwrap_or(0);
            out.push(list_type);
            out.extend_from_slice(&(node.children.len() as i32).to_be_bytes());
            for child in &node.children {
                if list_type == 10 {
                    write_compound_body(out, child)?;
                } else {
                    write_payload(out, child)?;
                }
            }
        }
        10 => write_compound_body(out, node)?,
        11 => {
            let arr = node
                .value
                .as_ref()
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            out.extend_from_slice(&(arr.len() as i32).to_be_bytes());
            for item in arr {
                let v = item.as_i64().unwrap_or(0) as i32;
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        12 => {
            let arr = node
                .value
                .as_ref()
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            out.extend_from_slice(&(arr.len() as i32).to_be_bytes());
            for item in arr {
                let v = item.as_i64().unwrap_or(0);
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        _ => return Err(format!("unsupported tag type {}", node.tag_type)),
    }
    Ok(())
}

fn write_compound_body(out: &mut Vec<u8>, node: &NbtNode) -> Result<(), String> {
    for child in &node.children {
        out.push(child.tag_type);
        write_string(out, &child.name);
        write_payload(out, child)?;
    }
    out.push(0); // TAG_End
    Ok(())
}

fn write_root(node: &NbtNode) -> Result<Vec<u8>, String> {
    if node.tag_type != 10 {
        return Err("root must be compound".into());
    }
    let mut out = Vec::new();
    out.push(10);
    write_string(&mut out, &node.name);
    write_compound_body(&mut out, node)?;
    Ok(out)
}

/// Reads one chunk's NBT as a tree (region / entities / poi).
pub fn read_chunk_editor(
    world_dir: &Path,
    region_x: i32,
    region_z: i32,
    index: usize,
    dimension: Option<&str>,
    layer: Option<&str>,
) -> Result<ChunkEditorData, String> {
    let dim = normalize_dimension(dimension);
    let layer = layer.unwrap_or("region");
    let dir = dimension_data_dir(world_dir, dim, layer);
    let (blob, _) = load_chunk_raw(&dir, region_x, region_z, index)?
        .ok_or_else(|| "chunk not present".to_string())?;
    let nbt = decompress_chunk_blob(&blob).ok_or("failed to decompress chunk")?;
    let root = parse_root(&nbt).ok_or("failed to parse NBT")?;
    let lx = (index % 32) as i32;
    let lz = (index / 32) as i32;
    Ok(ChunkEditorData {
        region_x,
        region_z,
        index,
        chunk_x: region_x * 32 + lx,
        chunk_z: region_z * 32 + lz,
        layer: layer.to_string(),
        root,
    })
}

/// Writes an edited NBT tree back into the MCA file.
pub fn write_chunk_editor(
    world_dir: &Path,
    data: &ChunkEditorData,
    dimension: Option<&str>,
) -> Result<(), String> {
    let dim = normalize_dimension(dimension);
    let dir = dimension_data_dir(world_dir, dim, &data.layer);
    let nbt = write_root(&data.root)?;
    // Preserve compression type from existing blob when possible.
    let compression = load_chunk_raw(&dir, data.region_x, data.region_z, data.index)?
        .and_then(|(blob, _)| blob.get(4).copied())
        .unwrap_or(2);
    let new_blob = compress_chunk_blob(compression, &nbt).ok_or("failed to compress")?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    save_chunk_raw(
        &dir,
        data.region_x,
        data.region_z,
        data.index,
        &new_blob,
        ts,
    )
}

/// Applies NBT Changer fields to selected chunks.
pub fn change_world_chunks(
    world_dir: &Path,
    selections: &[(i32, i32, Vec<usize>)],
    change: &NbtChangeRequest,
    dimension: Option<&str>,
) -> Result<usize, String> {
    let dim = normalize_dimension(dimension);
    let region_dir = dimension_data_dir(world_dir, dim, "region");
    let mut changed = 0usize;

    for (rx, rz, indices) in selections {
        for &index in indices {
            let Some((blob, ts)) = load_chunk_raw(&region_dir, *rx, *rz, index)? else {
                continue;
            };
            let Some(mut nbt) = decompress_chunk_blob(&blob) else {
                continue;
            };
            let mut dirty = false;

            if let Some(v) = change.inhabited_time {
                if set_or_insert_long(&mut nbt, "InhabitedTime", v, change.force) {
                    dirty = true;
                }
            }
            if let Some(ref s) = change.status {
                if set_or_insert_string(&mut nbt, "Status", s, change.force) {
                    dirty = true;
                }
            }
            if let Some(v) = change.data_version {
                if set_or_insert_int(&mut nbt, "DataVersion", v, change.force) {
                    dirty = true;
                }
            }
            if let Some(v) = change.light_populated {
                if set_or_insert_byte(&mut nbt, "LightPopulated", v as i8, change.force) {
                    dirty = true;
                }
            }
            if let Some(v) = change.last_update {
                if set_or_insert_long(&mut nbt, "LastUpdate", v, change.force) {
                    dirty = true;
                }
            }
            if change.delete_entities {
                if clear_named_lists(
                    &mut nbt,
                    &[
                        "Entities",
                        "entities",
                        "TileEntities",
                        "tile_entities",
                        "block_entities",
                        "blockEntities",
                    ],
                ) {
                    dirty = true;
                }
                // Also clear entities MCA entry for this chunk.
                let ent_dir = dimension_data_dir(world_dir, dim, "entities");
                if let Ok(Some(_)) = load_chunk_raw(&ent_dir, *rx, *rz, index) {
                    let _ = crate::region::delete_single_chunk(&ent_dir, *rx, *rz, index);
                }
            }
            if change.fix_status {
                if fix_empty_status(&mut nbt) {
                    dirty = true;
                }
            }
            if change.prevent_retrogen
                || change.force_blend
                || change.biome.is_some()
                || change.delete_sections.is_some()
                || change.replace_blocks.is_some()
                || change.delete_structure_refs.is_some()
            {
                if apply_tree_changes(&mut nbt, change) {
                    dirty = true;
                }
            }

            if !dirty {
                continue;
            }
            let compression = blob.get(4).copied().unwrap_or(2);
            let Some(new_blob) = compress_chunk_blob(compression, &nbt) else {
                continue;
            };
            save_chunk_raw(&region_dir, *rx, *rz, index, &new_blob, ts)?;
            changed += 1;
        }
    }
    Ok(changed)
}

fn apply_tree_changes(nbt: &mut Vec<u8>, change: &NbtChangeRequest) -> bool {
    let Some(mut root) = parse_root(nbt) else {
        return false;
    };
    let mut dirty = false;

    if change.prevent_retrogen {
        if remove_child(&mut root, "below_zero_retrogen") {
            dirty = true;
        }
        // Ensure status is full-ish.
        if set_child_string(&mut root, "Status", "minecraft:full") {
            dirty = true;
        }
    }

    if change.force_blend {
        if force_blend_data(&mut root) {
            dirty = true;
        }
    }

    if let Some(ref biome) = change.biome {
        if fill_biomes(&mut root, biome) {
            dirty = true;
        }
    }

    if let Some(ref spec) = change.delete_sections {
        if delete_sections(&mut root, spec) {
            dirty = true;
        }
    }

    if let Some(ref spec) = change.replace_blocks {
        if replace_blocks_in_palette(&mut root, spec) {
            dirty = true;
        }
    }

    if let Some(ref spec) = change.delete_structure_refs {
        if delete_structure_refs(&mut root, spec) {
            dirty = true;
        }
    }

    if !dirty {
        return false;
    }
    match write_root(&root) {
        Ok(bytes) => {
            *nbt = bytes;
            true
        }
        Err(_) => false,
    }
}

fn find_child_mut<'a>(root: &'a mut NbtNode, name: &str) -> Option<&'a mut NbtNode> {
    root.children.iter_mut().find(|c| c.name == name)
}

fn remove_child(root: &mut NbtNode, name: &str) -> bool {
    let before = root.children.len();
    root.children.retain(|c| c.name != name);
    root.children.len() != before
}

fn set_child_string(root: &mut NbtNode, name: &str, value: &str) -> bool {
    if let Some(child) = find_child_mut(root, name) {
        if child.tag_type == 8 {
            let old = child.value.clone();
            child.value = Some(json!(value));
            return old != child.value;
        }
    }
    root.children.push(NbtNode {
        tag_type: 8,
        name: name.to_string(),
        value: Some(json!(value)),
        children: vec![],
        list_type: None,
    });
    true
}

fn normalize_block_name(s: &str) -> String {
    let t = s.trim().trim_matches('\'').to_string();
    if t.contains(':') {
        t
    } else {
        format!("minecraft:{t}")
    }
}

fn parse_csv_tokens(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().trim_matches('\'').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn parse_section_ranges(spec: &str) -> Vec<(Option<i32>, Option<i32>)> {
    // Returns inclusive ranges; (None,None) means all.
    let mut out = Vec::new();
    let s = spec.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("all") || s == ":" || s.eq_ignore_ascii_case("true") {
        out.push((None, None));
        return out;
    }
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once(':') {
            let min = if a.trim().is_empty() {
                None
            } else {
                a.trim().parse().ok()
            };
            let max = if b.trim().is_empty() {
                None
            } else {
                b.trim().parse().ok()
            };
            out.push((min, max));
        } else if let Ok(y) = part.parse::<i32>() {
            out.push((Some(y), Some(y)));
        }
    }
    out
}

fn section_y(section: &NbtNode) -> Option<i32> {
    section.children.iter().find_map(|c| {
        if (c.name == "Y" || c.name == "y") && c.tag_type == 1 {
            c.value.as_ref().and_then(|v| v.as_i64()).map(|v| v as i32)
        } else if (c.name == "Y" || c.name == "y") && c.tag_type == 3 {
            c.value.as_ref().and_then(|v| v.as_i64()).map(|v| v as i32)
        } else {
            None
        }
    })
}

fn y_in_ranges(y: i32, ranges: &[(Option<i32>, Option<i32>)]) -> bool {
    for (min, max) in ranges {
        match (min, max) {
            (None, None) => return true,
            (Some(a), Some(b)) => {
                if y >= *a && y <= *b {
                    return true;
                }
            }
            (Some(a), None) => {
                if y >= *a {
                    return true;
                }
            }
            (None, Some(b)) => {
                if y <= *b {
                    return true;
                }
            }
        }
    }
    false
}

fn delete_sections(root: &mut NbtNode, spec: &str) -> bool {
    let ranges = parse_section_ranges(spec);
    if ranges.is_empty() {
        return false;
    }
    let mut dirty = false;
    for name in ["sections", "Sections"] {
        if let Some(list) = find_child_mut(root, name) {
            if list.tag_type != 9 {
                continue;
            }
            let before = list.children.len();
            list.children.retain(|sec| match section_y(sec) {
                Some(y) => !y_in_ranges(y, &ranges),
                None => true,
            });
            if list.children.len() != before {
                dirty = true;
            }
        }
    }
    // Legacy Level.Sections
    if let Some(level) = find_child_mut(root, "Level") {
        if let Some(list) = find_child_mut(level, "Sections") {
            if list.tag_type == 9 {
                let before = list.children.len();
                list.children.retain(|sec| match section_y(sec) {
                    Some(y) => !y_in_ranges(y, &ranges),
                    None => true,
                });
                if list.children.len() != before {
                    dirty = true;
                }
            }
        }
    }
    dirty
}

/// Keep only sections matching MCA Selector range syntax (opposite of DeleteSections).
fn keep_sections(root: &mut NbtNode, spec: &str) -> bool {
    let ranges = parse_section_ranges(spec);
    if ranges.is_empty() {
        return false;
    }
    // "all" / ":" → keep everything
    if ranges.len() == 1 && ranges[0] == (None, None) {
        return false;
    }
    let mut dirty = false;
    let retain = |sec: &NbtNode| match section_y(sec) {
        Some(y) => y_in_ranges(y, &ranges),
        None => true,
    };
    for name in ["sections", "Sections"] {
        if let Some(list) = find_child_mut(root, name) {
            if list.tag_type != 9 {
                continue;
            }
            let before = list.children.len();
            list.children.retain(retain);
            if list.children.len() != before {
                dirty = true;
            }
        }
    }
    if let Some(level) = find_child_mut(root, "Level") {
        if let Some(list) = find_child_mut(level, "Sections") {
            if list.tag_type == 9 {
                let before = list.children.len();
                list.children.retain(retain);
                if list.children.len() != before {
                    dirty = true;
                }
            }
        }
    }
    dirty
}

fn set_section_y(section: &mut NbtNode, y: i32) {
    for c in &mut section.children {
        if c.name == "Y" || c.name == "y" {
            if c.tag_type == 1 {
                c.value = Some(json!(y as i8));
            } else if c.tag_type == 3 {
                c.value = Some(json!(y));
            }
            return;
        }
    }
}

fn shift_sections_y(root: &mut NbtNode, delta: i32) -> bool {
    if delta == 0 {
        return false;
    }
    let mut dirty = false;
    let shift_list = |list: &mut NbtNode| {
        let mut d = false;
        for sec in &mut list.children {
            if let Some(y) = section_y(sec) {
                set_section_y(sec, y + delta);
                d = true;
            }
        }
        d
    };
    for name in ["sections", "Sections"] {
        if let Some(list) = find_child_mut(root, name) {
            if list.tag_type == 9 && shift_list(list) {
                dirty = true;
            }
        }
    }
    if let Some(level) = find_child_mut(root, "Level") {
        if let Some(list) = find_child_mut(level, "Sections") {
            if list.tag_type == 9 && shift_list(list) {
                dirty = true;
            }
        }
    }
    dirty
}

fn shift_numeric_y_field(node: &mut NbtNode, block_delta: f64) -> bool {
    let mut dirty = false;
    // Pos: [x, y, z] double list
    if let Some(pos) = find_child_mut(node, "Pos") {
        if pos.tag_type == 9 && pos.list_type == Some(6) && pos.children.len() >= 2 {
            if let Some(Value::Number(n)) = pos.children[1].value.clone() {
                if let Some(y) = n.as_f64() {
                    pos.children[1].value = Some(json!(y + block_delta));
                    dirty = true;
                }
            }
        }
    }
    for y_name in ["y", "Y"] {
        if let Some(y_node) = find_child_mut(node, y_name) {
            if let Some(Value::Number(n)) = y_node.value.clone() {
                if y_node.tag_type == 5 || y_node.tag_type == 6 {
                    if let Some(y) = n.as_f64() {
                        y_node.value = Some(json!(y + block_delta));
                        dirty = true;
                    }
                } else if let Some(y) = n.as_i64() {
                    y_node.value = Some(json!(y + block_delta as i64));
                    dirty = true;
                }
            }
        }
    }
    dirty
}

fn shift_entities_y(root: &mut NbtNode, block_delta: f64) -> bool {
    if block_delta == 0.0 {
        return false;
    }
    let mut dirty = false;
    for list_name in [
        "Entities",
        "entities",
        "block_entities",
        "TileEntities",
        "blockEntities",
    ] {
        if let Some(list) = find_child_mut(root, list_name) {
            if list.tag_type != 9 {
                continue;
            }
            for ent in &mut list.children {
                if shift_numeric_y_field(ent, block_delta) {
                    dirty = true;
                }
            }
        }
    }
    if let Some(level) = find_child_mut(root, "Level") {
        for list_name in ["Entities", "TileEntities"] {
            if let Some(list) = find_child_mut(level, list_name) {
                if list.tag_type != 9 {
                    continue;
                }
                for ent in &mut list.children {
                    if shift_numeric_y_field(ent, block_delta) {
                        dirty = true;
                    }
                }
            }
        }
    }
    dirty
}

/// Apply MCA Selector import transforms: keep sections + vertical section offset.
pub fn transform_import_chunk_blob(
    blob: &[u8],
    sections: Option<&str>,
    y_offset_sections: i32,
) -> Result<Vec<u8>, String> {
    let compression = if blob.len() >= 5 { blob[4] } else { 2 };
    let nbt = decompress_chunk_blob(blob).ok_or("failed to decompress chunk")?;
    let mut root = parse_root(&nbt).ok_or("failed to parse NBT")?;
    let mut dirty = false;
    if let Some(spec) = sections {
        let s = spec.trim();
        if !s.is_empty() && !s.eq_ignore_ascii_case("all") && s != ":" {
            if keep_sections(&mut root, s) {
                dirty = true;
            }
        }
    }
    if y_offset_sections != 0 {
        if shift_sections_y(&mut root, y_offset_sections) {
            dirty = true;
        }
        let block_delta = (y_offset_sections as f64) * 16.0;
        if shift_entities_y(&mut root, block_delta) {
            dirty = true;
        }
    }
    if !dirty {
        return Ok(blob.to_vec());
    }
    let out_nbt = write_root(&root)?;
    compress_chunk_blob(compression, &out_nbt).ok_or_else(|| "failed to recompress chunk".into())
}

fn fill_biomes(root: &mut NbtNode, biome: &str) -> bool {
    let mut dirty = false;
    let name = normalize_block_name(biome);
    let numeric: Option<i32> = biome.trim().parse().ok();

    // Legacy Biomes arrays at root
    for biomes_name in ["Biomes", "biomes"] {
        if let Some(arr) = find_child_mut(root, biomes_name) {
            if let Some(id) = numeric {
                if arr.tag_type == 7 || arr.tag_type == 11 {
                    if let Some(Value::Array(items)) = arr.value.as_mut() {
                        for item in items.iter_mut() {
                            *item = if arr.tag_type == 7 {
                                json!(id as i8)
                            } else {
                                json!(id)
                            };
                        }
                        dirty = true;
                    }
                }
            }
        }
    }
    // Legacy Level.Biomes
    if let Some(level) = find_child_mut(root, "Level") {
        for biomes_name in ["Biomes", "biomes"] {
            if let Some(arr) = find_child_mut(level, biomes_name) {
                if let Some(id) = numeric {
                    if arr.tag_type == 7 || arr.tag_type == 11 {
                        if let Some(Value::Array(items)) = arr.value.as_mut() {
                            for item in items.iter_mut() {
                                *item = if arr.tag_type == 7 {
                                    json!(id as i8)
                                } else {
                                    json!(id)
                                };
                            }
                            dirty = true;
                        }
                    }
                }
            }
        }
    }

    // Modern section biomes palettes
    for sections_name in ["sections", "Sections"] {
        if let Some(list) = find_child_mut(root, sections_name) {
            if list.tag_type != 9 {
                continue;
            }
            for section in &mut list.children {
                if let Some(biomes) = find_child_mut(section, "biomes") {
                    if biomes.tag_type != 10 {
                        continue;
                    }
                    if let Some(palette) = find_child_mut(biomes, "palette") {
                        if palette.tag_type == 9 {
                            if palette.list_type == Some(8)
                                || palette.children.iter().all(|c| c.tag_type == 8)
                            {
                                palette.children = vec![NbtNode {
                                    tag_type: 8,
                                    name: "[0]".into(),
                                    value: Some(json!(name.clone())),
                                    children: vec![],
                                    list_type: None,
                                }];
                                palette.list_type = Some(8);
                                dirty = true;
                            } else {
                                palette.children = vec![NbtNode {
                                    tag_type: 10,
                                    name: "[0]".into(),
                                    value: None,
                                    children: vec![NbtNode {
                                        tag_type: 8,
                                        name: "Name".into(),
                                        value: Some(json!(name.clone())),
                                        children: vec![],
                                        list_type: None,
                                    }],
                                    list_type: None,
                                }];
                                palette.list_type = Some(10);
                                dirty = true;
                            }
                            biomes.children.retain(|c| c.name != "data");
                        }
                    }
                }
            }
        }
    }
    dirty
}

fn replace_blocks_in_palette(root: &mut NbtNode, spec: &str) -> bool {
    let pairs = parse_replace_block_pairs(spec);
    if pairs.is_empty() {
        return false;
    }
    let mut dirty = false;

    for sections_name in ["sections", "Sections"] {
        if let Some(list) = find_child_mut(root, sections_name) {
            for section in &mut list.children {
                if let Some(bs) = find_child_mut(section, "block_states") {
                    if let Some(palette) = find_child_mut(bs, "palette") {
                        if rewrite_palette_entries(palette, &pairs) {
                            dirty = true;
                        }
                    }
                }
                if let Some(palette) = find_child_mut(section, "Palette") {
                    if rewrite_palette_entries(palette, &pairs) {
                        dirty = true;
                    }
                }
            }
        }
    }
    // Legacy Level.Sections
    if let Some(level) = find_child_mut(root, "Level") {
        if let Some(list) = find_child_mut(level, "Sections") {
            for section in &mut list.children {
                if let Some(palette) = find_child_mut(section, "Palette") {
                    if rewrite_palette_entries(palette, &pairs) {
                        dirty = true;
                    }
                }
            }
        }
    }
    dirty
}

#[derive(Debug, Clone)]
struct BlockPattern {
    name: String,
    /// When Some, Properties must match all listed keys (extra keys allowed).
    props: Option<HashMap<String, String>>,
}

fn parse_replace_block_pairs(spec: &str) -> Vec<(BlockPattern, BlockPattern)> {
    let mut pairs = Vec::new();
    // MCA Selector allows `,` or `;` as pair separators.
    let normalized = spec.replace(';', ",");
    for part in normalized.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((from, to)) = part.split_once('=') {
            pairs.push((parse_block_pattern(from), parse_block_pattern(to)));
        }
    }
    pairs
}

fn parse_block_pattern(raw: &str) -> BlockPattern {
    let s = raw.trim().trim_matches('\'');
    // name[prop=val,prop2=val] or name{prop:val,prop2:val}
    let (name_part, props) = if let Some(open) = s.find('[') {
        let close = s.rfind(']').unwrap_or(s.len());
        let name = &s[..open];
        let body = &s[open + 1..close];
        (name, parse_props_body(body, '='))
    } else if let Some(open) = s.find('{') {
        let close = s.rfind('}').unwrap_or(s.len());
        let name = &s[..open];
        let body = &s[open + 1..close];
        (name, parse_props_body(body, ':'))
    } else {
        (s, None)
    };
    BlockPattern {
        name: normalize_block_name(name_part),
        props,
    }
}

fn parse_props_body(body: &str, sep: char) -> Option<HashMap<String, String>> {
    let body = body.trim();
    if body.is_empty() {
        return Some(HashMap::new());
    }
    let mut map = HashMap::new();
    for part in body.split(',') {
        let part = part.trim().trim_matches('\'');
        if part.is_empty() {
            continue;
        }
        if let Some((k, v)) = part.split_once(sep) {
            map.insert(
                k.trim().trim_matches('"').to_string(),
                v.trim().trim_matches('"').trim_matches('\'').to_string(),
            );
        }
    }
    Some(map)
}

fn palette_entry_name(entry: &NbtNode) -> Option<String> {
    if entry.tag_type == 8 {
        entry
            .value
            .as_ref()
            .and_then(|v| v.as_str())
            .map(normalize_block_name)
    } else if entry.tag_type == 10 {
        entry.children.iter().find_map(|c| {
            if c.name == "Name" {
                c.value
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .map(normalize_block_name)
            } else {
                None
            }
        })
    } else {
        None
    }
}

fn palette_entry_props(entry: &NbtNode) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if entry.tag_type != 10 {
        return map;
    }
    let Some(props) = entry.children.iter().find(|c| c.name == "Properties") else {
        return map;
    };
    for c in &props.children {
        if let Some(Value::String(s)) = &c.value {
            map.insert(c.name.clone(), s.clone());
        } else if let Some(v) = &c.value {
            map.insert(c.name.clone(), v.to_string().trim_matches('"').to_string());
        }
    }
    map
}

fn pattern_matches(entry: &NbtNode, pat: &BlockPattern) -> bool {
    let Some(name) = palette_entry_name(entry) else {
        return false;
    };
    if name != pat.name {
        return false;
    }
    match &pat.props {
        None => true,
        Some(need) => {
            let have = palette_entry_props(entry);
            need.iter()
                .all(|(k, v)| have.get(k).map(|x| x == v).unwrap_or(false))
        }
    }
}

fn apply_block_replacement(entry: &mut NbtNode, to: &BlockPattern) -> bool {
    let mut dirty = false;
    if entry.tag_type == 8 {
        // String-only palette (rare modern) — can only set name
        if let Some(Value::String(s)) = entry.value.as_mut() {
            let next = to.name.clone();
            if *s != next {
                *s = next;
                dirty = true;
            }
        }
        return dirty;
    }
    if entry.tag_type != 10 {
        return false;
    }
    // Ensure Name
    if let Some(name) = entry.children.iter_mut().find(|c| c.name == "Name") {
        if let Some(Value::String(s)) = name.value.as_mut() {
            if *s != to.name {
                *s = to.name.clone();
                dirty = true;
            }
        }
    } else {
        entry.children.push(NbtNode {
            tag_type: 8,
            name: "Name".into(),
            value: Some(json!(to.name.clone())),
            children: vec![],
            list_type: None,
        });
        dirty = true;
    }
    if let Some(ref props) = to.props {
        // Replace Properties entirely with target props
        entry.children.retain(|c| c.name != "Properties");
        let children: Vec<NbtNode> = props
            .iter()
            .map(|(k, v)| NbtNode {
                tag_type: 8,
                name: k.clone(),
                value: Some(json!(v.clone())),
                children: vec![],
                list_type: None,
            })
            .collect();
        if !children.is_empty() {
            entry.children.push(NbtNode {
                tag_type: 10,
                name: "Properties".into(),
                value: None,
                children,
                list_type: None,
            });
        }
        dirty = true;
    }
    dirty
}

fn rewrite_palette_entries(palette: &mut NbtNode, pairs: &[(BlockPattern, BlockPattern)]) -> bool {
    let mut dirty = false;
    for entry in &mut palette.children {
        for (from, to) in pairs {
            if pattern_matches(entry, from) {
                if apply_block_replacement(entry, to) {
                    dirty = true;
                }
                break;
            }
        }
    }
    dirty
}

fn delete_structure_refs(root: &mut NbtNode, spec: &str) -> bool {
    let names = parse_csv_tokens(spec);
    if names.is_empty() {
        return false;
    }
    let mut dirty = false;
    for struct_name in ["structures", "Structures"] {
        if let Some(structures) = find_child_mut(root, struct_name) {
            for child_name in ["References", "starts", "Starts"] {
                if let Some(map) = find_child_mut(structures, child_name) {
                    if map.tag_type == 10 {
                        let before = map.children.len();
                        map.children.retain(|c| {
                            !names.iter().any(|n| {
                                c.name == *n
                                    || c.name == format!("minecraft:{n}")
                                    || c.name.ends_with(&format!(":{n}"))
                            })
                        });
                        if map.children.len() != before {
                            dirty = true;
                        }
                    }
                }
            }
        }
    }
    dirty
}

fn force_blend_data(root: &mut NbtNode) -> bool {
    // Reset blending_data so Minecraft re-blends this chunk with neighbors.
    let _ = remove_child(root, "blending_data");
    root.children.push(NbtNode {
        tag_type: 10,
        name: "blending_data".into(),
        value: None,
        children: vec![
            NbtNode {
                tag_type: 3,
                name: "min_section".into(),
                value: Some(json!(-4)),
                children: vec![],
                list_type: None,
            },
            NbtNode {
                tag_type: 3,
                name: "max_section".into(),
                value: Some(json!(20)),
                children: vec![],
                list_type: None,
            },
        ],
        list_type: None,
    });
    true
}

/// Parse MCA Selector selection CSV into chunk refs.
/// Lines: `rx;rz` (full region listed separately), or `rx;rz;cx;cz`.
/// Optional first line `inverted`.
pub fn parse_selection_csv(text: &str) -> Result<(bool, Vec<ChunkRef>, Vec<(i32, i32)>), String> {
    let mut inverted = false;
    let mut chunks = Vec::new();
    let mut full_regions = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("inverted") {
            inverted = true;
            continue;
        }
        let parts: Vec<&str> = line.split(';').map(|p| p.trim()).collect();
        if parts.len() == 2 {
            let rx: i32 = parts[0]
                .parse()
                .map_err(|_| format!("bad region x: {}", parts[0]))?;
            let rz: i32 = parts[1]
                .parse()
                .map_err(|_| format!("bad region z: {}", parts[1]))?;
            full_regions.push((rx, rz));
        } else if parts.len() >= 4 {
            let rx: i32 = parts[0]
                .parse()
                .map_err(|_| format!("bad region x: {}", parts[0]))?;
            let rz: i32 = parts[1]
                .parse()
                .map_err(|_| format!("bad region z: {}", parts[1]))?;
            let cx: i32 = parts[2]
                .parse()
                .map_err(|_| format!("bad chunk x: {}", parts[2]))?;
            let cz: i32 = parts[3]
                .parse()
                .map_err(|_| format!("bad chunk z: {}", parts[3]))?;
            let lx = cx.rem_euclid(32) as usize;
            let lz = cz.rem_euclid(32) as usize;
            chunks.push(ChunkRef {
                region_x: rx,
                region_z: rz,
                index: lz * 32 + lx,
            });
        } else {
            return Err(format!("bad selection line: {line}"));
        }
    }
    Ok((inverted, chunks, full_regions))
}

pub fn write_selection_csv(chunks: &[ChunkRef], inverted: bool) -> String {
    use std::collections::HashMap;
    let mut lines = Vec::new();
    if inverted {
        lines.push("inverted".to_string());
    }
    let mut by: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for c in chunks {
        by.entry((c.region_x, c.region_z))
            .or_default()
            .push(c.index);
    }
    let mut keys: Vec<_> = by.keys().copied().collect();
    keys.sort();
    for (rx, rz) in keys {
        let mut idxs = by[&(rx, rz)].clone();
        idxs.sort_unstable();
        idxs.dedup();
        if idxs.len() == 1024 {
            lines.push(format!("{rx};{rz}"));
        } else {
            for idx in idxs {
                let cx = rx * 32 + (idx % 32) as i32;
                let cz = rz * 32 + (idx / 32) as i32;
                lines.push(format!("{rx};{rz};{cx};{cz}"));
            }
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

/// Expand each selected chunk by Chebyshev radius (chunks).
pub fn expand_chunk_refs(chunks: &[ChunkRef], radius: i32) -> Vec<ChunkRef> {
    if radius <= 0 {
        return chunks.to_vec();
    }
    use std::collections::HashSet;
    let mut set = HashSet::new();
    for c in chunks {
        let cx = c.region_x * 32 + (c.index % 32) as i32;
        let cz = c.region_z * 32 + (c.index / 32) as i32;
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let nx = cx + dx;
                let nz = cz + dz;
                let rx = nx.div_euclid(32);
                let rz = nz.div_euclid(32);
                let lx = nx.rem_euclid(32) as usize;
                let lz = nz.rem_euclid(32) as usize;
                set.insert((rx, rz, lz * 32 + lx));
            }
        }
    }
    let mut out: Vec<_> = set
        .into_iter()
        .map(|(region_x, region_z, index)| ChunkRef {
            region_x,
            region_z,
            index,
        })
        .collect();
    out.sort_by_key(|c| (c.region_z, c.region_x, c.index));
    out
}

/// Convert chunk refs to region-grouped selections for delete/copy APIs.
pub fn chunk_refs_to_selections(chunks: &[ChunkRef]) -> Vec<(i32, i32, Vec<usize>)> {
    use std::collections::HashMap;
    let mut by: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for c in chunks {
        by.entry((c.region_x, c.region_z))
            .or_default()
            .push(c.index);
    }
    let mut out: Vec<_> = by
        .into_iter()
        .map(|((rx, rz), mut idxs)| {
            idxs.sort_unstable();
            idxs.dedup();
            (rx, rz, idxs)
        })
        .collect();
    out.sort_by_key(|(rx, rz, _)| (*rz, *rx));
    out
}

/// Resolve full-region CSV lines into present chunk indices by reading MCA headers.
pub fn resolve_full_regions(
    world_dir: &Path,
    regions: &[(i32, i32)],
    dimension: Option<&str>,
) -> Result<Vec<ChunkRef>, String> {
    let dim = normalize_dimension(dimension);
    let region_dir = dimension_data_dir(world_dir, dim, "region");
    let mut out = Vec::new();
    for &(rx, rz) in regions {
        let path = region_dir.join(format!("r.{rx}.{rz}.mca"));
        if !path.is_file() {
            continue;
        }
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        if data.len() < 8192 {
            continue;
        }
        for index in 0..1024 {
            let loc = index * 4;
            let offset = ((data[loc] as usize) << 16)
                | ((data[loc + 1] as usize) << 8)
                | (data[loc + 2] as usize);
            if offset != 0 {
                out.push(ChunkRef {
                    region_x: rx,
                    region_z: rz,
                    index,
                });
            }
        }
    }
    Ok(out)
}

/// Parse MCA Selector `--fields` string: `Status = full, InhabitedTime = 0`.
pub fn parse_change_fields(fields: &str) -> Result<NbtChangeRequest, String> {
    let mut req = NbtChangeRequest::default();
    for part in fields.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, val) = part
            .split_once('=')
            .ok_or_else(|| format!("bad field assignment: {part}"))?;
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\'');
        match key {
            "InhabitedTime" => {
                req.inhabited_time = Some(
                    val.parse()
                        .map_err(|_| format!("bad InhabitedTime: {val}"))?,
                );
            }
            "Status" => req.status = Some(val.to_string()),
            "DataVersion" => {
                req.data_version =
                    Some(val.parse().map_err(|_| format!("bad DataVersion: {val}"))?);
            }
            "LightPopulated" => {
                req.light_populated = Some(
                    val.parse()
                        .map_err(|_| format!("bad LightPopulated: {val}"))?,
                );
            }
            "LastUpdate" => {
                req.last_update = Some(val.parse().map_err(|_| format!("bad LastUpdate: {val}"))?);
            }
            "Biome" => req.biome = Some(val.to_string()),
            "DeleteSections" => req.delete_sections = Some(val.to_string()),
            "ReplaceBlocks" => req.replace_blocks = Some(val.to_string()),
            "DeleteStructureReference" | "DeleteStructureRefs" => {
                req.delete_structure_refs = Some(val.to_string());
            }
            "DeleteEntities" => {
                req.delete_entities = matches!(val, "1" | "true" | "True" | "TRUE");
            }
            "FixStatus" => {
                req.fix_status = matches!(val, "1" | "true" | "True" | "TRUE");
            }
            "PreventRetrogen" => {
                req.prevent_retrogen = matches!(val, "1" | "true" | "True" | "TRUE");
            }
            "ForceBlend" => {
                req.force_blend = matches!(val, "1" | "true" | "True" | "TRUE");
            }
            other => return Err(format!("unknown field: {other}")),
        }
    }
    Ok(req)
}

/// List every present chunk in a dimension (for CLI "apply to whole world").
pub fn list_present_chunks(
    world_dir: &Path,
    dimension: Option<&str>,
) -> Result<Vec<ChunkRef>, String> {
    let dim = normalize_dimension(dimension);
    let region_dir = dimension_data_dir(world_dir, dim, "region");
    if !region_dir.is_dir() {
        return Err("no region folder".into());
    }
    let mut regions = Vec::new();
    let entries = std::fs::read_dir(&region_dir).map_err(|e| e.to_string())?;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("mca") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let parts: Vec<&str> = stem.split('.').collect();
        if parts.len() < 3 || parts[0] != "r" {
            continue;
        }
        let Ok(rx) = parts[1].parse::<i32>() else {
            continue;
        };
        let Ok(rz) = parts[2].parse::<i32>() else {
            continue;
        };
        regions.push((rx, rz));
    }
    resolve_full_regions(world_dir, &regions, Some(dim))
}

/// Load selection CSV and resolve full-region lines against the world.
pub fn load_selection_csv_resolved(
    world_dir: &Path,
    csv_path: &Path,
    dimension: Option<&str>,
) -> Result<Vec<ChunkRef>, String> {
    let text = std::fs::read_to_string(csv_path).map_err(|e| e.to_string())?;
    let (inverted, mut chunks, full_regions) = parse_selection_csv(&text)?;
    let from_regions = resolve_full_regions(world_dir, &full_regions, dimension)?;
    chunks.extend(from_regions);
    if inverted {
        let all = list_present_chunks(world_dir, dimension)?;
        use std::collections::HashSet;
        let exclude: HashSet<_> = chunks
            .iter()
            .map(|c| (c.region_x, c.region_z, c.index))
            .collect();
        return Ok(all
            .into_iter()
            .filter(|c| !exclude.contains(&(c.region_x, c.region_z, c.index)))
            .collect());
    }
    // dedup
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    chunks.retain(|c| seen.insert((c.region_x, c.region_z, c.index)));
    Ok(chunks)
}

/// Scans chunks (or whole dimension if selections empty) for advanced content filters.
pub fn filter_world_chunks_advanced(
    world_dir: &Path,
    selections: &[(i32, i32, Vec<usize>)],
    filter: &AdvancedChunkFilter,
    dimension: Option<&str>,
) -> Result<Vec<ChunkRef>, String> {
    let dim = normalize_dimension(dimension);
    let region_dir = dimension_data_dir(world_dir, dim, "region");
    if !region_dir.is_dir() {
        return Err("no region folder".into());
    }

    let entity_needles = filter
        .entity_names
        .as_deref()
        .map(parse_csv_tokens)
        .unwrap_or_default();
    let structure_needles = filter
        .structure_names
        .as_deref()
        .map(parse_csv_tokens)
        .unwrap_or_default();
    let palette_needles = filter
        .palette_names
        .as_deref()
        .map(parse_csv_tokens)
        .unwrap_or_default();

    let has_any = !entity_needles.is_empty()
        || !structure_needles.is_empty()
        || !palette_needles.is_empty()
        || filter.min_entities.is_some()
        || filter.max_entities.is_some();
    if !has_any {
        return Err("no advanced filter criteria".into());
    }

    let targets: Vec<(i32, i32, usize)> = if selections.is_empty() {
        // Scan all region files.
        let mut all = Vec::new();
        let entries = std::fs::read_dir(&region_dir).map_err(|e| e.to_string())?;
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("mca") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let parts: Vec<&str> = stem.split('.').collect();
            if parts.len() < 3 || parts[0] != "r" {
                continue;
            }
            let Ok(rx) = parts[1].parse::<i32>() else {
                continue;
            };
            let Ok(rz) = parts[2].parse::<i32>() else {
                continue;
            };
            for index in 0..1024 {
                all.push((rx, rz, index));
            }
        }
        all
    } else {
        let mut all = Vec::new();
        for (rx, rz, indices) in selections {
            for &index in indices {
                all.push((*rx, *rz, index));
            }
        }
        all
    };

    let mut hits = Vec::new();
    for (rx, rz, index) in targets {
        let Ok(Some((blob, _))) = load_chunk_raw(&region_dir, rx, rz, index) else {
            continue;
        };
        let Some(nbt) = decompress_chunk_blob(&blob) else {
            continue;
        };
        let Some(root) = parse_root(&nbt) else {
            continue;
        };

        let entity_ids = collect_entity_ids(&root);
        let structure_keys = collect_structure_keys(&root);
        let palette_names = collect_palette_names(&root);
        let ent_count = entity_ids.len() as u32;

        if let Some(min) = filter.min_entities {
            if ent_count < min {
                continue;
            }
        }
        if let Some(max) = filter.max_entities {
            if ent_count > max {
                continue;
            }
        }
        if !entity_needles.is_empty() {
            let ok = entity_needles.iter().any(|n| {
                entity_ids
                    .iter()
                    .any(|id| id.contains(n) || id.ends_with(&format!(":{n}")))
            });
            if !ok {
                continue;
            }
        }
        if !structure_needles.is_empty() {
            let ok = structure_needles.iter().any(|n| {
                structure_keys
                    .iter()
                    .any(|k| k.contains(n) || k.ends_with(&format!(":{n}")))
            });
            if !ok {
                continue;
            }
        }
        if !palette_needles.is_empty() {
            let ok = palette_needles.iter().any(|n| {
                let want = normalize_block_name(n);
                palette_names.iter().any(|p| p == &want || p.contains(n))
            });
            if !ok {
                continue;
            }
        }

        hits.push(ChunkRef {
            region_x: rx,
            region_z: rz,
            index,
        });
    }
    Ok(hits)
}

fn collect_entity_ids(root: &NbtNode) -> Vec<String> {
    let mut out = Vec::new();
    for name in ["Entities", "entities"] {
        if let Some(list) = root
            .children
            .iter()
            .find(|c| c.name == name && c.tag_type == 9)
        {
            for ent in &list.children {
                if let Some(id) = ent
                    .children
                    .iter()
                    .find(|c| c.name == "id" || c.name == "Id")
                {
                    if let Some(Value::String(s)) = &id.value {
                        out.push(s.clone());
                    }
                }
            }
        }
    }
    out
}

fn collect_structure_keys(root: &NbtNode) -> Vec<String> {
    let mut out = Vec::new();
    for name in ["structures", "Structures"] {
        if let Some(structures) = root
            .children
            .iter()
            .find(|c| c.name == name && c.tag_type == 10)
        {
            for child in &structures.children {
                if child.tag_type == 10 {
                    for k in &child.children {
                        out.push(k.name.clone());
                    }
                }
            }
        }
    }
    out
}

fn collect_palette_names(root: &NbtNode) -> Vec<String> {
    let mut out = Vec::new();
    for sections_name in ["sections", "Sections"] {
        if let Some(list) = root
            .children
            .iter()
            .find(|c| c.name == sections_name && c.tag_type == 9)
        {
            for section in &list.children {
                if let Some(bs) = section.children.iter().find(|c| c.name == "block_states") {
                    if let Some(palette) = bs.children.iter().find(|c| c.name == "palette") {
                        for entry in &palette.children {
                            if entry.tag_type == 8 {
                                if let Some(Value::String(s)) = &entry.value {
                                    out.push(s.clone());
                                }
                            } else if let Some(name) =
                                entry.children.iter().find(|c| c.name == "Name")
                            {
                                if let Some(Value::String(s)) = &name.value {
                                    out.push(s.clone());
                                }
                            }
                        }
                    }
                }
                if let Some(palette) = section.children.iter().find(|c| c.name == "Palette") {
                    for entry in &palette.children {
                        if let Some(name) = entry.children.iter().find(|c| c.name == "Name") {
                            if let Some(Value::String(s)) = &name.value {
                                out.push(s.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

// ── Low-level root compound mutators (binary NBT) ─────────────────

fn skip_payload(nbt: &[u8], pos: &mut usize, tag_type: u8) {
    match tag_type {
        1 => *pos += 1,
        2 => *pos += 2,
        3 | 5 => *pos += 4,
        4 | 6 => *pos += 8,
        7 => {
            let len = read_i32(nbt, pos) as usize;
            *pos += len;
        }
        8 => {
            let len = read_u16(nbt, pos) as usize;
            *pos += len;
        }
        9 => {
            let lt = nbt.get(*pos).copied().unwrap_or(0);
            *pos += 1;
            let len = read_i32(nbt, pos) as usize;
            for _ in 0..len {
                if lt == 10 {
                    skip_compound(nbt, pos);
                } else {
                    skip_payload(nbt, pos, lt);
                }
            }
        }
        10 => skip_compound(nbt, pos),
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

fn skip_compound(nbt: &[u8], pos: &mut usize) {
    loop {
        let t = match nbt.get(*pos) {
            Some(v) => *v,
            None => return,
        };
        *pos += 1;
        if t == 0 {
            break;
        }
        let nlen = read_u16(nbt, pos) as usize;
        *pos += nlen;
        skip_payload(nbt, pos, t);
    }
}

fn root_entries_start(nbt: &[u8]) -> Option<usize> {
    if nbt.is_empty() || nbt[0] != 10 {
        return None;
    }
    let mut pos = 1usize;
    let nlen = read_u16(nbt, &mut pos) as usize;
    pos += nlen;
    Some(pos)
}

fn find_root_tag(nbt: &[u8], key: &str) -> Option<(usize, u8, usize, usize)> {
    // returns (tag_type_pos, tag_type, name_end_pos, payload_start)
    let mut pos = root_entries_start(nbt)?;
    loop {
        let type_pos = pos;
        let tag_type = *nbt.get(pos)?;
        pos += 1;
        if tag_type == 0 {
            return None;
        }
        let nlen = read_u16(nbt, &mut pos) as usize;
        let name = std::str::from_utf8(&nbt[pos..pos + nlen]).ok()?;
        pos += nlen;
        let payload_start = pos;
        if name == key {
            return Some((type_pos, tag_type, payload_start, 0));
        }
        skip_payload(nbt, &mut pos, tag_type);
    }
}

fn set_or_insert_int(nbt: &mut Vec<u8>, key: &str, value: i32, force: bool) -> bool {
    if let Some((_tp, tag_type, payload, _)) = find_root_tag(nbt, key) {
        if tag_type != 3 {
            return false;
        }
        if payload + 4 > nbt.len() {
            return false;
        }
        nbt[payload..payload + 4].copy_from_slice(&value.to_be_bytes());
        return true;
    }
    if !force {
        return false;
    }
    insert_root_tag(nbt, 3, key, &value.to_be_bytes())
}

fn set_or_insert_long(nbt: &mut Vec<u8>, key: &str, value: i64, force: bool) -> bool {
    if let Some((_tp, tag_type, payload, _)) = find_root_tag(nbt, key) {
        if tag_type != 4 {
            return false;
        }
        if payload + 8 > nbt.len() {
            return false;
        }
        nbt[payload..payload + 8].copy_from_slice(&value.to_be_bytes());
        return true;
    }
    if !force {
        return false;
    }
    insert_root_tag(nbt, 4, key, &value.to_be_bytes())
}

fn set_or_insert_byte(nbt: &mut Vec<u8>, key: &str, value: i8, force: bool) -> bool {
    if let Some((_tp, tag_type, payload, _)) = find_root_tag(nbt, key) {
        if tag_type != 1 {
            return false;
        }
        if payload >= nbt.len() {
            return false;
        }
        nbt[payload] = value as u8;
        return true;
    }
    if !force {
        return false;
    }
    insert_root_tag(nbt, 1, key, &[value as u8])
}

fn set_or_insert_string(nbt: &mut Vec<u8>, key: &str, value: &str, force: bool) -> bool {
    if let Some((type_pos, tag_type, payload, _)) = find_root_tag(nbt, key) {
        if tag_type != 8 {
            return false;
        }
        // Replace whole tag (variable length) by rebuilding around it.
        let mut p = payload;
        let old_len = read_u16(nbt, &mut p) as usize;
        let end = p + old_len;
        let mut new_tag = Vec::new();
        new_tag.push(8);
        write_string(&mut new_tag, key);
        write_string(&mut new_tag, value);
        nbt.splice(type_pos..end, new_tag);
        return true;
    }
    if !force {
        return false;
    }
    let mut payload = Vec::new();
    write_string(&mut payload, value);
    // insert_root_tag writes type+name+payload; for string payload already includes length.
    // Better custom insert:
    let mut tag = Vec::new();
    tag.push(8);
    write_string(&mut tag, key);
    write_string(&mut tag, value);
    insert_raw_root_tag(nbt, &tag)
}

fn insert_root_tag(nbt: &mut Vec<u8>, tag_type: u8, key: &str, payload: &[u8]) -> bool {
    let mut tag = Vec::new();
    tag.push(tag_type);
    write_string(&mut tag, key);
    tag.extend_from_slice(payload);
    insert_raw_root_tag(nbt, &tag)
}

fn insert_raw_root_tag(nbt: &mut Vec<u8>, tag: &[u8]) -> bool {
    // Insert before final TAG_End of root compound.
    let Some(start) = root_entries_start(nbt) else {
        return false;
    };
    let mut pos = start;
    loop {
        let t = match nbt.get(pos) {
            Some(v) => *v,
            None => return false,
        };
        if t == 0 {
            nbt.splice(pos..pos, tag.iter().copied());
            return true;
        }
        pos += 1;
        let nlen = {
            let mut p = pos;
            read_u16(nbt, &mut p) as usize
        };
        pos += 2 + nlen;
        skip_payload(nbt, &mut pos, t);
    }
}

fn clear_named_lists(nbt: &mut Vec<u8>, names: &[&str]) -> bool {
    let mut dirty = false;
    for name in names {
        if let Some((type_pos, tag_type, payload, _)) = find_root_tag(nbt, name) {
            if tag_type != 9 {
                continue;
            }
            // Replace list with empty list of same element type.
            let list_type = nbt.get(payload).copied().unwrap_or(10);
            let mut p = payload;
            skip_payload(nbt, &mut p, 9);
            let end = p;
            let mut new_tag = Vec::new();
            new_tag.push(9);
            write_string(&mut new_tag, name);
            new_tag.push(list_type);
            new_tag.extend_from_slice(&0i32.to_be_bytes());
            nbt.splice(type_pos..end, new_tag);
            dirty = true;
        }
    }
    dirty
}

fn fix_empty_status(nbt: &mut Vec<u8>) -> bool {
    // If Status is empty but sections/block data exists → set full.
    let status_empty = find_root_tag(nbt, "Status").and_then(|(_, t, payload, _)| {
        if t != 8 {
            return None;
        }
        let mut p = payload;
        let len = read_u16(nbt, &mut p) as usize;
        let s = String::from_utf8_lossy(&nbt[p..p + len]).to_string();
        Some(s == "empty" || s == "minecraft:empty" || s.is_empty())
    });
    if status_empty != Some(true) {
        return false;
    }
    let has_terrain = find_root_tag(nbt, "sections").is_some()
        || find_root_tag(nbt, "Sections").is_some()
        || find_root_tag(nbt, "Level").is_some();
    if !has_terrain {
        return false;
    }
    set_or_insert_string(nbt, "Status", "minecraft:full", true)
}

/// Extra overlay fields extracted while scanning chunk NBT.
pub fn extract_overlay_extras(nbt: &[u8]) -> ChunkMetaExtras {
    let mut biome_id: i32 = -1;
    let mut surface_y: i16 = -9999;
    let mut entity_count: u16 = 0;
    let mut structure_count: u16 = 0;

    let Some(mut pos) = root_entries_start(nbt) else {
        return ChunkMetaExtras {
            biome_id,
            surface_y,
            entity_count,
            structure_count,
        };
    };

    while pos < nbt.len() {
        let tag_type = nbt[pos];
        pos += 1;
        if tag_type == 0 {
            break;
        }
        let nlen = read_u16(nbt, &mut pos) as usize;
        if pos + nlen > nbt.len() {
            break;
        }
        let name = std::str::from_utf8(&nbt[pos..pos + nlen]).unwrap_or("");
        pos += nlen;

        match (name, tag_type) {
            ("Biomes", 7) => {
                let len = read_i32(nbt, &mut pos) as usize;
                if pos < nbt.len() && len > 0 {
                    biome_id = nbt[pos] as i32;
                }
                pos += len;
            }
            ("Biomes", 11) => {
                let len = read_i32(nbt, &mut pos) as usize;
                if len > 0 && pos + 4 <= nbt.len() {
                    biome_id =
                        i32::from_be_bytes([nbt[pos], nbt[pos + 1], nbt[pos + 2], nbt[pos + 3]]);
                }
                pos += len * 4;
            }
            ("Heightmaps", 10) | ("HeightMap", 10) => {
                if let Some(y) = sample_heightmap_in_compound(nbt, &mut pos) {
                    surface_y = y;
                } else {
                    skip_compound(nbt, &mut pos);
                }
            }
            ("Entities" | "entities" | "block_entities" | "TileEntities" | "blockEntities", 9) => {
                let _lt = nbt.get(pos).copied().unwrap_or(0);
                pos += 1;
                let len = read_i32(nbt, &mut pos) as i32;
                if name == "Entities" || name == "entities" {
                    entity_count = entity_count.saturating_add(len.max(0) as u16);
                }
                // skip list payload already partially consumed — use skip from list start
                // We already consumed type+len; skip elements:
                let lt = _lt;
                for _ in 0..len.max(0) as usize {
                    if lt == 10 {
                        skip_compound(nbt, &mut pos);
                    } else {
                        skip_payload(nbt, &mut pos, lt);
                    }
                }
            }
            ("structures" | "Structures", 10) => {
                // Count child tags roughly.
                let mut p = pos;
                let before = p;
                loop {
                    let t = match nbt.get(p) {
                        Some(v) => *v,
                        None => break,
                    };
                    p += 1;
                    if t == 0 {
                        break;
                    }
                    let nl = read_u16(nbt, &mut p) as usize;
                    p += nl;
                    structure_count = structure_count.saturating_add(1);
                    skip_payload(nbt, &mut p, t);
                }
                if p == before {
                    skip_compound(nbt, &mut pos);
                } else {
                    pos = p;
                }
            }
            _ => skip_payload(nbt, &mut pos, tag_type),
        }
    }

    if biome_id < 0 {
        if let Some(root) = parse_root(nbt) {
            if let Some(name) = sample_modern_biome_name(&root) {
                biome_id = hash_biome_name(&name);
            }
        }
    }

    ChunkMetaExtras {
        biome_id,
        surface_y,
        entity_count,
        structure_count,
    }
}

fn hash_biome_name(name: &str) -> i32 {
    let mut h: u32 = 2166136261;
    for b in name.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(16777619);
    }
    (h & 0x7FFF_FFFF) as i32
}

fn sample_modern_biome_name(root: &NbtNode) -> Option<String> {
    let sections = root
        .children
        .iter()
        .find(|c| c.name == "sections" || c.name == "Sections")?;
    let mut best: Option<String> = None;
    for sec in &sections.children {
        let Some(biomes) = sec.children.iter().find(|c| c.name == "biomes") else {
            continue;
        };
        let Some(palette) = biomes.children.iter().find(|c| c.name == "palette") else {
            continue;
        };
        for entry in &palette.children {
            let name = if entry.tag_type == 8 {
                entry
                    .value
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                entry.children.iter().find_map(|c| {
                    if c.name == "Name" {
                        c.value
                            .as_ref()
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
            };
            if let Some(n) = name {
                if best.is_none()
                    || (!n.contains("ocean") && !n.contains("river") && !n.contains("the_void"))
                {
                    best = Some(n);
                }
            }
        }
    }
    best
}

fn sample_heightmap_in_compound(nbt: &[u8], pos: &mut usize) -> Option<i16> {
    let mut found: Option<i16> = None;
    loop {
        let t = *nbt.get(*pos)?;
        *pos += 1;
        if t == 0 {
            break;
        }
        let nlen = read_u16(nbt, pos) as usize;
        if *pos + nlen > nbt.len() {
            return found;
        }
        let name = std::str::from_utf8(&nbt[*pos..*pos + nlen])
            .unwrap_or("")
            .to_string();
        *pos += nlen;
        if t == 12
            && found.is_none()
            && (name.contains("WORLD_SURFACE")
                || name.contains("MOTION_BLOCKING")
                || name == "MOTION_BLOCKING_NO_LEAVES")
        {
            let len = read_i32(nbt, pos) as usize;
            if len > 0 && *pos + 8 <= nbt.len() {
                let packed = i64::from_be_bytes(nbt[*pos..*pos + 8].try_into().ok()?);
                found = Some((packed & 0x1FF) as i16);
            }
            *pos += len * 8;
        } else {
            skip_payload(nbt, pos, t);
        }
    }
    found
}

/// MCA Selector-style map filter query (AND / OR of field comparisons).
///
/// Examples:
/// - `InhabitedTime < 100`
/// - `Status = full AND DataVersion >= 3105`
/// - `xPos >= -16 AND xPos < 16 AND EntityCount > 0`
#[derive(Debug, Clone)]
pub struct MapFilterQuery {
    clauses: Vec<(MapClause, bool)>, // (clause, and_with_prev) — first bool ignored
}

#[derive(Debug, Clone)]
struct MapClause {
    field: String,
    op: CmpOp,
    value: String,
}

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

/// Parse a filter query string into a [`MapFilterQuery`].
pub fn parse_map_filter_query(query: &str) -> Result<MapFilterQuery, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("empty filter query".into());
    }
    let mut clauses = Vec::new();
    let mut rest = q;
    let mut first = true;
    while !rest.is_empty() {
        let mut and_with_prev = true;
        if !first {
            let upper = rest.to_ascii_uppercase();
            if let Some(stripped) = upper.strip_prefix("AND ") {
                let skip = rest.len() - stripped.len();
                rest = rest[skip..].trim_start();
                and_with_prev = true;
            } else if let Some(stripped) = upper.strip_prefix("OR ") {
                let skip = rest.len() - stripped.len();
                rest = rest[skip..].trim_start();
                and_with_prev = false;
            } else {
                return Err(format!("expected AND/OR before `{rest}`"));
            }
        }
        first = false;
        let (clause, after) = parse_one_clause(rest)?;
        clauses.push((clause, and_with_prev));
        rest = after.trim_start();
    }
    Ok(MapFilterQuery { clauses })
}

fn parse_one_clause(s: &str) -> Result<(MapClause, &str), String> {
    let s = s.trim_start();
    let field_end = s
        .find(|c: char| c.is_whitespace() || "=!<>".contains(c))
        .ok_or_else(|| format!("missing operator in `{s}`"))?;
    let field = s[..field_end].trim().to_string();
    if field.is_empty() {
        return Err("missing field name".into());
    }
    let mut rest = s[field_end..].trim_start();
    let op = if rest.starts_with("!=") {
        rest = rest[2..].trim_start();
        CmpOp::Ne
    } else if rest.starts_with("<=") {
        rest = rest[2..].trim_start();
        CmpOp::Le
    } else if rest.starts_with(">=") {
        rest = rest[2..].trim_start();
        CmpOp::Ge
    } else if rest.starts_with('=') {
        rest = rest[1..].trim_start();
        CmpOp::Eq
    } else if rest.starts_with('<') {
        rest = rest[1..].trim_start();
        CmpOp::Lt
    } else if rest.starts_with('>') {
        rest = rest[1..].trim_start();
        CmpOp::Gt
    } else {
        return Err(format!("unknown operator near `{rest}`"));
    };

    let (value, after) = if rest.starts_with('"') || rest.starts_with('\'') {
        let quote = rest.chars().next().unwrap();
        let end = rest[1..]
            .find(quote)
            .ok_or_else(|| "unclosed string in filter query".to_string())?
            + 1;
        let value = rest[1..end].to_string();
        let after = rest[end + 1..].trim_start();
        (value, after)
    } else {
        let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        let mut value = rest[..end].to_string();
        // Stop at AND/OR if glued somehow
        let upper = value.to_ascii_uppercase();
        if upper == "AND" || upper == "OR" {
            return Err("missing value before AND/OR".into());
        }
        let after = rest[end..].trim_start();
        // Trim trailing punctuation
        while value.ends_with(',') {
            value.pop();
        }
        (value, after)
    };

    Ok((MapClause { field, op, value }, after))
}

fn status_code_from_name(s: &str) -> Option<i64> {
    match s.to_ascii_lowercase().as_str() {
        "unknown" | "0" => Some(0),
        "empty" | "1" => Some(1),
        "partial" | "spawn" | "2" => Some(2),
        "full" | "3" => Some(3),
        _ => s.parse().ok(),
    }
}

fn cmp_i64(left: i64, op: CmpOp, right: i64) -> bool {
    match op {
        CmpOp::Eq => left == right,
        CmpOp::Ne => left != right,
        CmpOp::Lt => left < right,
        CmpOp::Gt => left > right,
        CmpOp::Le => left <= right,
        CmpOp::Ge => left >= right,
    }
}

fn eval_clause(
    clause: &MapClause,
    rx: i32,
    rz: i32,
    index: usize,
    cell: &crate::region::ChunkCell,
) -> bool {
    let (cx, cz) = {
        let lx = (index % 32) as i32;
        let lz = (index / 32) as i32;
        (rx * 32 + lx, rz * 32 + lz)
    };
    let field = clause.field.to_ascii_lowercase();
    match field.as_str() {
        "inhabitedtime" | "inhabited" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(cell.inhabited_time as i64, clause.op, right)
        }
        "dataversion" | "version" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(cell.data_version as i64, clause.op, right)
        }
        "status" => {
            if let Some(code) = status_code_from_name(&clause.value) {
                cmp_i64(cell.status as i64, clause.op, code)
            } else {
                false
            }
        }
        "xpos" | "x" | "chunkx" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(cx as i64, clause.op, right)
        }
        "zpos" | "z" | "chunkz" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(cz as i64, clause.op, right)
        }
        "entitycount" | "entities" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(cell.entity_count as i64, clause.op, right)
        }
        "structurecount" | "structures" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(cell.structure_count as i64, clause.op, right)
        }
        "surfacey" | "height" | "y" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(cell.surface_y as i64, clause.op, right)
        }
        "biome" | "biomeid" => {
            let right: i64 = clause.value.parse().unwrap_or(-1);
            cmp_i64(cell.biome_id as i64, clause.op, right)
        }
        "lastmodified" | "timestamp" | "lastupdate" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(cell.last_modified as i64, clause.op, right)
        }
        "regionx" | "rx" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(rx as i64, clause.op, right)
        }
        "regionz" | "rz" => {
            let right: i64 = clause.value.parse().unwrap_or(0);
            cmp_i64(rz as i64, clause.op, right)
        }
        _ => false,
    }
}

impl MapFilterQuery {
    pub fn matches_cell(
        &self,
        rx: i32,
        rz: i32,
        index: usize,
        cell: &crate::region::ChunkCell,
    ) -> bool {
        if self.clauses.is_empty() {
            return true;
        }
        let mut result = eval_clause(&self.clauses[0].0, rx, rz, index, cell);
        for (clause, and_with_prev) in self.clauses.iter().skip(1) {
            let ok = eval_clause(clause, rx, rz, index, cell);
            if *and_with_prev {
                result = result && ok;
            } else {
                result = result || ok;
            }
        }
        result
    }
}

/// Select present chunks matching a map filter query (reads world map metadata).
pub fn select_world_by_query(
    world_dir: &Path,
    query: &str,
    dimension: Option<&str>,
) -> Result<Vec<ChunkRef>, String> {
    let parsed = parse_map_filter_query(query)?;
    let map = crate::region::read_world_map(world_dir, dimension)?;
    let mut hits = Vec::new();
    for region in &map.regions {
        for (index, cell) in region.chunks.iter().enumerate() {
            if cell.present == 0 {
                continue;
            }
            if parsed.matches_cell(region.region_x, region.region_z, index, cell) {
                hits.push(ChunkRef {
                    region_x: region.region_x,
                    region_z: region.region_z,
                    index,
                });
            }
        }
    }
    Ok(hits)
}
