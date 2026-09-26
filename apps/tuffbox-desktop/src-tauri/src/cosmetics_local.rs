//! Local TuffBox cosmetics profile (disk) + optional share to Supabase edge.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const API_BASE: &str = "https://vsoqnwknpueuubiovyjd.supabase.co";
const ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InZzb3Fud2tucHVldXViaW92eWpkIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODQ4MTEwMDYsImV4cCI6MjEwMDM4NzAwNn0.E9L11ipWyNiSchUx6pxT3HOVxu_vHtYDUOnNTixqJaI";

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CosmeticsProfile {
    pub player_key: String,
    pub username: String,
    pub skin_model: String,
    pub share_public: bool,
    pub wings: Option<String>,
    pub hat: Option<String>,
    pub trail: bool,
    pub jump_circles: bool,
    #[serde(default = "default_true")]
    pub hit_particles: bool,
    #[serde(default = "default_true")]
    pub hit_bubbles: bool,
    #[serde(default = "default_true")]
    pub target_esp: bool,
    #[serde(default = "default_true")]
    pub kill_effect: bool,
    pub cape_meta: serde_json::Value,
    pub write_secret: String,
    pub skin_path: Option<String>,
    pub cape_path: Option<String>,
}

impl Default for CosmeticsProfile {
    fn default() -> Self {
        Self {
            player_key: String::new(),
            username: String::new(),
            skin_model: "classic".into(),
            share_public: true,
            wings: None,
            hat: None,
            trail: false,
            jump_circles: false,
            hit_particles: true,
            hit_bubbles: true,
            target_esp: true,
            kill_effect: true,
            cape_meta: serde_json::json!({}),
            write_secret: String::new(),
            skin_path: None,
            cape_path: None,
        }
    }
}

fn root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("cosmetics")
}

fn profile_dir(player_key: &str) -> PathBuf {
    root().join(player_key)
}

fn ensure_secret(p: &mut CosmeticsProfile) {
    if p.write_secret.len() >= 16 {
        return;
    }
    let mut buf = [0u8; 32];
    // ponytail: not CSPRNG; fine for local write-secret until we wire OS RNG.
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for (i, b) in buf.iter_mut().enumerate() {
        *b = ((t >> ((i % 8) * 8)) as u8).wrapping_add(i as u8).wrapping_mul(31);
    }
    p.write_secret = hex::encode(buf);
}

pub fn active_extras(player_key: &str) -> tuffbox_core::CosmeticsLaunchExtras {
    let mut p = load_profile(player_key).unwrap_or_else(|_| CosmeticsProfile {
        player_key: player_key.to_string(),
        ..Default::default()
    });
    if p.player_key.trim().is_empty() {
        p.player_key = player_key.to_string();
    }
    ensure_secret(&mut p);
    // Persist secret so in-game upsert and next launch share the same key.
    if !player_key.is_empty() && player_key != "offline" {
        let dir = profile_dir(player_key);
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(
            dir.join("profile.json"),
            serde_json::to_string_pretty(&p).unwrap_or_default(),
        );
    }
    tuffbox_core::CosmeticsLaunchExtras {
        wings: p.wings.filter(|w| !w.is_empty()),
        hat: p.hat.filter(|h| !h.is_empty()),
        trail: p.trail,
        jump_circles: p.jump_circles,
        hit_particles: p.hit_particles,
        hit_bubbles: p.hit_bubbles,
        target_esp: p.target_esp,
        kill_effect: p.kill_effect,
        write_secret: p.write_secret,
    }
}

