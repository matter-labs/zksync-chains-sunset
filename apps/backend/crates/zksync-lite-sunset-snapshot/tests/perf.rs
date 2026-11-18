use std::fs;
use std::time::Instant;
use zksync_lite_sunset_snapshot::compiler::{CompileSnapshotRequest, compile_snapshot};
use zksync_lite_sunset_snapshot::perf_fixture::{
    SyntheticSnapshotSpec, write_snapshot_csv_fixture,
};

#[test]
#[ignore = "manual performance smoke test"]
fn compile_snapshot_performance_smoke() {
    let root = unique_temp_dir("snapshot-perf-smoke");
    let data_dir = root.join("data");
    let out_dir = root.join("out");
    let spec = SyntheticSnapshotSpec::performance_smoke();

    write_snapshot_csv_fixture(&data_dir, spec).expect("fixture data should be written");

    let started_at = Instant::now();
    let result = compile_snapshot(&CompileSnapshotRequest {
        data_dir: data_dir.clone(),
        out_dir: out_dir.clone(),
        snapshot_label: "snapshot-perf-smoke".to_string(),
    })
    .expect("snapshot compile should succeed");

    eprintln!(
        "snapshot-perf-smoke elapsed_ms={} normalized_source_rows={} claim_entries={}",
        started_at.elapsed().as_millis(),
        result.manifest.row_counts.normalized_source_rows,
        result.manifest.row_counts.claim_entries,
    );

    let _ = fs::remove_dir_all(root);
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}
