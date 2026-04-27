use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Ollama,
    Vllm,
    Litellm,
    OpenAiCompatible,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::Vllm => "vllm",
            Self::Litellm => "litellm",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub id: String,
    pub name: String,
    pub backend_kind: BackendKind,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub system_prompt: String,
    pub temperature: Option<f32>,
    pub context_hint: Option<String>,
}

impl ModelProfile {
    pub fn default_local() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default local model".to_string(),
            backend_kind: BackendKind::Ollama,
            base_url: "http://localhost:11434/v1".to_string(),
            model: "qwen2.5-coder:7b".to_string(),
            api_key: None,
            system_prompt: default_system_prompt(),
            temperature: None,
            context_hint: Some("Default Ollama-compatible local profile".to_string()),
        }
    }

    pub fn from_legacy(
        id: impl Into<String>,
        base_url: String,
        model: String,
        api_key: Option<String>,
    ) -> Self {
        let model = if model.trim().is_empty() {
            "qwen2.5-coder:7b".to_string()
        } else {
            model
        };
        Self {
            id: id.into(),
            name: format!("Local {model}"),
            backend_kind: BackendKind::Ollama,
            base_url,
            model,
            api_key,
            system_prompt: default_system_prompt(),
            temperature: None,
            context_hint: Some("Migrated from legacy settings".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_active_profile")]
    pub active_profile: String,
    #[serde(default)]
    pub model_profiles: Vec<ModelProfile>,
    pub llm_base_url: String,
    pub llm_model: String,
    pub api_key: Option<String>,
    pub default_workspace: PathBuf,
    pub command_timeout_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        let profile = ModelProfile::default_local();
        Self {
            active_profile: profile.id.clone(),
            model_profiles: vec![profile.clone()],
            llm_base_url: profile.base_url,
            llm_model: profile.model,
            api_key: None,
            default_workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            command_timeout_secs: 120,
        }
    }
}

impl AppSettings {
    pub fn ensure_model_profiles(&mut self) {
        if self.active_profile.trim().is_empty() {
            self.active_profile = default_active_profile();
        }
        if self.model_profiles.is_empty() {
            self.model_profiles.push(ModelProfile::from_legacy(
                self.active_profile.clone(),
                self.llm_base_url.clone(),
                self.llm_model.clone(),
                self.api_key.clone(),
            ));
        }
        if self.active_model_profile().is_none() {
            let first = self
                .model_profiles
                .first()
                .map(|profile| profile.id.clone())
                .unwrap_or_else(default_active_profile);
            self.active_profile = first;
        }
        let profile = self
            .active_model_profile()
            .cloned()
            .unwrap_or_else(ModelProfile::default_local);
        self.llm_base_url = profile.base_url;
        self.llm_model = profile.model;
        self.api_key = profile.api_key;
    }

    pub fn active_model_profile(&self) -> Option<&ModelProfile> {
        self.model_profiles
            .iter()
            .find(|profile| profile.id == self.active_profile)
    }

    pub fn active_model_profile_mut(&mut self) -> Option<&mut ModelProfile> {
        self.model_profiles
            .iter_mut()
            .find(|profile| profile.id == self.active_profile)
    }
}

fn default_active_profile() -> String {
    "default".to_string()
}

pub fn default_system_prompt() -> String {
    "You are CodeSmith, a local execution-only coding agent. Explain clearly, prefer safe read-only diagnostics, and never claim a command has run unless tool output proves it. When proposing a shell command, return strict JSON only with command, cwd, and reason fields. Do not wrap command proposal JSON in Markdown fences. Commands require explicit user approval before execution."
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

impl ChatRole {
    pub fn as_openai(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub role: ChatRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub tool_calls: Vec<CommandProposal>,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            content,
            timestamp: Utc::now(),
            tool_calls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandProposal {
    pub command: String,
    pub cwd: PathBuf,
    pub reason: String,
    pub risk_level: RiskLevel,
    pub requires_approval: bool,
}

impl CommandProposal {
    pub fn new(command: impl Into<String>, cwd: PathBuf, reason: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            cwd,
            reason: reason.into(),
            risk_level: RiskLevel::Low,
            requires_approval: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandStatus {
    PendingApproval,
    Rejected,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRun {
    pub id: Uuid,
    pub proposal: CommandProposal,
    pub status: CommandStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

impl CommandRun {
    pub fn new(
        proposal: CommandProposal,
        status: CommandStatus,
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            proposal,
            status,
            stdout,
            stderr,
            exit_code,
            started_at: now,
            finished_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub requires_approval: bool,
    pub risk_level: RiskLevel,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WikiStatus {
    Active,
    Conflict,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiPage {
    pub id: Uuid,
    pub title: String,
    pub domain: String,
    pub source_count: u32,
    pub confidence: f32,
    pub status: WikiStatus,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceStatus {
    Active,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: Uuid,
    pub path: PathBuf,
    pub hash: String,
    pub kind: String,
    pub ingested_at: DateTime<Utc>,
    pub status: SourceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestJob {
    pub id: Uuid,
    pub source_id: Uuid,
    pub status: SourceStatus,
    pub analysis_path: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiPageMetadata {
    pub id: Uuid,
    pub title: String,
    pub path: PathBuf,
    pub sources: Vec<Uuid>,
    pub updated_at: DateTime<Utc>,
    pub status: WikiStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmEvent {
    Token(String),
    Finished,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerEvent {
    Stdout(String),
    Stderr(String),
    Finished(CommandStatus),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageEvent {
    SessionSaved(Uuid),
    CommandRunSaved(Uuid),
}