/// Merge in-game GUI prefs from `{game_dir}/.tuffbox/cosmetics-gui.json` over disk profile.
pub fn merge_gui_extras(
    game_dir: &std::path::Path,
    mut base: tuffbox_core::CosmeticsLaunchExtras,
) -> tuffbox_core::CosmeticsLaunchExtras {
    let path = game_dir.join(".tuffbox").join("cosmetics-gui.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return base;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return base;
    };
    let wings_on = v.get("wingsEnabled").and_then(|x| x.as_bool()).unwrap_or(true);
    let hat_on = v.get("hatEnabled").and_then(|x| x.as_bool()).unwrap_or(true);
    if let Some(w) = v.get("wingsId").and_then(|x| x.as_str()) {
        base.wings = if wings_on && !w.is_empty() {
            Some(w.to_string())
        } else {
            None
        };
    }
    if let Some(h) = v.get("hatId").and_then(|x| x.as_str()) {
        base.hat = if hat_on && !h.is_empty() {
            Some(h.to_string())
        } else {
            None
        };
    }
    if let Some(b) = v.get("trail").and_then(|x| x.as_bool()) {
        base.trail = b;
    }
    if let Some(b) = v.get("jumpCircles").and_then(|x| x.as_bool()) {
        base.jump_circles = b;
    }
    if let Some(b) = v.get("hitParticles").and_then(|x| x.as_bool()) {
        base.hit_particles = b;
    }
    if let Some(b) = v.get("hitBubbles").and_then(|x| x.as_bool()) {
        base.hit_bubbles = b;
    }
    if let Some(b) = v.get("targetEsp").and_then(|x| x.as_bool()) {
        base.target_esp = b;
    }
    if let Some(b) = v.get("killEffect").and_then(|x| x.as_bool()) {
        base.kill_effect = b;
    }
    base
}

pub fn load_profile(player_key: &str) -> Result<CosmeticsProfile, String> {
    let path = profile_dir(player_key).join("profile.json");
    if !path.is_file() {
        let mut p = CosmeticsProfile {
            player_key: player_key.to_string(),
            ..Default::default()
        };
        ensure_secret(&mut p);
        return Ok(p);
    }
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut p: CosmeticsProfile = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    ensure_secret(&mut p);
    Ok(p)
}

