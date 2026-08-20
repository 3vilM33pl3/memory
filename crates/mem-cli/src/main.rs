// SPDX-License-Identifier: AGPL-3.0-or-later

// Tests may unwrap; production code must not (workspace lints deny it).
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod commands;
mod commits;
mod error_hints;
mod plan_execution;
mod resume;
mod scan;
mod telemetry;
#[cfg(feature = "tui")]
mod tui;
#[cfg(feature = "tui")]
mod wizard;
mod writer_identity;

async fn run_main() -> std::process::ExitCode {
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

#[cfg(not(target_os = "windows"))]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    run_main().await
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    const WINDOWS_MAIN_STACK_SIZE: usize = 8 * 1024 * 1024;
    let thread = match std::thread::Builder::new()
        .name("memory-main".to_string())
        .stack_size(WINDOWS_MAIN_STACK_SIZE)
        .spawn(|| {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("Error: could not initialize the async runtime: {error}");
                    return std::process::ExitCode::FAILURE;
                }
            };
            runtime.block_on(run_main())
        }) {
        Ok(thread) => thread,
        Err(error) => {
            eprintln!("Error: could not start the Memory Layer main thread: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match thread.join() {
        Ok(exit_code) => exit_code,
        Err(_) => {
            eprintln!("Error: the Memory Layer main thread terminated unexpectedly");
            std::process::ExitCode::FAILURE
        }
    }
}
