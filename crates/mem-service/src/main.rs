use anyhow::Result;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!(
            "memory service {}",
            mem_api::Profile::detect().display_version(env!("CARGO_PKG_VERSION"))
        );
        return Ok(());
    }

    mem_service::run_service(std::env::args().nth(1).map(PathBuf::from)).await
}
