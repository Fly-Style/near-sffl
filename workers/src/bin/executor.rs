use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use workers::chain::connections::build_executor_providers;
use workers::config;
use workers::executor_def::Executor;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(EnvFilter::builder()
            .with_default_directive(LevelFilter::DEBUG.into())
            .from_env_lossy())
        .init();

    // NOTE: saving it to constant reference to not get into the DCE.
    let config = config::DVNConfig::load_from_env()?;
    let (ws_provider, http_provider) = build_executor_providers(&config).await?;
    
    let executor = Executor::new(config);
    executor.listen( &ws_provider, &http_provider).await?;

    Ok(())
}