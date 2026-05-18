use anyhow::Result;
use mem_service as service_runtime;
use std::path::PathBuf;

use crate::commands::{
    runtime::{ServiceArgs, ServiceCommand, default_global_config_path},
    runtime::{
        ensure_shared_service_api_token_for_config, preview_shared_service_api_token_for_config,
    },
    service_support::{
        backend_service_status, disable_backend_service, enable_backend_service,
        preview_disable_backend_service, preview_enable_backend_service,
        restart_all_memory_services,
    },
};

pub(crate) async fn handle(args: &ServiceArgs, cli_config: Option<PathBuf>) -> Result<()> {
    let config_path = cli_config
        .clone()
        .unwrap_or_else(default_global_config_path);
    match &args.command {
        ServiceCommand::Run => {
            service_runtime::run_service(cli_config).await?;
        }
        ServiceCommand::Enable(args) => {
            if args.dry_run {
                println!("{}", preview_enable_backend_service(&config_path));
            } else {
                let token_result =
                    ensure_shared_service_api_token_for_config(&config_path, None, true)?;
                if token_result.changed {
                    println!("{}", token_result.summary_line());
                }
                println!("{}", enable_backend_service(&config_path).await?);
            }
        }
        ServiceCommand::Disable(args) => {
            if args.dry_run {
                println!("{}", preview_disable_backend_service(&config_path));
            } else {
                println!("{}", disable_backend_service()?);
            }
        }
        ServiceCommand::Status => println!("{}", backend_service_status(&config_path)?),
        ServiceCommand::RestartAll(args) => {
            let report = restart_all_memory_services(args.dry_run, args.mark_tui_restart)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.summary());
            }
        }
        ServiceCommand::EnsureApiToken(args) => {
            let _ = args.shared;
            let result = if args.dry_run {
                preview_shared_service_api_token_for_config(
                    &config_path,
                    None,
                    args.rotate_placeholder,
                )?
            } else {
                ensure_shared_service_api_token_for_config(
                    &config_path,
                    None,
                    args.rotate_placeholder,
                )?
            };
            if args.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", result.summary_line());
            }
        }
    }
    Ok(())
}
