use tracing_subscriber::EnvFilter;
use workers::config;
use workers::executor_def::Executor;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // NOTE: saving it to constant reference to not get into the DCE.
    let executor = Executor::new(config::DVNConfig::load_from_env()?);
    executor.listen().await?;

    Ok(())
}