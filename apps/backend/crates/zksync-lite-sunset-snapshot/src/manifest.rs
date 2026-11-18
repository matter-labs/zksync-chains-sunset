use alloy::primitives::B256;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub schema_version: u32,
    pub snapshot_label: String,
    pub created_at: String,
    pub source_checksum: B256,
    pub source_files: Vec<SnapshotSourceFile>,
    pub row_counts: SnapshotRowCounts,
    pub roots: SnapshotRoots,
    pub artifacts: SnapshotArtifactFiles,
    pub compiler: SnapshotCompilerInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSourceFile {
    pub path: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRowCounts {
    pub accounts: usize,
    pub tokens: usize,
    pub balances: usize,
    pub normalized_source_rows: usize,
    pub claim_entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRoots {
    pub claim_root: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotArtifactFiles {
    pub manifest: String,
    pub tokens: String,
    pub claim_entries: String,
    pub claim_proofs: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotCompilerInfo {
    pub package_name: String,
    pub package_version: String,
    pub git_commit: Option<String>,
}
