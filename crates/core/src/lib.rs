use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    pub llm_base_url: String,
    pub llm_model: String,
    pub api_key: Option<String>,
    pub default_workspace: PathBuf,
    pub command_timeout_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            llm_base_url: "http://localhost:11434/v1".to_string(),
            llm_model: "qwen2.5-coder:7b".to_string(),
            api_key: None,
            default_workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            command_timeout_secs: 120,
        }
    }
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
