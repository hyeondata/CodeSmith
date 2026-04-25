use anyhow::{Context, Result};
use codesmith_agent::{AgentOutput, parse_agent_output};
use codesmith_core::{AppSettings, ChatMessage, ChatRole, CommandProposal};
use codesmith_llm::OpenAiClient;
use codesmith_policy::evaluate;
use codesmith_runner::run_approved_command;
use codesmith_wiki::WikiStore;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn approval_hint() -> &'static str {
    "approval required: rerun with --yes to execute this allowed command"
}

pub fn repl_help() -> &'static str {
    "CodeSmith interactive commands\n\
     /help                     show this help\n\
     /settings                 show all current settings\n\
     /set base-url <url>       set OpenAI-compatible base URL\n\
     /set model <name>         set local model name\n\
     /set api-key <key|none>   set or clear API key placeholder\n\
     /set workspace <path>     set command workspace\n\
     /set timeout <seconds>    set command timeout\n\
     /prompts                  show recommended prompts\n\
     /doctor                   test local LLM connection\n\
     /wiki list                list saved wiki pages\n\
     /wiki search <query>      search local wiki\n\
     /exit                     quit\n"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplCommand {
    Empty,
    Help,
    Prompts,
    Settings,
    Set(SettingUpdate),
    Doctor,
    WikiList,
    WikiSearch(String),
    Exit,
    Prompt(String),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingUpdate {
    BaseUrl(String),
    Model(String),
    ApiKey(Option<String>),
    Workspace(PathBuf),
    TimeoutSecs(u64),
}

pub fn parse_repl_line(line: &str) -> ReplCommand {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ReplCommand::Empty;
    }
    if !trimmed.starts_with('/') {
        return ReplCommand::Prompt(trimmed.to_string());
    }

    match trimmed {
        "/exit" | "/quit" => ReplCommand::Exit,
        "/help" => ReplCommand::Help,
        "/prompts" => ReplCommand::Prompts,
        "/settings" => ReplCommand::Settings,
        "/doctor" => ReplCommand::Doctor,
        "/wiki list" => ReplCommand::WikiList,
        _ => parse_parameterized_repl_command(trimmed),
    }
}

pub fn apply_setting_update(settings: &mut AppSettings, update: SettingUpdate) -> Result<String> {
    let message = match update {
        SettingUpdate::BaseUrl(value) => {
            ensure_non_empty("base-url", &value)?;
            settings.llm_base_url = value;
            "base-url updated".to_string()
        }
        SettingUpdate::Model(value) => {
            ensure_non_empty("model", &value)?;
            settings.llm_model = value;
            "model updated".to_string()
        }
        SettingUpdate::ApiKey(value) => {
            settings.api_key = value.filter(|key| !key.trim().is_empty());
            "api-key updated".to_string()
        }
        SettingUpdate::Workspace(value) => {
            settings.default_workspace = value;
            "workspace updated".to_string()
        }
        SettingUpdate::TimeoutSecs(value) => {
            if value == 0 {
                anyhow::bail!("timeout must be greater than 0");
            }
            settings.command_timeout_secs = value;
            "timeout updated".to_string()
        }
    };
    Ok(message)
}

pub fn settings_summary(settings: &AppSettings, settings_path: &Path) -> String {
    format!(
        "Settings\npath: {}\nbase-url: {}\nmodel: {}\napi-key: {}\nworkspace: {}\ntimeout: {}s\n",
        settings_path.display(),
        settings.llm_base_url,
        settings.llm_model,
        if settings.api_key.as_deref().unwrap_or("").is_empty() {
            "<none>"
        } else {
            "<set>"
        },
        settings.default_workspace.display(),
        settings.command_timeout_secs
    )
}

pub fn trusted_workspaces_path(root: &Path) -> PathBuf {
    root.join("trusted-workspaces.txt")
}

pub fn workspace_trust_prompt(workspace: &Path) -> String {
    format!(
        "Trust this workspace?\n{}\nOnly trusted workspaces can use interactive LLM prompts and command approvals. Type 'yes' to trust: ",
        workspace.display()
    )
}

pub fn recommended_prompts_output() -> &'static str {
    "Recommended prompts\n\
     - Summarize this project structure and suggest the next safe command.\n\
     - Inspect @workspace and propose a read-only diagnostic command.\n\
     - Explain @file:Cargo.toml and list important dependencies.\n\
     - Search the local wiki for prior command patterns before answering.\n"
}

