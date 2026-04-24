use std::env;

use anyhow::Result;
use mem_api::AppConfig;
use mem_platform as platform;

#[derive(Debug, Clone)]
pub(crate) struct WriterIdentity {
    pub(crate) id: String,
    pub(crate) name: Option<String>,
}

pub(crate) fn resolve_writer_identity(
    config: &AppConfig,
    cli_writer_id: Option<&str>,
) -> Result<WriterIdentity> {
    resolve_writer_identity_for_tool(config, cli_writer_id, "memory")
}

pub(crate) fn resolve_writer_identity_for_tool(
    config: &AppConfig,
    cli_writer_id: Option<&str>,
    tool_name: &str,
) -> Result<WriterIdentity> {
    if let Some(writer_id) = cli_writer_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(WriterIdentity {
            id: writer_id.to_string(),
            name: config.writer.name.clone(),
        });
    }
    if let Ok(writer_id) = env::var("MEMORY_LAYER_WRITER_ID") {
        let trimmed = writer_id.trim();
        if !trimmed.is_empty() {
            return Ok(WriterIdentity {
                id: trimmed.to_string(),
                name: config.writer.name.clone(),
            });
        }
    }
    if let Ok(writer_id) = env::var("MEMORY_LAYER_AGENT_ID") {
        let trimmed = writer_id.trim();
        if !trimmed.is_empty() {
            return Ok(WriterIdentity {
                id: trimmed.to_string(),
                name: config.writer.name.clone(),
            });
        }
    }
    let trimmed = config.writer.id.trim();
    if !trimmed.is_empty() {
        return Ok(WriterIdentity {
            id: trimmed.to_string(),
            name: config.writer.name.clone(),
        });
    }
    Ok(WriterIdentity {
        id: platform::derive_default_writer_id(tool_name),
        name: config.writer.name.clone(),
    })
}
