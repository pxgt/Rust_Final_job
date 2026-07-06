use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    specprobe::run().await
}
