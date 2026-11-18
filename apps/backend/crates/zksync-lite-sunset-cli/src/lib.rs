mod compose_seeded_smoke;

use clap::{Parser, Subcommand};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::info;
use tracing_subscriber::EnvFilter;
use url::Url;
use zksync_lite_sunset_core::config::Config;
use zksync_lite_sunset_core::dev_seed::{
    self, DevFixtureMode, DevSeedRequest, GenerateDevSnapshotRequest,
};
use zksync_lite_sunset_core::snapshots;
use zksync_lite_sunset_core::snapshots::{
    ClaimMaterializationRequest, ClaimsImportRequest, SnapshotImportRequest,
};
use zksync_lite_sunset_snapshot::compiler::{CompileSnapshotRequest, compile_snapshot};
use zksync_lite_sunset_snapshot::importer::{
    ImportSnapshotArtifactsRequest, import_snapshot_artifacts,
};

#[derive(Debug, Parser)]
#[command(name = "zksync-lite-sunset-cli")]
#[command(about = "Backend utilities for zkSync Lite sunset", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    DbInit {
        #[arg(long)]
        database_url: Url,
    },
    ClaimsTokenTotal {
        #[arg(long)]
        database_url: Url,
        #[arg(long)]
        token_address: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    ImportSnapshots {
        #[arg(long)]
        database_url: Url,
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
        #[arg(long, default_value_t = NonZeroUsize::new(5000).expect("5000 is non-zero"))]
        chunk_size: NonZeroUsize,
        #[arg(long, default_value_t = false)]
        truncate: bool,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    ClaimsMaterialize {
        #[arg(long)]
        database_url: Url,
        #[arg(long)]
        snapshot_label: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    ClaimsImport {
        #[arg(long)]
        database_url: Url,
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
        #[arg(long, default_value_t = NonZeroUsize::new(5000).expect("5000 is non-zero"))]
        chunk_size: NonZeroUsize,
        #[arg(long, default_value_t = false)]
        truncate: bool,
        #[arg(long)]
        snapshot_label: Option<String>,
        #[arg(long, default_value_t = false)]
        reset_runtime_state: bool,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    SnapshotsCompile {
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long)]
        snapshot_label: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    SnapshotsImportArtifacts {
        #[arg(long)]
        database_url: Url,
        #[arg(long)]
        artifact_dir: PathBuf,
        #[arg(long, default_value_t = false)]
        reset_runtime_state: bool,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    GenerateDevSnapshot {
        #[arg(long)]
        address: alloy::primitives::Address,
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long, default_value = "default")]
        mode: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    DevSeedSnapshot {
        #[arg(long)]
        database_url: Url,
        #[arg(long)]
        address: alloy::primitives::Address,
        #[arg(long)]
        out_dir: Option<PathBuf>,
        #[arg(long, default_value = "default")]
        mode: String,
        #[arg(long)]
        snapshot_label: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    ComposeSeededSmoke(compose_seeded_smoke::ComposeSeededSmokeArgs),
}

pub async fn run() -> eyre::Result<()> {
    init_logging();
    let cli = Cli::parse();
    match cli.command {
        Commands::DbInit { database_url } => {
            snapshots::initialize_snapshot_storage(&database_url).await?;
            println!("db initialized");
        }
        Commands::ClaimsTokenTotal {
            database_url,
            token_address,
            json,
        } => {
            let result =
                snapshots::sum_claim_entries_for_token(&database_url, &token_address).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", result.total);
            }
        }
        Commands::ImportSnapshots {
            database_url,
            data_dir,
            chunk_size,
            truncate,
            json,
        } => {
            info!(
                component = "cli",
                command = "import-snapshots",
                data_dir = %data_dir.display(),
                chunk_size = chunk_size.get(),
                truncate,
                "starting command"
            );
            let request = SnapshotImportRequest {
                database_url,
                data_dir,
                chunk_size,
                truncate,
            };
            let counts = snapshots::import_snapshot_state(&request).await?;
            info!(
                component = "cli",
                command = "import-snapshots",
                accounts = counts.accounts,
                tokens = counts.tokens,
                balances = counts.balances,
                "completed command"
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&counts)?);
            } else {
                println!("imported accounts: {}", counts.accounts);
                println!("imported tokens: {}", counts.tokens);
                println!("imported balances: {}", counts.balances);
                println!("snapshot import completed");
            }
        }
        Commands::ClaimsMaterialize {
            database_url,
            snapshot_label,
            json,
        } => {
            let cfg = Config::default();
            let request = ClaimMaterializationRequest {
                database_url,
                snapshot_label: snapshot_label.unwrap_or_else(|| {
                    format!("{}-manual", cfg.materialization.snapshot_label_prefix)
                }),
            };
            info!(
                component = "cli",
                command = "claims-materialize",
                snapshot_label = %request.snapshot_label,
                "starting command"
            );
            let result = snapshots::materialize_claims(&request).await?;
            info!(
                component = "cli",
                command = "claims-materialize",
                entry_count = result.entry_count,
                source_checksum = %format!("{:#x}", result.source_checksum),
                merkle_root = %format!("{:#x}", result.merkle_root),
                "completed command"
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("snapshot label: {}", result.label);
                println!("merkle root: {:#x}", result.merkle_root);
                println!("source checksum: {:#x}", result.source_checksum);
                println!("entry count: {}", result.entry_count);
                println!("claim materialization completed");
            }
        }
        Commands::ClaimsImport {
            database_url,
            data_dir,
            chunk_size,
            truncate,
            snapshot_label,
            reset_runtime_state,
            json,
        } => {
            let cfg = Config::default();
            let request = ClaimsImportRequest {
                database_url,
                data_dir,
                chunk_size,
                truncate,
                snapshot_label: snapshot_label.unwrap_or_else(|| {
                    format!("{}-import", cfg.materialization.snapshot_label_prefix)
                }),
                reset_runtime_state,
            };
            info!(
                component = "cli",
                command = "claims-import",
                data_dir = %request.data_dir.display(),
                chunk_size = request.chunk_size.get(),
                truncate = request.truncate,
                snapshot_label = %request.snapshot_label,
                reset_runtime_state = request.reset_runtime_state,
                "starting command"
            );
            let result = snapshots::import_and_materialize_claims(&request).await?;
            info!(
                component = "cli",
                command = "claims-import",
                imported_accounts = result.imported.accounts,
                imported_tokens = result.imported.tokens,
                imported_balances = result.imported.balances,
                claim_entry_count = result.materialized.entry_count,
                "completed command"
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("imported accounts: {}", result.imported.accounts);
                println!("imported tokens: {}", result.imported.tokens);
                println!("imported balances: {}", result.imported.balances);
                println!("snapshot label: {}", result.materialized.label);
                println!("merkle root: {:#x}", result.materialized.merkle_root);
                println!(
                    "source checksum: {:#x}",
                    result.materialized.source_checksum
                );
                println!("entry count: {}", result.materialized.entry_count);
                println!("claims import completed");
            }
        }
        Commands::SnapshotsCompile {
            data_dir,
            out_dir,
            snapshot_label,
            json,
        } => {
            let cfg = Config::default();
            let request = CompileSnapshotRequest {
                data_dir,
                out_dir,
                snapshot_label: snapshot_label.unwrap_or_else(|| {
                    format!("{}-artifact", cfg.materialization.snapshot_label_prefix)
                }),
            };
            info!(
                component = "cli",
                command = "snapshots-compile",
                data_dir = %request.data_dir.display(),
                out_dir = %request.out_dir.display(),
                snapshot_label = %request.snapshot_label,
                "starting command"
            );
            let result = compile_snapshot(&request)?;
            info!(
                component = "cli",
                command = "snapshots-compile",
                out_dir = %result.out_dir.display(),
                source_checksum = %format!("{:#x}", result.manifest.source_checksum),
                claim_root = %format!("{:#x}", result.manifest.roots.claim_root),
                "completed command"
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&result.manifest)?);
            } else {
                println!("artifact dir: {}", result.out_dir.display());
                println!("snapshot label: {}", result.manifest.snapshot_label);
                println!("source checksum: {:#x}", result.manifest.source_checksum);
                println!("claim root: {:#x}", result.manifest.roots.claim_root);
                println!(
                    "normalized source rows: {}",
                    result.manifest.row_counts.normalized_source_rows
                );
                println!("snapshot compile completed");
            }
        }
        Commands::SnapshotsImportArtifacts {
            database_url,
            artifact_dir,
            reset_runtime_state,
            json,
        } => {
            snapshots::initialize_snapshot_storage(&database_url).await?;
            let request = ImportSnapshotArtifactsRequest {
                database_url,
                artifact_dir,
                reset_runtime_state,
            };
            info!(
                component = "cli",
                command = "snapshots-import-artifacts",
                artifact_dir = %request.artifact_dir.display(),
                reset_runtime_state = request.reset_runtime_state,
                "starting command"
            );
            let result = import_snapshot_artifacts(&request).await?;
            info!(
                component = "cli",
                command = "snapshots-import-artifacts",
                artifact_dir = %result.artifact_dir.display(),
                source_checksum = %format!("{:#x}", result.source_checksum),
                "completed command"
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("artifact dir: {}", result.artifact_dir.display());
                println!("snapshot label: {}", result.snapshot_label);
                println!("source checksum: {:#x}", result.source_checksum);
                println!("artifact import completed");
            }
        }
        Commands::GenerateDevSnapshot {
            address,
            out_dir,
            mode,
            json,
        } => {
            let mode = DevFixtureMode::from_str(&mode)?;
            info!(
                component = "cli",
                command = "generate-dev-snapshot",
                address = %format!("{address:#x}"),
                mode = mode.as_str(),
                output_dir = %out_dir.display(),
                "starting command"
            );
            let result = dev_seed::generate_dev_snapshot(&GenerateDevSnapshotRequest {
                address,
                output_dir: out_dir,
                mode,
            })
            .await?;
            info!(
                component = "cli",
                command = "generate-dev-snapshot",
                token_count = result.token_count,
                output_dir = %result.output_dir.display(),
                "completed command"
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("address: {:#x}", result.address);
                println!("mode: {}", result.mode.as_str());
                println!("output dir: {}", result.output_dir.display());
                println!("token count: {}", result.token_count);
                println!("dev snapshot generated");
            }
        }
        Commands::DevSeedSnapshot {
            database_url,
            address,
            out_dir,
            mode,
            snapshot_label,
            json,
        } => {
            let mode = DevFixtureMode::from_str(&mode)?;
            let snapshot_label = snapshot_label.unwrap_or_else(dev_seed::default_snapshot_label);
            info!(
                component = "cli",
                command = "dev-seed-snapshot",
                address = %format!("{address:#x}"),
                mode = mode.as_str(),
                snapshot_label = %snapshot_label,
                "starting command"
            );
            let result = dev_seed::seed_dev_snapshot(&DevSeedRequest {
                database_url,
                address,
                mode,
                snapshot_label,
                output_dir: out_dir,
            })
            .await?;
            info!(
                component = "cli",
                command = "dev-seed-snapshot",
                imported_accounts = result.imported.accounts,
                imported_tokens = result.imported.tokens,
                imported_balances = result.imported.balances,
                entry_count = result.materialized.entry_count,
                "completed command"
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("address: {:#x}", result.address);
                println!("mode: {}", result.mode.as_str());
                println!("output dir: {}", result.output_dir.display());
                println!("imported accounts: {}", result.imported.accounts);
                println!("imported tokens: {}", result.imported.tokens);
                println!("imported balances: {}", result.imported.balances);
                println!("snapshot label: {}", result.materialized.label);
                println!("merkle root: {:#x}", result.materialized.merkle_root);
                println!("entry count: {}", result.materialized.entry_count);
                println!("dev snapshot seeded");
            }
        }
        Commands::ComposeSeededSmoke(args) => {
            compose_seeded_smoke::run(args).await?;
        }
    }

    Ok(())
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
        .compact()
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}
