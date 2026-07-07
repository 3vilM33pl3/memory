mod commands;
mod commits;
mod error_hints;
mod plan_execution;
mod resume;
mod scan;
mod telemetry;
mod tui;
mod wizard;
mod writer_identity;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match commands::run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            if let Some(hint) = error_hints::error_hint(&error) {
                eprintln!("\n{hint}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}