pub fn expand_at_mentions(prompt: &str, workspace: &Path) -> Result<String> {
    let mut attachments = Vec::new();
    for token in prompt.split_whitespace() {
        if token == "@workspace" {
            attachments.push(format!("## @workspace\n{}", workspace.display()));
        } else if let Some(raw_path) = token.strip_prefix("@file:") {
            attachments.push(expand_file_mention(raw_path, workspace)?);
        }
    }

    if attachments.is_empty() {
        return Ok(prompt.to_string());
    }

    Ok(format!(
        "{prompt}\n\nAttached context:\n{}",
        attachments.join("\n\n")
    ))
}

pub fn is_workspace_trusted(trust_file: &Path, workspace: &Path) -> Result<bool> {
    if !trust_file.exists() {
        return Ok(false);
    }
    let workspace = normalized_workspace_path(workspace)?;
    let contents = fs::read_to_string(trust_file)?;
    Ok(contents.lines().any(|line| line.trim() == workspace))
}

pub fn trust_workspace(trust_file: &Path, workspace: &Path) -> Result<()> {
    if is_workspace_trusted(trust_file, workspace)? {
        return Ok(());
    }
    if let Some(parent) = trust_file.parent() {
        fs::create_dir_all(parent)?;
    }
    let workspace = normalized_workspace_path(workspace)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(trust_file)?;
    writeln!(file, "{workspace}")?;
    Ok(())
}

pub async fn handle_proposal_json(json: &str, settings: &AppSettings, yes: bool) -> Result<String> {
    let proposal = parse_command_proposal(json)?;
    handle_proposal(proposal, settings, yes).await
}

pub async fn handle_print_prompt(
    prompt: &str,
    settings: &AppSettings,
    wiki: Option<&WikiStore>,
    yes: bool,
) -> Result<String> {
    let messages = build_prompt_messages(prompt, wiki);
    let client = OpenAiClient::new(settings.clone());
    let output = client.stream_chat(&messages).await?.concat();
    match parse_cli_agent_output(&output).context("parse agent output")? {
        AgentOutput::Text(text) => Ok(format!("{text}\n")),
        AgentOutput::Command(proposal) => handle_proposal(proposal, settings, yes).await,
    }
}

pub fn build_prompt_messages(prompt: &str, wiki: Option<&WikiStore>) -> Vec<ChatMessage> {
    build_conversation_messages(prompt, &[], wiki)
}

pub fn build_conversation_messages(
    prompt: &str,
    history: &[ChatMessage],
    wiki: Option<&WikiStore>,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    if let Some(context) = wiki_context(prompt, wiki) {
        messages.push(ChatMessage::new(ChatRole::System, context));
    }
    messages.extend(history.iter().cloned());
    messages.push(ChatMessage::new(ChatRole::User, prompt.to_string()));
    messages
}

pub async fn doctor_output(settings: &AppSettings, settings_path: &Path) -> String {
    let connection = match OpenAiClient::new(settings.clone()).test_connection().await {
        Ok(()) => "Connection OK".to_string(),
        Err(error) => format!("Connection failed: {error}"),
    };
    format!(
        "CodeSmith doctor\nsettings: {}\nbase_url: {}\nmodel: {}\nworkspace: {}\ntimeout_secs: {}\n{}\n",
        settings_path.display(),
        settings.llm_base_url,
        settings.llm_model,
        settings.default_workspace.display(),
        settings.command_timeout_secs,
        connection
    )
}

