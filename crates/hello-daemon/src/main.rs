use clap::Parser;

#[derive(Parser)]
#[command(
    version,
    about = "Linux Hello diagnostic D-Bus service (authentication unavailable)"
)]
struct Arguments {
    /// Use an unprivileged session bus for development; never a login authority.
    #[arg(long)]
    session: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Arguments::parse();
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();
    #[cfg(target_os = "linux")]
    {
        let _connection = hello_daemon::serve(args.session).await?;
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            diagnostic_only = true,
            "Linux Hello diagnostics available"
        );
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
        tracing::info!("Linux Hello stopped");
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        Err("The diagnostic daemon requires Linux".into())
    }
}
