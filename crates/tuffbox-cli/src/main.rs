mod dataset_ftb_quests;

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use tuffbox_core::{
    ContentProvider, DependencyGraph, ModSource, ModSpec, ProjectManifest, ProviderFileInfo,
    ProviderSearchQuery, Resolver, Side, Snapshot, SnapshotStore, SourceKind, TuffboxLockfile,
};

#[derive(Debug, Parser)]
#[command(name = "tuffbox")]
#[command(about = "TuffBox IDE developer harness", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Project manifest commands.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Dependency graph commands.
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },
    /// Resolver commands.
    Resolve { manifest: PathBuf },
    /// Snapshot commands.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// Modrinth provider commands.
    Modrinth {
        #[command(subcommand)]
        command: ModrinthCommand,
    },
    /// CurseForge provider / dataset commands.
    Curseforge {
        #[command(subcommand)]
        command: CurseforgeCommand,
    },
    /// Launch a test instance.
    Launch {
        manifest: PathBuf,
        #[arg(short, long)]
        profile: Option<String>,
    },
    /// MCA Selector-style world tools.
    World {
        #[command(subcommand)]
        command: WorldCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Validate a TuffBox project manifest.
    Validate { manifest: PathBuf },
    /// Create and print lockfile JSON.
    Lock { manifest: PathBuf },
    /// Add a mod from Modrinth to the project manifest.
    AddMod {
        manifest: PathBuf,
        mod_id: String,
        #[arg(short, long)]
        side: Option<String>,
    },
    /// Remove a mod from the project manifest.
    RemoveMod { manifest: PathBuf, mod_id: String },
    /// Update a mod in the project manifest to the latest compatible version.
    UpdateMod { manifest: PathBuf, mod_id: String },
}

#[derive(Debug, Subcommand)]
enum GraphCommand {
    /// Print graph as JSON.
    Print { manifest: PathBuf },
    /// Print diagnostics as JSON.
    Diagnostics { manifest: PathBuf },
}

#[derive(Debug, Subcommand)]
enum SnapshotCommand {
    /// Create a snapshot of the current project state.
    Create {
        project_dir: PathBuf,
        #[arg(short, long)]
        name: String,
        #[arg(short, long, default_value = "manual")]
        reason: String,
        #[arg(short, long)]
        manifest: Option<PathBuf>,
    },
    /// List existing snapshots.
    List { project_dir: PathBuf },
    /// Show diff between two snapshots.
    Diff {
        project_dir: PathBuf,
        from: String,
        to: String,
    },
    /// Rollback project to a snapshot.
    Rollback { project_dir: PathBuf, id: String },
}

