#[tokio::main]
async fn main() -> anyhow::Result<()> {
    sqlite_web_starter::startup::run_until_stop().await
}
