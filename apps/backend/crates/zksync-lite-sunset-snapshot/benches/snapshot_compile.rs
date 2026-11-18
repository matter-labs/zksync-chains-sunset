use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zksync_lite_sunset_snapshot::compiler::{
    CompileSnapshotRequest, compile_snapshot, materialize_claim_entries,
};
use zksync_lite_sunset_snapshot::perf_fixture::{
    SyntheticSnapshotSpec, generate_source_rows, write_snapshot_csv_fixture,
};

fn snapshot_materialization_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot-materialization");
    let medium_rows =
        generate_source_rows(SyntheticSnapshotSpec::benchmark_medium()).expect("fixture rows");

    group.bench_function(BenchmarkId::new("claim", "medium"), |b| {
        b.iter_batched(
            || medium_rows.clone(),
            |rows| materialize_claim_entries(rows).expect("claim materialization"),
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

fn snapshot_compile_bench(c: &mut Criterion) {
    let root = unique_temp_dir("snapshot-bench");
    let data_dir = root.join("data");
    write_snapshot_csv_fixture(&data_dir, SyntheticSnapshotSpec::benchmark_small())
        .expect("fixture data");

    c.bench_function("snapshot-compile/small-fixture", |b| {
        let mut iteration = 0usize;
        b.iter_batched(
            || {
                iteration += 1;
                root.join(format!("out-{iteration}"))
            },
            |out_dir| {
                compile_snapshot(&CompileSnapshotRequest {
                    data_dir: data_dir.clone(),
                    out_dir: out_dir.clone(),
                    snapshot_label: "snapshot-bench".to_string(),
                })
                .expect("snapshot compile");
                cleanup_dir(&out_dir);
            },
            BatchSize::PerIteration,
        );
    });

    cleanup_dir(&root);
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

fn cleanup_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

criterion_group!(
    benches,
    snapshot_materialization_benches,
    snapshot_compile_bench
);
criterion_main!(benches);
