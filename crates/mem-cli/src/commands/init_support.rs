#![allow(unused_imports)]

use crate::{commits, resume, scan, tui, wizard};
use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::{fs::PermissionsExt, net::UnixStream};
use std::{
    env, fs,
    io::{self, IsTerminal, Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;
use mem_agenttop::LightweightAgentSession;
use mem_api::{
    ActivityListResponse, AppConfig, ArchiveRequest, ArchiveResponse, CaptureTaskRequest,
    CheckpointActivityRequest, CommitDetailResponse, CommitSyncRequest, CommitSyncResponse,
    CurateRequest, CurateResponse, DeleteMemoryRequest, DeleteMemoryResponse, GraphActivityRequest,
    MemoryEntryResponse, MemoryType, PlanActivityAction, PlanActivityRequest, Profile,
    ProjectCommitsResponse, ProjectMemoriesResponse, ProjectMemoryBundlePreview,
    ProjectMemoryExportOptions, ProjectMemoryImportPreview, ProjectMemoryImportResponse,
    ProjectOverviewResponse, ProvenanceVerificationRequest, ProvenanceVerificationResponse,
    PruneEmbeddingsRequest, PruneEmbeddingsResponse, QueryFilters, QueryRequest, QueryResponse,
    ReembedRequest, ReembedResponse, ReindexRequest, ReindexResponse, ReplacementPolicy,
    ResumeRequest, ResumeResponse, ScanActivityRequest, TestResult, TokenUsage, UpToSpeedRequest,
    UpToSpeedResponse, discover_global_config_path, discover_repo_env_path, effective_llm_base_url,
    is_ollama_provider, is_supported_llm_provider, llm_max_output_tokens_field,
    llm_requires_api_key, load_repo_replacement_policy, read_repo_project_slug,
    resolve_llm_api_key,
};
use mem_platform as platform;
use mem_watch::{
    build_capture_request as build_automation_capture_request,
    detect_changed_files as watch_detect_changed_files,
    fetch_project_overview as fetch_automation_overview, load_state, should_capture, should_curate,
    update_session_from_repo,
};
use reqwest::{Client, header::HeaderMap};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgPoolOptions};
use uuid::Uuid;

use crate::commands::runtime::*;
use crate::plan_execution::{
    durable_plan_source_path, ensure_checkbox_plan, normalize_plan_markdown_for_hash,
    parse_plan_checkboxes,
};
use crate::writer_identity::{WriterIdentity, resolve_writer_identity};

pub(crate) fn repo_replacement_policy(repo_root: &Path) -> ReplacementPolicy {
    load_repo_replacement_policy(repo_root).unwrap_or_default()
}

pub(crate) fn initialize_repo(
    repo_root: &Path,
    project: &str,
    force: bool,
    print_only: bool,
) -> Result<String> {
    let mem_dir = repo_root.join(".mem");
    let project_paths = mem_platform::project_paths(repo_root, project)
        .ok_or_else(|| anyhow::anyhow!("could not resolve user project config paths"))?;
    let runtime_dir = project_paths.runtime_dir();
    let config_path = project_paths.config_path();
    let env_path = project_paths.env_path();
    let home_project_path = project_paths.project_path();
    let project_path = mem_dir.join("project.toml");
    let legacy_config_path = mem_dir.join("config.toml");
    let legacy_env_path = mem_dir.join("memory-layer.env");
    let local_gitignore_path = mem_dir.join(".gitignore");
    let agent_config_path = repo_root.join(".agents").join("memory-layer.toml");
    let skill_root = repo_root.join(".agents").join("skills");
    let skill_template_dir = discover_skill_template_dir()
        .ok_or_else(|| anyhow::anyhow!("could not locate packaged memory-layer skill template"))?;

    let config_contents = render_repo_config(repo_root, &project_paths);
    let project_contents = render_project_metadata(project, repo_root);
    let agent_project_contents = render_agent_project_config(project, repo_root);
    if !print_only {
        fs::create_dir_all(&project_paths.config_dir)
            .with_context(|| format!("create {}", project_paths.config_dir.display()))?;
        fs::create_dir_all(&runtime_dir)
            .with_context(|| format!("create {}", runtime_dir.display()))?;
        if force || !config_path.exists() {
            if !force && legacy_config_path.exists() {
                fs::copy(&legacy_config_path, &config_path).with_context(|| {
                    format!(
                        "copy {} to {}",
                        legacy_config_path.display(),
                        config_path.display()
                    )
                })?;
            } else {
                fs::write(&config_path, config_contents)
                    .with_context(|| format!("write {}", config_path.display()))?;
            }
        }
        if !force && !env_path.exists() && legacy_env_path.exists() {
            migrate_legacy_env_or_create_token(&legacy_env_path, &env_path)?;
        }
        if force || !home_project_path.exists() {
            fs::write(&home_project_path, &project_contents)
                .with_context(|| format!("write {}", home_project_path.display()))?;
        }
        fs::create_dir_all(&mem_dir).context("create .mem")?;
        if force || !project_path.exists() {
            fs::write(&project_path, project_contents).context("write .mem/project.toml")?;
        }
        if let Some(parent) = agent_config_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        if force || !agent_config_path.exists() {
            fs::write(&agent_config_path, agent_project_contents)
                .context("write .agents/memory-layer.toml")?;
        }
        ensure_mem_gitignore(&local_gitignore_path, force)?;
        sync_memory_skill_bundle(&skill_template_dir, &skill_root, force)?;
        ensure_claude_md_memory_section(repo_root, project)?;
    }

    Ok(render_init_summary(
        repo_root,
        project,
        &config_path,
        &project_path,
        &agent_config_path,
        &skill_root,
        print_only,
    ))
}

pub(crate) fn ensure_mem_gitignore(path: &Path, force: bool) -> Result<()> {
    const CONTENTS: &str = "*\n!.gitignore\n!project.toml\n";
    if force || !path.exists() {
        fs::write(path, CONTENTS).context("write .mem/.gitignore")?;
        return Ok(());
    }

    let current = fs::read_to_string(path).context("read .mem/.gitignore")?;
    let lines: Vec<&str> = current
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let allowed = ["*", "!.gitignore", "!project.toml"];
    let has_safe_default = allowed
        .into_iter()
        .all(|required| lines.contains(&required));
    let has_only_safe_rules = lines.iter().all(|line| allowed.contains(line));
    if !has_safe_default || !has_only_safe_rules {
        fs::write(path, CONTENTS).context("migrate .mem/.gitignore")?;
    }

    Ok(())
}

pub(crate) fn initialize_dev_overlay(repo_root: &Path, args: &DevInitArgs) -> Result<String> {
    let project = mem_api::project_slug_for_repo(repo_root);
    let project_paths = mem_platform::project_paths(repo_root, &project)
        .ok_or_else(|| anyhow::anyhow!("could not resolve user project config paths"))?;
    let base_config_path = project_paths.config_path();
    if !base_config_path.is_file() && !repo_root.join(".mem").join("config.toml").is_file() {
        anyhow::bail!(
            "no project config found at {}. Run `memory init` first to bootstrap the \
             base config before layering the dev overlay on top.",
            base_config_path.display()
        );
    }
    let overlay_path = project_paths.dev_config_path();
    let runtime_dev_dir = project_paths.runtime_dir().join("dev");
    let capnp_unix_socket = dev_capnp_unix_socket_path(&project_paths);
    let state_file_path = runtime_dev_dir.join("automation-state.json");
    let audit_log_path = runtime_dev_dir.join("automation.log");

    let shared_snippet = resolve_shared_global_snippet(args)?;

    let mut contents = format!(
        "# Overlay on top of the user-local project config for the dev profile.\n\
         # Active when MEMORY_LAYER_PROFILE=dev or the binary runs from a cargo target/ directory.\n\
         # The dev profile does NOT read the global config — anything shared (database URL,\n\
         # LLM endpoints) lives here. Re-run `memory dev init --copy-from-global` to refresh.\n\
         \n\
         [service]\n\
         bind_addr = \"{bind_addr}\"\n\
         capnp_tcp_addr = \"{capnp_tcp_addr}\"\n\
         capnp_unix_socket = \"{capnp_unix_socket}\"\n\
         \n\
         [automation]\n\
         state_file_path = \"{state_file_path}\"\n\
         audit_log_path = \"{audit_log_path}\"\n\
         \n\
         [cluster]\n\
         service_id = \"memory-layer-dev\"\n",
        bind_addr = args.bind_addr,
        capnp_tcp_addr = args.capnp_tcp_addr,
        capnp_unix_socket = capnp_unix_socket.display(),
        state_file_path = state_file_path.display(),
        audit_log_path = audit_log_path.display(),
    );
    if !shared_snippet.is_empty() {
        contents.push('\n');
        contents.push_str(&shared_snippet);
    }

    if args.dry_run {
        return Ok(format!(
            "[dry run] would write {} ({} bytes) and create {}\n\n{}",
            overlay_path.display(),
            contents.len(),
            runtime_dev_dir.display(),
            contents
        ));
    }

    if let Some(parent) = overlay_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::create_dir_all(&runtime_dev_dir)
        .with_context(|| format!("create {}", runtime_dev_dir.display()))?;
    if overlay_path.exists() && !args.force {
        return Ok(format!(
            "dev overlay already present at {} (use --force to overwrite)",
            overlay_path.display()
        ));
    }
    fs::write(&overlay_path, &contents)
        .with_context(|| format!("write {}", overlay_path.display()))?;
    Ok(format!(
        "wrote {} and ensured {}\nnext: run `cargo run --bin memory -- service run` in another \
         shell, then `cargo run --bin memory -- tui`.",
        overlay_path.display(),
        runtime_dev_dir.display()
    ))
}

pub(crate) fn dev_capnp_unix_socket_path(project_paths: &mem_platform::ProjectPaths) -> PathBuf {
    let identity = project_paths
        .key
        .rsplit_once('-')
        .map(|(_, hash)| hash)
        .unwrap_or(&project_paths.key);
    env::temp_dir().join(format!("memory-layer-dev-{identity}.sock"))
}

/// Tables we willingly copy from the global config into the dev overlay. The
/// service endpoint + automation paths + cluster id are intentionally
/// excluded so the dev stack always diverges where it matters.
const SHARED_GLOBAL_SECTIONS: &[&str] = &["database", "llm", "embeddings", "features", "writer"];

pub(crate) fn resolve_shared_global_snippet(args: &DevInitArgs) -> Result<String> {
    let Some(global_path) = mem_api::discover_global_config_path() else {
        if args.copy_from_global {
            anyhow::bail!(
                "--copy-from-global was set but no global config was found \
                 (expected one of the paths reported by `memory doctor`)"
            );
        }
        return Ok(String::new());
    };
    let should_copy = if args.copy_from_global {
        true
    } else if args.no_copy_from_global {
        false
    } else if io::stdin().is_terminal() && io::stdout().is_terminal() {
        prompt_yes_no(&format!(
            "Copy shared settings (database URL, LLM/embedding endpoints) from {} into the dev overlay?",
            global_path.display()
        ))?
    } else {
        false
    };
    if !should_copy {
        return Ok(String::new());
    }
    let raw = fs::read_to_string(&global_path)
        .with_context(|| format!("read {}", global_path.display()))?;
    let value: toml::Value =
        toml::from_str(&raw).with_context(|| format!("parse {}", global_path.display()))?;
    let Some(table) = value.as_table() else {
        return Ok(String::new());
    };
    let mut copied = toml::value::Table::new();
    for section in SHARED_GLOBAL_SECTIONS {
        if let Some(value) = table.get(*section) {
            copied.insert((*section).to_string(), value.clone());
        }
    }
    if copied.is_empty() {
        return Ok(String::new());
    }
    let rendered =
        toml::to_string(&toml::Value::Table(copied)).context("serialize shared sections")?;
    Ok(format!(
        "# Copied from {} — re-run `memory dev init --copy-from-global --force` to refresh.\n{}",
        global_path.display(),
        rendered
    ))
}
