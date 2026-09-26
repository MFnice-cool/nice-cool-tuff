//! Daily Modpack Index → Supabase MPI co-occurrence seed (hub-side only).
//!
//! Writes into `mpi_mod_cooccurrence_pairs` (separate from TuffSwarm launcher pairs).
//! Crawl strategy: many packs per pack-theme category/tag, then optional version×loader
//! targets. Only mod co-occurrence is stored (no pack files / CDN).

use serde_json::Value;
use std::time::Duration;
use tracing::{info, warn};

/// Light version×loader targets (supplement category crawl).
const VERSION_TARGETS: &[(&str, &str)] = &[
    ("1.21.1", "neoforge"),
    ("1.21.1", "forge"),
    ("1.21.1", "fabric"),
    ("1.20.1", "neoforge"),
    ("1.20.1", "forge"),
    ("1.20.1", "fabric"),
];

#[derive(Debug, Clone)]
pub struct SupabaseCreds {
    pub url: String,
    pub service_role_key: String,
}

pub fn creds_from_env() -> Option<SupabaseCreds> {
    let url = std::env::var("SUPABASE_URL").ok()?.trim().to_string();
    let key = std::env::var("SUPABASE_SERVICE_ROLE_KEY")
        .ok()?
        .trim()
        .to_string();
    if url.is_empty() || key.is_empty() {
        return None;
    }
    Some(SupabaseCreds {
        url,
        service_role_key: key,
    })
}

fn pack_id(hit: &Value) -> Option<u64> {
    hit.get("id").and_then(|v| v.as_u64()).or_else(|| {
        hit.get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
    })
}

fn pack_downloads(hit: &Value) -> u64 {
    hit.get("downloads")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .max(1)
}

fn weight_from_downloads(dl: u64) -> u64 {
    // 1..=6 — soft boost for popular packs without exploding counts.
    let w = ((dl as f64).log10().floor() as u64).saturating_sub(2);
    w.clamp(1, 6)
}

async fn seed_pack(
    creds: &SupabaseCreds,
    hit: &Value,
    mc_version: &str,
    loader: &str,
    category_slug: &str,
) {
    let Some(id) = pack_id(hit) else {
        return;
    };
    let already = tuffbox_core::swarm_supabase::mpi_pack_already_synced_scoped(
        &creds.url,
        &creds.service_role_key,
        id,
        mc_version,
        loader,
        category_slug,
    )
    .await
    .unwrap_or(false);
    if already {
        return;
    }

    let slugs = tokio::task::spawn_blocking(move || {
        tuffbox_core::modpack_index::modpack_mod_slugs(id, 100, true)
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .unwrap_or_default();

    if slugs.len() < 2 {
        let _ = tuffbox_core::swarm_supabase::mark_mpi_pack_synced_scoped(
            &creds.url,
            &creds.service_role_key,
            id,
            mc_version,
            loader,
            category_slug,
            slugs.len() as i32,
        )
        .await;
        return;
    }

    let weight = weight_from_downloads(pack_downloads(hit));
    let rows = tuffbox_core::modpack_index::expand_pair_rows_with_category(
        &slugs,
        mc_version,
        loader,
        weight,
        "mpi",
        category_slug,
    );
    let rows: Vec<_> = rows.into_iter().take(800).collect();
    match tuffbox_core::swarm_supabase::seed_mpi_cooccurrence_pairs_supabase(
        &creds.url,
        &creds.service_role_key,
        &rows,
    )
    .await
    {
        Ok(n) => info!(
            pack_id = id,
            category = category_slug,
            pairs = n,
            "seeded MPI pack pairs"
        ),
        Err(e) => warn!(pack_id = id, error = %e, "MPI seed failed"),
    }

    let _ = tuffbox_core::swarm_supabase::mark_mpi_pack_synced_scoped(
        &creds.url,
        &creds.service_role_key,
        id,
        mc_version,
        loader,
        category_slug,
        slugs.len() as i32,
    )
    .await;

    // Soft rate-limit toward Modpack Index (~3600/hr).
    tokio::time::sleep(Duration::from_millis(350)).await;
}

/// Crawl many packs per pack-theme category → mod co-occurrence → Supabase MPI table.
async fn sync_by_categories(creds: &SupabaseCreds, packs_per_category: u32) -> Result<(), String> {
    let packs_per_category = packs_per_category.clamp(5, 40);
    let themes = tuffbox_core::modpack_index::PACK_THEMES;
    info!(
        themes = themes.len(),
        packs_per_category, "MPI category crawl starting"
    );

    for theme in themes {
        info!(category = theme.slug, id = theme.id, "MPI category target");
        let category_id = theme.id;
        let packs = tokio::task::spawn_blocking(move || {
            tuffbox_core::modpack_index::search_modpacks_by_category_analytics(
                category_id,
                1,
                packs_per_category,
            )
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e)?;

        let (hits, _total) = packs;
        for hit in hits {
            // Category graph: empty mc/loader — co-occurrence across CF+MR packs in this theme.
            seed_pack(creds, &hit, "", "", theme.slug).await;
        }
    }
    Ok(())
}

/// Optional version×loader supplement (empty category_slug).
async fn sync_by_versions(creds: &SupabaseCreds, packs_per_target: u32) -> Result<(), String> {
    let packs_per_target = packs_per_target.clamp(5, 40);
    for &(version, loader) in VERSION_TARGETS {
        info!(version, loader, "MPI version target");
        let version = version.to_string();
        let loader = loader.to_string();
        let packs = tokio::task::spawn_blocking({
            let version = version.clone();
            move || {
                tuffbox_core::modpack_index::search_modpacks_for_version_analytics(
                    &version,
                    1,
                    packs_per_target,
                )
            }
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e)?;

        let (hits, _total) = packs;
        for hit in hits {
            seed_pack(creds, &hit, &version, &loader, "").await;
        }
    }
    Ok(())
}

/// One-shot crawl: pack themes (primary) + version targets → seed `mpi_mod_cooccurrence_pairs`.
pub async fn run_mpi_sync_once(creds: &SupabaseCreds, packs_per_target: u32) -> Result<(), String> {
    info!(
        packs_per_target,
        "MPI analytics sync starting (TuffSwarm-Analytics/1.0) → mpi_mod_cooccurrence_pairs"
    );
    sync_by_categories(creds, packs_per_target).await?;
    sync_by_versions(creds, packs_per_target.min(15)).await?;
    match tuffbox_core::swarm_supabase::refresh_mod_partner_tops_supabase(
        &creds.url,
        &creds.service_role_key,
    )
    .await
    {
        Ok(rows) => info!(rows, "mod_partner_tops refresh finished"),
        Err(e) => warn!(error = %e, "mod_partner_tops refresh failed"),
    }
    info!("MPI analytics sync finished");
    Ok(())
}

pub fn spawn_daily_loop(creds: SupabaseCreds) {
    tokio::spawn(async move {
        // Run once shortly after boot, then every 24h.
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            if let Err(e) = run_mpi_sync_once(&creds, 25).await {
                warn!(error = %e, "MPI daily sync error");
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}