#[derive(Debug, Subcommand)]
enum WorldCommand {
    /// Print a compact JSON map overview for a world dimension.
    Map {
        /// Path to the world save folder (contains region/).
        world: PathBuf,
        #[arg(long, default_value = "overworld")]
        dim: String,
    },
    /// Delete chunks from a selection CSV (MCA Selector format).
    Delete {
        world: PathBuf,
        #[arg(long)]
        selection: PathBuf,
        #[arg(long, default_value = "overworld")]
        dim: String,
    },
    /// Export selected chunks to an empty folder.
    Export {
        world: PathBuf,
        #[arg(long)]
        selection: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "overworld")]
        dim: String,
    },
    /// Change NBT fields (`Status = full, InhabitedTime = 0`).
    Change {
        world: PathBuf,
        #[arg(long)]
        fields: String,
        #[arg(long)]
        selection: Option<PathBuf>,
        #[arg(long, default_value = "overworld")]
        dim: String,
        #[arg(long)]
        force: bool,
    },
    /// Content filter → write selection CSV.
    Filter {
        world: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        palette: Option<String>,
        #[arg(long)]
        entities: Option<String>,
        #[arg(long)]
        structures: Option<String>,
        #[arg(long)]
        min_entities: Option<u32>,
        #[arg(long)]
        selection: Option<PathBuf>,
        #[arg(long, default_value = "overworld")]
        dim: String,
        #[arg(long, default_value_t = 0)]
        radius: i32,
    },
    /// Select chunks by map filter query → CSV (`InhabitedTime < 100 AND Status = full`).
    Select {
        world: PathBuf,
        #[arg(long)]
        query: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "overworld")]
        dim: String,
        #[arg(long, default_value_t = 0)]
        radius: i32,
    },
    /// Import chunks from another world / export folder (MCA Selector Chunk Import).
    Import {
        /// Target world folder.
        world: PathBuf,
        /// Source world or export folder (contains region/).
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        selection: Option<PathBuf>,
        /// Only import into this target selection (CSV).
        #[arg(long)]
        into_selection: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        offset_x: i32,
        #[arg(long, default_value_t = 0)]
        offset_z: i32,
        /// Vertical section offset (×16 blocks).
        #[arg(long, default_value_t = 0)]
        y_offset: i32,
        /// Keep only these sections (`all`, `:-4`, `0:4`).
        #[arg(long)]
        sections: Option<String>,
        #[arg(long, default_value = "overworld")]
        dim: String,
        #[arg(long)]
        source_dim: Option<String>,
        /// Skip destinations that already have a chunk.
        #[arg(long)]
        no_overwrite: bool,
    },
    /// Render world/selection as a PNG image.
    Image {
        world: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        selection: Option<PathBuf>,
        #[arg(long, default_value = "status")]
        mode: String,
        #[arg(long, default_value_t = 4)]
        scale: u32,
        #[arg(long, default_value = "overworld")]
        dim: String,
    },
    /// Warm region metadata cache (speeds up subsequent map reads).
    Cache {
        world: PathBuf,
        #[arg(long, default_value = "overworld")]
        dim: String,
    },
    /// Clear region metadata cache for a world.
    CacheClear {
        world: PathBuf,
        #[arg(long)]
        dim: Option<String>,
    },
    /// Compact region/entities/poi files.
    Purge {
        world: PathBuf,
        #[arg(long, default_value = "overworld")]
        dim: String,
    },
}

