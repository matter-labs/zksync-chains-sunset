#[tokio::main]
async fn main() -> eyre::Result<()> {
    zksync_lite_sunset_cli::run().await
}