pub fn save_profile(mut profile: CosmeticsProfile) -> Result<CosmeticsProfile, String> {
    if profile.player_key.trim().is_empty() {
        return Err("playerKey required".into());
    }
    ensure_secret(&mut profile);
    let dir = profile_dir(&profile.player_key);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("profile.json");
    fs::write(&path, serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    if profile.share_public {
        let _ = sync_remote(&profile, None, None);
    }
    Ok(profile)
}

pub fn upload_skin_file(
    player_key: &str,
    username: &str,
    src_path: &str,
    model: &str,
) -> Result<CosmeticsProfile, String> {
    let bytes = fs::read(src_path).map_err(|e| e.to_string())?;
    if bytes.len() < 100 || bytes.len() > 8_388_608 {
        return Err("skin PNG size out of range".into());
    }
    let dir = profile_dir(player_key);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join("skin.png");
    fs::write(&dest, &bytes).map_err(|e| e.to_string())?;

    let mut p = load_profile(player_key)?;
    p.player_key = player_key.to_string();
    p.username = username.to_string();
    p.skin_model = if model.eq_ignore_ascii_case("slim") {
        "slim".into()
    } else {
        "classic".into()
    };
    p.skin_path = Some(dest.to_string_lossy().into_owned());
    ensure_secret(&mut p);
    fs::write(
        dir.join("profile.json"),
        serde_json::to_string_pretty(&p).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    if p.share_public {
        sync_remote(&p, Some(&bytes), None)?;
    }
    Ok(p)
}

pub fn upload_cape_file(
    player_key: &str,
    username: &str,
    src_path: &str,
    animated: bool,
    frame_ms: u32,
    frames: u32,
) -> Result<CosmeticsProfile, String> {
    let bytes = fs::read(src_path).map_err(|e| e.to_string())?;
    if bytes.len() < 100 || bytes.len() > 8_388_608 {
        return Err("cape PNG size out of range".into());
    }
    let dir = profile_dir(player_key);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join("cape.png");
    fs::write(&dest, &bytes).map_err(|e| e.to_string())?;

    let mut p = load_profile(player_key)?;
    p.player_key = player_key.to_string();
    p.username = username.to_string();
    p.cape_path = Some(dest.to_string_lossy().into_owned());
    p.cape_meta = serde_json::json!({
        "animated": animated,
        "frameMs": frame_ms,
        "frames": frames,
    });
    ensure_secret(&mut p);
    fs::write(
        dir.join("profile.json"),
        serde_json::to_string_pretty(&p).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    if p.share_public {
        sync_remote(&p, None, Some(&bytes))?;
    }
    Ok(p)
}

pub fn set_wings(
    player_key: &str,
    username: &str,
    wings: Option<String>,
) -> Result<CosmeticsProfile, String> {
    let mut p = load_profile(player_key)?;
    p.player_key = player_key.to_string();
    if !username.is_empty() {
        p.username = username.to_string();
    }
    p.wings = wings.filter(|w| !w.is_empty());
    save_profile(p)
}

pub fn set_visual_extras(
    player_key: &str,
    username: &str,
    hat: Option<String>,
    trail: bool,
    jump_circles: bool,
    hit_particles: bool,
    hit_bubbles: bool,
    target_esp: bool,
    kill_effect: bool,
) -> Result<CosmeticsProfile, String> {
    let mut p = load_profile(player_key)?;
    p.player_key = player_key.to_string();
    if !username.is_empty() {
        p.username = username.to_string();
    }
    p.hat = hat.filter(|h| !h.is_empty());
    p.trail = trail;
    p.jump_circles = jump_circles;
    p.hit_particles = hit_particles;
    p.hit_bubbles = hit_bubbles;
    p.target_esp = target_esp;
    p.kill_effect = kill_effect;
    save_profile(p)
}

fn sync_remote(
    profile: &CosmeticsProfile,
    skin_bytes: Option<&[u8]>,
    cape_bytes: Option<&[u8]>,
) -> Result<(), String> {
    use base64::Engine;
    let mut body = serde_json::json!({
        "playerKey": profile.player_key,
        "username": profile.username,
        "writeSecret": profile.write_secret,
        "skinModel": profile.skin_model,
        "sharePublic": profile.share_public,
        "capeMeta": profile.cape_meta,
        "cosmetics": {
            "wings": profile.wings,
            "hat": profile.hat,
            "trail": profile.trail,
            "jumpCircles": profile.jump_circles,
            "hitParticles": profile.hit_particles,
            "hitBubbles": profile.hit_bubbles,
            "targetEsp": profile.target_esp,
            "killEffect": profile.kill_effect
        },
    });
    if let Some(b) = skin_bytes {
        body["skinPngBase64"] = serde_json::Value::String(
            base64::engine::general_purpose::STANDARD.encode(b),
        );
    }
    if let Some(b) = cape_bytes {
        body["capePngBase64"] = serde_json::Value::String(
            base64::engine::general_purpose::STANDARD.encode(b),
        );
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{API_BASE}/functions/v1/cosmetics-upsert");
    let resp = client
        .post(url)
        .header("apikey", ANON_KEY)
        .header("Authorization", format!("Bearer {ANON_KEY}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(format!("cosmetics-upsert {status}: {text}"));
    }
    Ok(())
}

pub fn wings_catalog() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"id": "", "label": "None"}),
        serde_json::json!({"id": "angel", "label": "Angel"}),
        serde_json::json!({"id": "demon", "label": "Demon"}),
        serde_json::json!({"id": "fairy", "label": "Fairy"}),
    ]
}

pub fn hat_catalog() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"id": "", "label": "None"}),
        serde_json::json!({"id": "china", "label": "China hat"}),
        serde_json::json!({"id": "halo", "label": "Halo"}),
        serde_json::json!({"id": "horns", "label": "Horns"}),
        serde_json::json!({"id": "crown", "label": "Crown"}),
    ]
}
