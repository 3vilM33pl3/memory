mod types;

pub use types::*;

pub mod config {
    pub use crate::types::{
        AppConfig, AutomationConfig, ClusterConfig, DatabaseConfig, EmbeddingBackendConfig,
        EmbeddingsConfig, FeatureFlags, LlmAuditConfig, LlmConfig, McpConfig, OfflineConfig,
        Profile, RetentionConfig, ServiceConfig, WriterConfig, dev_overlay_path_for_base,
        discover_global_config_path, discover_global_env_path, discover_repo_config_path,
        discover_repo_dev_config_path, discover_repo_env_path, effective_llm_base_url,
        effective_llm_base_url_for, is_ollama_provider, is_supported_llm_provider,
        llm_max_output_tokens_field, llm_requires_api_key, load_repo_replacement_policy,
        resolve_llm_api_key, resolve_secret_value,
    };
}

pub mod env_file {
    pub use crate::types::{
        discover_global_env_path, discover_repo_env_path, resolve_secret_value,
    };
}

pub mod repo_config {
    pub use crate::types::{
        AgentAnalysisConfig, AgentCaptureConfig, AgentCurationConfig, AgentProjectConfig,
        AgentRetrievalConfig, find_repo_config_path, load_repo_agent_settings,
        load_repo_replacement_policy, project_paths_for_repo, project_slug_for_repo,
        read_repo_project_slug, repo_agent_settings_path,
    };
}

pub mod transport {
    pub use crate::types::{
        decode_capnp_text, encode_capnp_text, read_capnp_text_frame, write_capnp_text_frame,
    };
}

pub mod validation {
    pub use crate::types::ValidationError;
}