pub fn wiki_list_output(wiki: &WikiStore) -> Result<String> {
    let pages = wiki.list_pages()?;
    if pages.is_empty() {
        return Ok("Saved wiki pages\nnone\n".to_string());
    }
    Ok(format!(
        "Saved wiki pages\n{}\n",
        pages
            .into_iter()
            .map(|page| format!("- {}", page.title))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

pub fn wiki_search_output(wiki: &WikiStore, query: &str) -> Result<String> {
    let pages = wiki.search(query, 5)?;
    if pages.is_empty() {
        return Ok(format!("Wiki search: {query}\nno matches\n"));
    }
    Ok(format!(
        "Wiki search: {query}\n{}\n",
        pages
            .into_iter()
            .map(|page| format!("- {}", page.title))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

pub async fn handle_proposal(
    proposal: CommandProposal,
    settings: &AppSettings,
    yes: bool,
) -> Result<String> {
    let decision = evaluate(&proposal, &settings.default_workspace);
    let mut output = format!(
        "Command proposal\ncommand: {}\ncwd: {}\nreason: {}\npolicy: {}\n",
        proposal.command,
        proposal.cwd.display(),
        proposal.reason,
        decision.reason
    );

    if !decision.allowed {
        output.push_str(&format!("blocked: {}\n", decision.reason));
        return Ok(output);
    }

    if !yes {
        output.push_str(approval_hint());
        output.push('\n');
        return Ok(output);
    }

    let run = run_approved_command(
        proposal,
        Duration::from_secs(settings.command_timeout_secs.max(1)),
    )
    .await?;
    output.push_str(&format!(
        "status: {:?}\nexit: {:?}\nstdout:\n{}\nstderr:\n{}\n",
        run.status, run.exit_code, run.stdout, run.stderr
    ));
    Ok(output)
}

fn parse_command_proposal(json: &str) -> Result<CommandProposal> {
    match parse_cli_agent_output(json).context("parse agent output")? {
        AgentOutput::Command(proposal) => Ok(proposal),
        AgentOutput::Text(_) => anyhow::bail!("input is not a strict command proposal JSON"),
    }
}

pub fn parse_cli_agent_output(input: &str) -> Result<AgentOutput> {
    let parsed = parse_agent_output(input).context("parse agent output")?;
    if !matches!(parsed, AgentOutput::Text(_)) {
        return Ok(parsed);
    }

    let Some(json) = fenced_json_body(input) else {
        return Ok(parsed);
    };
    parse_agent_output(json).context("parse fenced agent output")
}

fn wiki_context(prompt: &str, wiki: Option<&WikiStore>) -> Option<String> {
    let pages = wiki?.search(prompt, 5).ok()?;
    if pages.is_empty() {
        return None;
    }
    Some(format!(
        "Relevant local wiki pages:\n{}",
        pages
            .into_iter()
            .map(|page| format!("## {}\n{}", page.title, page.body))
            .collect::<Vec<_>>()
            .join("\n\n")
    ))
}

fn parse_parameterized_repl_command(trimmed: &str) -> ReplCommand {
    if let Some(query) = trimmed.strip_prefix("/wiki search ") {
        let query = query.trim();
        return if query.is_empty() {
            ReplCommand::Unknown(trimmed.to_string())
        } else {
            ReplCommand::WikiSearch(query.to_string())
        };
    }

    let Some(rest) = trimmed.strip_prefix("/set ") else {
        return ReplCommand::Unknown(trimmed.to_string());
    };
    let Some((key, value)) = rest.split_once(' ') else {
        return ReplCommand::Unknown(trimmed.to_string());
    };
    let value = value.trim();
    if value.is_empty() {
        return ReplCommand::Unknown(trimmed.to_string());
    }

    match key {
        "base-url" | "base_url" => ReplCommand::Set(SettingUpdate::BaseUrl(value.to_string())),
        "model" => ReplCommand::Set(SettingUpdate::Model(value.to_string())),
        "api-key" | "api_key" => {
            let value = if value.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(value.to_string())
            };
            ReplCommand::Set(SettingUpdate::ApiKey(value))
        }
        "workspace" => ReplCommand::Set(SettingUpdate::Workspace(PathBuf::from(value))),
        "timeout" | "timeout-secs" | "timeout_secs" => match value.parse::<u64>() {
            Ok(timeout) => ReplCommand::Set(SettingUpdate::TimeoutSecs(timeout)),
            Err(_) => ReplCommand::Unknown(trimmed.to_string()),
        },
        _ => ReplCommand::Unknown(trimmed.to_string()),
    }
}

fn ensure_non_empty(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{name} cannot be empty");
    }
    Ok(())
}

fn normalized_workspace_path(workspace: &Path) -> Result<String> {
    let path = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    Ok(path.display().to_string())
}

fn expand_file_mention(raw_path: &str, workspace: &Path) -> Result<String> {
    ensure_non_empty("file mention", raw_path)?;
    let workspace = workspace
        .canonicalize()
        .with_context(|| format!("canonicalize workspace {}", workspace.display()))?;
    let candidate = workspace.join(raw_path);
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("read @file:{}", raw_path))?;
    if !canonical.starts_with(&workspace) {
        anyhow::bail!("@file path is outside trusted workspace: {}", raw_path);
    }
    let mut contents = fs::read_to_string(&canonical)
        .with_context(|| format!("read @file:{}", canonical.display()))?;
    const MAX_ATTACHMENT_BYTES: usize = 12_000;
    if contents.len() > MAX_ATTACHMENT_BYTES {
        contents.truncate(MAX_ATTACHMENT_BYTES);
        contents.push_str("\n[truncated]");
    }
    Ok(format!("## @file:{}\n```text\n{}\n```", raw_path, contents))
}

fn fenced_json_body(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))?
        .trim_start();
    body.strip_suffix("```").map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesmith_core::AppSettings;
    use std::path::PathBuf;

    fn settings() -> AppSettings {
        AppSettings {
            llm_base_url: "http://localhost:11434/v1".to_string(),
            llm_model: "local".to_string(),
            api_key: None,
            default_workspace: PathBuf::from("/Users/gim-yonghyeon/CodeSmith"),
            command_timeout_secs: 5,
        }
    }

    #[test]
    fn approval_hint_mentions_yes_flag() {
        assert_eq!(
            approval_hint(),
            "approval required: rerun with --yes to execute this allowed command"
        );
    }

    #[tokio::test]
    async fn safe_proposal_without_yes_requires_approval() {
        let output = handle_proposal_json(
            r#"{"command":"printf cli-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test"}"#,
            &settings(),
            false,
        )
        .await
        .expect("proposal should be handled");

        assert!(output.contains("Command proposal"));
        assert!(output.contains(approval_hint()));
        assert!(!output.contains("stdout:"));
    }

    #[tokio::test]
    async fn blocked_proposal_never_runs() {
        let output = handle_proposal_json(
            r#"{"command":"rm -rf target","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test"}"#,
            &settings(),
            true,
        )
        .await
        .expect("blocked proposal should return output");

        assert!(output.contains("blocked:"));
        assert!(!output.contains("stdout:"));
    }

    #[tokio::test]
    async fn yes_runs_allowed_proposal_and_returns_stdout() {
        let output = handle_proposal_json(
            r#"{"command":"printf cli-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test"}"#,
            &settings(),
            true,
        )
        .await
        .expect("proposal should run");

        assert!(output.contains("status: Succeeded"));
        assert!(output.contains("stdout:\ncli-ok"));
    }

    #[test]
    fn prompt_messages_include_matching_wiki_context_before_user_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = codesmith_wiki::WikiStore::open(dir.path()).expect("open wiki");
        wiki.save_page("Command: printf cli-ok", "commands", "stdout cli-ok")
            .expect("save page");

        let messages = build_prompt_messages("cli-ok", Some(&wiki));

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, codesmith_core::ChatRole::System);
        assert!(messages[0].content.contains("Command: printf cli-ok"));
        assert_eq!(messages[1].role, codesmith_core::ChatRole::User);
        assert_eq!(messages[1].content, "cli-ok");
    }

    #[test]
    fn prompt_messages_omit_wiki_context_without_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = codesmith_wiki::WikiStore::open(dir.path()).expect("open wiki");
        wiki.save_page("Command: printf cli-ok", "commands", "stdout cli-ok")
            .expect("save page");

        let messages = build_prompt_messages("unmatched", Some(&wiki));

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, codesmith_core::ChatRole::User);
        assert_eq!(messages[0].content, "unmatched");
    }

    #[test]
    fn wiki_list_output_shows_saved_page_titles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = codesmith_wiki::WikiStore::open(dir.path()).expect("open wiki");
        wiki.save_page("Command: printf cli-ok", "commands", "stdout cli-ok")
            .expect("save page");

        let output = wiki_list_output(&wiki).expect("list wiki");

        assert!(output.contains("Saved wiki pages"));
        assert!(output.contains("Command: printf cli-ok"));
    }

    #[test]
    fn repl_parser_recognizes_slash_commands_and_prompts() {
        assert_eq!(parse_repl_line(""), ReplCommand::Empty);
        assert_eq!(parse_repl_line("/exit"), ReplCommand::Exit);
        assert_eq!(parse_repl_line("/quit"), ReplCommand::Exit);
        assert_eq!(parse_repl_line("/help"), ReplCommand::Help);
        assert_eq!(parse_repl_line("/prompts"), ReplCommand::Prompts);
        assert_eq!(parse_repl_line("/settings"), ReplCommand::Settings);
        assert_eq!(
            parse_repl_line("/wiki search cu-run-ok"),
            ReplCommand::WikiSearch("cu-run-ok".to_string())
        );
        assert_eq!(
            parse_repl_line("hello"),
            ReplCommand::Prompt("hello".to_string())
        );
    }

    #[test]
    fn repl_parser_recognizes_all_setting_updates() {
        assert_eq!(
            parse_repl_line("/set base-url http://localhost:11434/v1"),
            ReplCommand::Set(SettingUpdate::BaseUrl(
                "http://localhost:11434/v1".to_string()
            ))
        );
        assert_eq!(
            parse_repl_line("/set model gemma4:e4b-mlx-bf16"),
            ReplCommand::Set(SettingUpdate::Model("gemma4:e4b-mlx-bf16".to_string()))
        );
        assert_eq!(
            parse_repl_line("/set api-key none"),
            ReplCommand::Set(SettingUpdate::ApiKey(None))
        );
        assert_eq!(
            parse_repl_line("/set workspace /Users/gim-yonghyeon/CodeSmith"),
            ReplCommand::Set(SettingUpdate::Workspace(PathBuf::from(
                "/Users/gim-yonghyeon/CodeSmith"
            )))
        );
        assert_eq!(
            parse_repl_line("/set timeout 30"),
            ReplCommand::Set(SettingUpdate::TimeoutSecs(30))
        );
    }

    #[test]
    fn applying_setting_updates_mutates_settings() {
        let mut settings = settings();

        apply_setting_update(
            &mut settings,
            SettingUpdate::BaseUrl("http://localhost:1234/v1".to_string()),
        )
        .expect("base url update");
        apply_setting_update(
            &mut settings,
            SettingUpdate::Model("local-model".to_string()),
        )
        .expect("model update");
        apply_setting_update(
            &mut settings,
            SettingUpdate::ApiKey(Some("secret".to_string())),
        )
        .expect("api key update");
        apply_setting_update(
            &mut settings,
            SettingUpdate::Workspace(PathBuf::from("/Users/gim-yonghyeon/CodeSmith")),
        )
        .expect("workspace update");
        apply_setting_update(&mut settings, SettingUpdate::TimeoutSecs(45))
            .expect("timeout update");

        assert_eq!(settings.llm_base_url, "http://localhost:1234/v1");
        assert_eq!(settings.llm_model, "local-model");
        assert_eq!(settings.api_key, Some("secret".to_string()));
        assert_eq!(
            settings.default_workspace,
            PathBuf::from("/Users/gim-yonghyeon/CodeSmith")
        );
        assert_eq!(settings.command_timeout_secs, 45);
    }

    #[test]
    fn conversation_messages_include_history_before_current_prompt() {
        let history = vec![
            ChatMessage::new(ChatRole::User, "first".to_string()),
            ChatMessage::new(ChatRole::Assistant, "second".to_string()),
        ];

        let messages = build_conversation_messages("third", &history, None);

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "first");
        assert_eq!(messages[1].content, "second");
        assert_eq!(messages[2].content, "third");
    }

    #[test]
    fn cli_agent_output_accepts_fenced_json_proposal() {
        let output = "```json\n{\"command\":\"printf fenced\",\"cwd\":\"/Users/gim-yonghyeon/CodeSmith\",\"reason\":\"test\"}\n```";

        let parsed = parse_cli_agent_output(output).expect("fenced json should parse");

        match parsed {
            AgentOutput::Command(proposal) => {
                assert_eq!(proposal.command, "printf fenced");
                assert_eq!(proposal.reason, "test");
            }
            AgentOutput::Text(_) => panic!("expected command proposal"),
        }
    }

    #[test]
    fn trusted_workspace_file_records_and_checks_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let trust_file = dir.path().join("trusted-workspaces.txt");
        let workspace = dir.path().join("project");
        std::fs::create_dir_all(&workspace).expect("workspace");

        assert!(!is_workspace_trusted(&trust_file, &workspace).expect("read missing trust file"));

        trust_workspace(&trust_file, &workspace).expect("trust workspace");

        assert!(is_workspace_trusted(&trust_file, &workspace).expect("read trust file"));
    }

    #[test]
    fn at_mentions_expand_workspace_and_workspace_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("project");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::write(workspace.join("note.txt"), "hello mention").expect("write note");

        let expanded = expand_at_mentions("explain @workspace @file:note.txt", &workspace)
            .expect("expand mentions");

        assert!(expanded.contains("Attached context"));
        assert!(expanded.contains("## @workspace"));
        assert!(expanded.contains("## @file:note.txt"));
        assert!(expanded.contains("hello mention"));
    }

    #[test]
    fn at_file_mentions_cannot_escape_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("project");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::write(dir.path().join("secret.txt"), "outside").expect("write outside");

        let error = expand_at_mentions("read @file:../secret.txt", &workspace)
            .expect_err("outside file should be blocked");

        assert!(error.to_string().contains("outside trusted workspace"));
    }
}