#[derive(Debug, Subcommand)]
enum ModrinthCommand {
    /// Search for projects on Modrinth.
    Search {
        query: String,
        #[arg(long)]
        mc: Option<String>,
        #[arg(long)]
        loader: Option<String>,
    },
    /// Show available versions for a project.
    Versions {
        project_id: String,
        #[arg(long)]
        mc: Option<String>,
        #[arg(long)]
        loader: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum CurseforgeCommand {
    /// Collect FTB Quests `.snbt` from popular CurseForge modpacks into a local dataset.
    ///
    /// Downloads each modpack `.zip` into RAM only (never written to disk), extracts
    /// files under `config/ftbquests/` / `overrides/config/ftbquests/`, and saves
    /// them as `[slug]_[name].snbt` under the output directory.
    ///
    /// Default search: modpacks in the CurseForge **Quests** category (`4478`),
    /// sorted by downloads. Packs without FTB Quests SNBT are skipped.
    CollectQuests {
        /// Max number of most-downloaded modpacks to process.
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Directory for extracted `.snbt` files.
        #[arg(long, default_value = "./dataset/raw")]
        out: PathBuf,
        /// Max concurrent zip downloads (lower = less RAM).
        #[arg(long, default_value_t = 3)]
        concurrency: usize,
        /// Optional CurseForge search filter (name/author).
        #[arg(long)]
        query: Option<String>,
        /// Category: `quests` (default, id 4478), numeric category id, or `none`.
        #[arg(long, default_value = "quests")]
        category: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Project { command } => match command {
            ProjectCommand::Validate { manifest } => {
                let manifest = load_manifest(manifest)?;
                manifest.validate_basic()?;
                println!("OK: {} {}", manifest.project.name, manifest.project.version);
            }
            ProjectCommand::Lock { manifest } => {
                let manifest = load_manifest(manifest)?;
                manifest.validate_basic()?;
                let graph = DependencyGraph::from_manifest(&manifest);
                let lockfile = TuffboxLockfile::from_manifest_and_graph(&manifest, &graph);
                println!("{}", serde_json::to_string_pretty(&lockfile)?);
            }
            ProjectCommand::AddMod {
                manifest,
                mod_id,
                side,
            } => {
                let manifest_path = manifest.clone();
                let snapshot = auto_snapshot(&manifest_path, "add-mod")?;
                let mut manifest = load_manifest(manifest_path.clone())?;
                add_mod_from_modrinth(&mut manifest, &mod_id, side)?;
                save_manifest(&manifest_path, &manifest)?;
                let report = download_project_mods(&manifest_path, &manifest);
                if !report.failed.is_empty() {
                    eprintln!("warning: {} mod(s) failed to download", report.failed.len());
                    for f in &report.failed {
                        eprintln!("  - {}: {}", f.mod_id, f.error);
                    }
                }
                println!(
                    "Added mod {} to {} (snapshot {})",
                    mod_id,
                    manifest_path.display(),
                    snapshot.id
                );
            }
            ProjectCommand::RemoveMod { manifest, mod_id } => {
                let manifest_path = manifest.clone();
                let snapshot = auto_snapshot(&manifest_path, "remove-mod")?;
                let mut manifest = load_manifest(manifest_path.clone())?;
                let removed = manifest.mods.iter().find(|m| m.id == mod_id).cloned();
                remove_mod(&mut manifest, &mod_id)?;
                save_manifest(&manifest_path, &manifest)?;
                if let Some(removed) = removed {
                    if let Some(file_name) = removed.file_name {
                        if let Some(instance_dir) =
                            tuffbox_core::instance_dir_for_manifest(&manifest_path)
                        {
                            let content_dir =
                                tuffbox_core::content_dir_for(&instance_dir, removed.content_type);
                            if let Err(e) = std::fs::remove_file(content_dir.join(&file_name)) {
                                eprintln!("warning: could not delete file {file_name}: {e}");
                            }
                        }
                    }
                }
                println!(
                    "Removed mod {} from {} (snapshot {})",
                    mod_id,
                    manifest_path.display(),
                    snapshot.id
                );
            }
            ProjectCommand::UpdateMod { manifest, mod_id } => {
                let manifest_path = manifest.clone();
                let snapshot = auto_snapshot(&manifest_path, "update-mod")?;
                let mut manifest = load_manifest(manifest_path.clone())?;
                update_mod_from_modrinth(&mut manifest, &mod_id)?;
                save_manifest(&manifest_path, &manifest)?;
                let report = download_project_mods(&manifest_path, &manifest);
                if !report.failed.is_empty() {
                    eprintln!("warning: {} mod(s) failed to download", report.failed.len());
                    for f in &report.failed {
                        eprintln!("  - {}: {}", f.mod_id, f.error);
                    }
                }
                println!(
                    "Updated mod {} in {} (snapshot {})",
                    mod_id,
                    manifest_path.display(),
                    snapshot.id
                );
            }
        },
        Command::Graph { command } => match command {
            GraphCommand::Print { manifest } => {
                let manifest = load_manifest(manifest)?;
                let graph = DependencyGraph::from_manifest(&manifest);
                println!("{}", serde_json::to_string_pretty(&graph)?);
            }
            GraphCommand::Diagnostics { manifest } => {
                let manifest = load_manifest(manifest)?;
                let graph = DependencyGraph::from_manifest(&manifest);
                let diagnostics = Resolver::analyze_project(&manifest, &graph);
                println!("{}", serde_json::to_string_pretty(&diagnostics)?);
            }
        },
        Command::Resolve { manifest } => {
            let manifest = load_manifest(manifest)?;
            let graph = DependencyGraph::from_manifest(&manifest);
            let diagnostics = Resolver::analyze_project(&manifest, &graph);
            let plan = Resolver::create_fix_plan(&graph, &diagnostics);
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        Command::Snapshot { command } => match command {
            SnapshotCommand::Create {
                project_dir,
                name,
                reason,
                manifest,
            } => {
                let manifest_path =
                    manifest.unwrap_or_else(|| project_dir.join("project.tuffbox.json"));
                let lockfile_path = manifest_path.with_extension("lock.json");
                let lockfile_path = if lockfile_path.exists() {
                    Some(lockfile_path)
                } else {
                    None
                };
                let store = SnapshotStore::new(&project_dir);
                let snapshot = store.create(
                    &name,
                    &reason,
                    &manifest_path,
                    lockfile_path.as_ref(),
                    &[] as &[&PathBuf],
                )?;
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
            }
            SnapshotCommand::List { project_dir } => {
                let store = SnapshotStore::new(&project_dir);
                let snapshots = store.list()?;
                println!("{}", serde_json::to_string_pretty(&snapshots)?);
            }
            SnapshotCommand::Diff {
                project_dir,
                from,
                to,
            } => {
                let store = SnapshotStore::new(&project_dir);
                let diff = store.diff(&from, &to)?;
                println!("{}", serde_json::to_string_pretty(&diff)?);
            }
            SnapshotCommand::Rollback { project_dir, id } => {
                let store = SnapshotStore::new(&project_dir);
                let snapshot = store.rollback(&id)?;
                println!("Rolled back to snapshot {}", snapshot.id);
            }
        },
        Command::Modrinth { command } => match command {
            ModrinthCommand::Search { query, mc, loader } => {
                let provider = tuffbox_core::ModrinthProvider::new();
                let results = provider.search(&ProviderSearchQuery {
                    query: Some(query),
                    minecraft_version: mc,
                    loader,
                    ..Default::default()
                })?;
                println!("{}", serde_json::to_string_pretty(&results.results)?);
            }
            ModrinthCommand::Versions {
                project_id,
                mc,
                loader,
            } => {
                let provider = tuffbox_core::ModrinthProvider::new();
                let versions = provider.get_versions(
                    &project_id,
                    &ProviderSearchQuery {
                        query: None,
                        minecraft_version: mc,
                        loader,
                        ..Default::default()
                    },
                )?;
                println!("{}", serde_json::to_string_pretty(&versions)?);
            }
        },
        Command::Curseforge { command } => match command {
            CurseforgeCommand::CollectQuests {
                limit,
                out,
                concurrency,
                query,
                category,
            } => {
                let category_id = dataset_ftb_quests::parse_category_arg(&category)?;
                let stats = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .context("start tokio runtime")?
                    .block_on(dataset_ftb_quests::collect_ftb_quests(
                        dataset_ftb_quests::CollectOptions {
                            limit,
                            out_dir: out.clone(),
                            concurrency,
                            query,
                            category_id,
                        },
                    ))?;
                println!(
                    "Done. modpacks={} ok={} skipped={} failed={} snbt_written={} out={}",
                    stats.modpacks_found,
                    stats.modpacks_ok,
                    stats.modpacks_skipped,
                    stats.modpacks_failed,
                    stats.snbt_written,
                    out.display()
                );
            }
        },
        Command::Launch { manifest, profile } => {
            let manifest_path = manifest.clone();
            let manifest = load_manifest(manifest)?;
            let profile_id = profile.unwrap_or_else(|| "client".to_string());
            let profile = manifest
                .profiles
                .iter()
                .find(|p| p.id == profile_id)
                .with_context(|| {
                    let available: Vec<_> =
                        manifest.profiles.iter().map(|p| p.id.as_str()).collect();
                    format!("profile '{profile_id}' not found; available: {available:?}")
                })?;

            let java = if let Some(java_path) = manifest.java.as_ref().and_then(|j| j.path.clone())
            {
                tuffbox_core::jre::check_java_at_path(&PathBuf::from(&java_path))?
            } else {
                tuffbox_core::TestLauncher::find_java_for_minecraft(&manifest.minecraft.version)?
            };
            let game_dir = manifest_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| {
                    eprintln!("warning: manifest path has no parent directory, falling back to current dir");
                    PathBuf::from(".")
                });
            let launcher_dir = dirs::data_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| {
                    eprintln!("warning: could not determine data/home directory, falling back to current dir");
                    PathBuf::from(".")
                })
                .join("TuffBox");
            std::fs::create_dir_all(&launcher_dir)?;

            let progress = tuffbox_core::mc_install::InstallProgress {
                log_path: game_dir.join("logs").join("latest.log"),
            };

            // Same safety net as the desktop launcher: verify every
            // manifest-declared mod actually has its jar on disk before we
            // launch, so `tuffbox launch` never silently starts vanilla.
            let sync_report = tuffbox_core::ensure_project_mods_downloaded(&manifest, &game_dir);
            if !sync_report.downloaded.is_empty() {
                println!(
                    "Downloaded {} missing mod file(s).",
                    sync_report.downloaded.len()
                );
            }
            for failure in &sync_report.failed {
                eprintln!(
                    "warning: failed to prepare mod '{}': {}",
                    failure.mod_id, failure.error
                );
            }

            let options = tuffbox_core::LaunchOptions {
                profile_id: profile_id.clone(),
                instance_dir: game_dir,
                memory_mb: profile.memory_mb.unwrap_or(4096),
                jvm_args: profile.jvm_args.clone(),
                quick_play_type: None,
                quick_play_value: None,
            };
            let (mut cmd, log_path) = tuffbox_core::TestLauncher::build_command(
                &manifest,
                profile,
                &options,
                &java,
                &launcher_dir,
                &progress,
                None,
                None,
                None,
                None,
            )?;
            let mut child = cmd.spawn()?;
            let pid = child.id();
            let started_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let status = child.wait()?;
            let result = tuffbox_core::LaunchResult {
                exit_code: status.code(),
                log_path,
                instance_id: manifest_path.to_string_lossy().to_string(),
                profile: profile_id.clone(),
                pid,
                started_at,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::World { command } => run_world_command(command)?,
    }

    Ok(())
}

fn run_world_command(command: WorldCommand) -> anyhow::Result<()> {
    fn estr(e: String) -> anyhow::Error {
        anyhow::Error::msg(e)
    }
    match command {
        WorldCommand::Map { world, dim } => {
            let map = tuffbox_core::region::read_world_map(&world, Some(&dim)).map_err(estr)?;
            println!(
                "{}",
                serde_json::json!({
                    "dimension": map.dimension,
                    "regionCount": map.region_count,
                    "totalPresent": map.total_present,
                    "minRegionX": map.min_region_x,
                    "maxRegionX": map.max_region_x,
                    "minRegionZ": map.min_region_z,
                    "maxRegionZ": map.max_region_z,
                })
            );
        }
        WorldCommand::Delete {
            world,
            selection,
            dim,
        } => {
            let refs = tuffbox_core::region_edit::load_selection_csv_resolved(
                &world,
                &selection,
                Some(&dim),
            )
            .map_err(estr)?;
            let sels = tuffbox_core::region_edit::chunk_refs_to_selections(&refs);
            let n = tuffbox_core::region::delete_world_chunks(&world, &sels, Some(&dim))
                .map_err(estr)?;
            println!("deleted {n} chunk entries from {}", world.display());
        }
        WorldCommand::Export {
            world,
            selection,
            output,
            dim,
        } => {
            let refs = tuffbox_core::region_edit::load_selection_csv_resolved(
                &world,
                &selection,
                Some(&dim),
            )
            .map_err(estr)?;
            let sels = tuffbox_core::region_edit::chunk_refs_to_selections(&refs);
            let n = tuffbox_core::region::export_world_chunks(&world, &sels, Some(&dim), &output)
                .map_err(estr)?;
            println!("exported {n} chunk entries → {}", output.display());
        }
        WorldCommand::Change {
            world,
            fields,
            selection,
            dim,
            force,
        } => {
            let mut change =
                tuffbox_core::region_edit::parse_change_fields(&fields).map_err(estr)?;
            if force {
                change.force = true;
            }
            let refs = if let Some(sel) = selection {
                tuffbox_core::region_edit::load_selection_csv_resolved(&world, &sel, Some(&dim))
                    .map_err(estr)?
            } else {
                tuffbox_core::region_edit::list_present_chunks(&world, Some(&dim)).map_err(estr)?
            };
            let sels = tuffbox_core::region_edit::chunk_refs_to_selections(&refs);
            let n =
                tuffbox_core::region_edit::change_world_chunks(&world, &sels, &change, Some(&dim))
                    .map_err(estr)?;
            println!("changed {n} chunks in {}", world.display());
        }
        WorldCommand::Filter {
            world,
            output,
            palette,
            entities,
            structures,
            min_entities,
            selection,
            dim,
            radius,
        } => {
            let scope = if let Some(sel) = selection {
                let refs = tuffbox_core::region_edit::load_selection_csv_resolved(
                    &world,
                    &sel,
                    Some(&dim),
                )
                .map_err(estr)?;
                tuffbox_core::region_edit::chunk_refs_to_selections(&refs)
            } else {
                Vec::new()
            };
            let filter = tuffbox_core::region_edit::AdvancedChunkFilter {
                entity_names: entities,
                structure_names: structures,
                palette_names: palette,
                min_entities,
                max_entities: None,
            };
            let mut hits = tuffbox_core::region_edit::filter_world_chunks_advanced(
                &world,
                &scope,
                &filter,
                Some(&dim),
            )
            .map_err(estr)?;
            if radius > 0 {
                hits = tuffbox_core::region_edit::expand_chunk_refs(&hits, radius);
            }
            let csv = tuffbox_core::region_edit::write_selection_csv(&hits, false);
            std::fs::write(&output, csv)?;
            println!("wrote {} chunk(s) → {}", hits.len(), output.display());
        }
        WorldCommand::Select {
            world,
            query,
            output,
            dim,
            radius,
        } => {
            let mut hits =
                tuffbox_core::region_edit::select_world_by_query(&world, &query, Some(&dim))
                    .map_err(estr)?;
            if radius > 0 {
                hits = tuffbox_core::region_edit::expand_chunk_refs(&hits, radius);
            }
            let csv = tuffbox_core::region_edit::write_selection_csv(&hits, false);
            std::fs::write(&output, csv)?;
            println!("selected {} chunk(s) → {}", hits.len(), output.display());
        }
        WorldCommand::Import {
            world,
            from,
            selection,
            into_selection,
            offset_x,
            offset_z,
            y_offset,
            sections,
            dim,
            source_dim,
            no_overwrite,
        } => {
            let src_dim = source_dim.as_deref().unwrap_or(&dim);
            let source_sels = if let Some(sel) = selection {
                let refs = tuffbox_core::region_edit::load_selection_csv_resolved(
                    &from,
                    &sel,
                    Some(src_dim),
                )
                .map_err(estr)?;
                tuffbox_core::region_edit::chunk_refs_to_selections(&refs)
            } else {
                Vec::new()
            };
            let target_only = if let Some(sel) = into_selection {
                let refs = tuffbox_core::region_edit::load_selection_csv_resolved(
                    &world,
                    &sel,
                    Some(&dim),
                )
                .map_err(estr)?;
                Some(tuffbox_core::region_edit::chunk_refs_to_selections(&refs))
            } else {
                None
            };
            let opts = tuffbox_core::region::ImportOptions {
                offset_x,
                offset_z,
                overwrite: !no_overwrite,
                sections,
                y_offset,
            };
            let n = tuffbox_core::region::import_world_chunks(
                &world,
                &from,
                &source_sels,
                Some(src_dim),
                Some(&dim),
                &opts,
                target_only.as_deref(),
            )
            .map_err(estr)?;
            println!(
                "imported {n} chunk entries from {} → {} (offset {offset_x},{offset_z} y={y_offset})",
                from.display(),
                world.display()
            );
        }
        WorldCommand::Image {
            world,
            output,
            selection,
            mode,
            scale,
            dim,
        } => {
            let sels = if let Some(sel) = selection {
                let refs = tuffbox_core::region_edit::load_selection_csv_resolved(
                    &world,
                    &sel,
                    Some(&dim),
                )
                .map_err(estr)?;
                tuffbox_core::region_edit::chunk_refs_to_selections(&refs)
            } else {
                Vec::new()
            };
            let color = tuffbox_core::region::MapColorMode::parse(&mode);
            let (w, h) = tuffbox_core::region::render_world_map_png(
                &world,
                Some(&dim),
                &sels,
                color,
                scale,
                &output,
            )
            .map_err(estr)?;
            println!("wrote {w}×{h} PNG → {}", output.display());
        }
        WorldCommand::Cache { world, dim } => {
            let n = tuffbox_core::region::warm_world_map_cache(&world, Some(&dim)).map_err(estr)?;
            let dir = tuffbox_core::region::world_map_cache_dir(&world, &dim).map_err(estr)?;
            println!("cached {n} region(s) → {}", dir.display());
        }
        WorldCommand::CacheClear { world, dim } => {
            let n = tuffbox_core::region::clear_world_map_cache(&world, dim.as_deref())
                .map_err(estr)?;
            println!("cleared {n} cache file(s) for {}", world.display());
        }
        WorldCommand::Purge { world, dim } => {
            let n = tuffbox_core::region::purge_world_regions(&world, Some(&dim)).map_err(estr)?;
            println!("purged {n} region file(s) in {}", world.display());
        }
    }
    Ok(())
}

fn add_mod_from_modrinth(
    manifest: &mut ProjectManifest,
    mod_id: &str,
    side: Option<String>,
) -> anyhow::Result<()> {
    let provider = tuffbox_core::ModrinthProvider::new();
    let project = provider.get_project(mod_id)?;

    if manifest
        .mods
        .iter()
        .any(|m| m.id == project.slug || m.source.project_id.as_deref() == Some(&project.id))
    {
        anyhow::bail!("mod {} is already in the project", project.slug);
    }

    let query = ProviderSearchQuery {
        query: None,
        minecraft_version: Some(manifest.minecraft.version.clone()),
        loader: Some(loader_slug(&manifest.loader.kind)),
        ..Default::default()
    };
    let versions = provider.get_versions(mod_id, &query)?;
    let version = versions
        .into_iter()
        .next()
        .with_context(|| format!("no compatible version found for {mod_id}"))?;

    let file =
        ProviderFileInfo::select_file_for_loader(&version, &loader_slug(&manifest.loader.kind))
            .cloned()
            .with_context(|| format!("no primary file for version {}", version.id))?;

    let dependencies = provider.resolve_dependencies(&version.id)?;

    let side = match side.as_deref() {
        Some("client") => Side::Client,
        Some("server") => Side::Server,
        Some("both") => Side::Both,
        Some("auto") | None => Side::from_modrinth(
            project.client_side.as_deref(),
            project.server_side.as_deref(),
        ),
        Some(other) => {
            anyhow::bail!("invalid side '{other}'; expected: client, server, both, auto")
        }
    };

    let mod_spec = build_mod_spec(&project, &version, file, dependencies, side);
    manifest.mods.push(mod_spec);
    Ok(())
}

fn remove_mod(manifest: &mut ProjectManifest, mod_id: &str) -> anyhow::Result<()> {
    let original_len = manifest.mods.len();
    manifest.mods.retain(|m| m.id != mod_id);
    if manifest.mods.len() == original_len {
        anyhow::bail!("mod {} not found in project", mod_id);
    }
    Ok(())
}

fn update_mod_from_modrinth(manifest: &mut ProjectManifest, mod_id: &str) -> anyhow::Result<()> {
    let provider = tuffbox_core::ModrinthProvider::new();

    let index = manifest
        .mods
        .iter()
        .position(|m| m.id == mod_id)
        .with_context(|| format!("mod {mod_id} not found in project"))?;

    let project_id = match manifest.mods[index].source.project_id.clone() {
        Some(id) => id,
        None => {
            let project = provider.get_project(mod_id)?;
            project.id.clone()
        }
    };
    let project = provider.get_project(&project_id)?;

    let query = ProviderSearchQuery {
        query: None,
        minecraft_version: Some(manifest.minecraft.version.clone()),
        loader: Some(loader_slug(&manifest.loader.kind)),
        ..Default::default()
    };
    let versions = provider.get_versions(&project_id, &query)?;
    let version = versions
        .into_iter()
        .next()
        .with_context(|| format!("no compatible version found for {project_id}"))?;

    let file =
        ProviderFileInfo::select_file_for_loader(&version, &loader_slug(&manifest.loader.kind))
            .cloned()
            .with_context(|| format!("no primary file for version {}", version.id))?;

    let dependencies = provider.resolve_dependencies(&version.id)?;
    let side = manifest.mods[index].side;

    let mod_spec = build_mod_spec(&project, &version, file, dependencies, side);
    manifest.mods[index] = mod_spec;
    Ok(())
}

fn build_mod_spec(
    project: &tuffbox_core::ProjectInfo,
    version: &tuffbox_core::VersionInfo,
    file: ProviderFileInfo,
    dependencies: Vec<tuffbox_core::ModDependencySpec>,
    side: Side,
) -> ModSpec {
    ModSpec {
        id: project.slug.clone(),
        name: project.name.clone(),
        source: ModSource {
            kind: SourceKind::Modrinth,
            project_id: Some(project.id.clone()),
            file_id: Some(version.id.clone()),
            url: Some(file.url),
            path: None,
            icon_url: project.icon_url.clone(),
            categories: Vec::new(),
        },
        version: version.version_number.clone(),
        file_name: Some(file.filename),
        hashes: Some(tuffbox_core::FileHashes {
            sha1: file.hashes.sha1,
            sha512: file.hashes.sha512,
        }),
        side,
        dependencies,
        status: vec!["ok".to_string()],
        content_type: tuffbox_core::manifest::ContentType::from_modrinth_project_type(
            &project.project_type,
        ),
        authors: project
            .author
            .as_ref()
            .map(|a| vec![a.clone()])
            .unwrap_or_default(),
        option: None,
    }
}

fn auto_snapshot(manifest_path: &Path, operation: &str) -> anyhow::Result<Snapshot> {
    let project_dir = manifest_path
        .parent()
        .with_context(|| format!("manifest path has no parent: {}", manifest_path.display()))?;
    let lockfile_path = manifest_path.with_extension("lock.json");
    let lockfile_path = if lockfile_path.exists() {
        Some(lockfile_path)
    } else {
        None
    };
    let store = SnapshotStore::new(project_dir);
    let name = format!("auto-before-{operation}");
    let reason = format!("Auto snapshot before {operation}");
    Ok(store.create(
        &name,
        &reason,
        manifest_path,
        lockfile_path.as_ref(),
        &[] as &[&PathBuf],
    )?)
}

fn save_manifest(path: &Path, manifest: &ProjectManifest) -> anyhow::Result<()> {
    let updated = serde_json::to_string_pretty(manifest)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &updated)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Downloads every manifest-declared entry that isn't already present with
/// a matching hash into its content-type-appropriate folder. Best-effort:
/// failures are printed but don't abort the CLI command, since the
/// manifest write already succeeded and diagnostics/graph will still flag
/// missing files.
fn download_project_mods(
    manifest_path: &Path,
    manifest: &ProjectManifest,
) -> tuffbox_core::ModSyncReport {
    let instance_dir =
        tuffbox_core::instance_dir_for_manifest(manifest_path).unwrap_or_else(|| {
            manifest_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
        });
    let report = tuffbox_core::ensure_project_mods_downloaded(manifest, &instance_dir);
    for failure in &report.failed {
        eprintln!(
            "warning: failed to download mod {}: {}",
            failure.mod_id, failure.error
        );
    }
    report
}

fn loader_slug(kind: &tuffbox_core::LoaderKind) -> String {
    tuffbox_core::graph::loader_kind_slug(kind).to_string()
}

fn load_manifest(path: PathBuf) -> anyhow::Result<ProjectManifest> {
    ProjectManifest::load_from_path(&path)
        .with_context(|| format!("failed to load manifest {}", path.display()))
}
