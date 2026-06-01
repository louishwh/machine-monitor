#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fleetwatch_server::run().await
}
