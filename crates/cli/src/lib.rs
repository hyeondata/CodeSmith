use anyhow::{Context, Result};
use codesmith_agent::{AgentOutput, parse_agent_output};
use codesmith_core::{
    AppSettings, BackendKind, ChatMessage, ChatRole, CommandProposal, CommandRun, CommandStatus,
    IngestJob, ModelProfile, PolicyDecision, SourceStatus, default_system_prompt,
};
use codesmith_llm::OpenAiClient;
use codesmith_policy::evaluate;
use codesmith_runner::run_approved_command;
use codesmith_storage::Storage;
use codesmith_wiki::WikiStore;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn approval_hint() -> &'static str {
    "approval required: rerun with --yes to execute this allowed command"
}

pub fn read_required_approval<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
) -> std::io::Result<bool> {
    loop {
        write!(writer, "Approve? type y or n: ")?;
        writer.flush()?;

        let mut answer = String::new();
        if reader.read_line(&mut answer)? == 0 {
            writeln!(writer, "rejected")?;
            return Ok(false);
        }

        match answer.trim() {
            "y" | "Y" | "yes" | "YES" => return Ok(true),
            "n" | "N" | "no" | "NO" => return Ok(false),
            _ => {
                writeln!(writer, "Please type y or n.")?;
            }
        }
    }
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
     /models                   list model profiles\n\
     /model use <id>           switch active model profile\n\
     /model show               show active model profile\n\
     /prompts                  show recommended prompts\n\
     /doctor                   test local LLM connection\n\
     /ingest file <path>       snapshot a trusted workspace file into raw/ and wiki\n\
     /ingest folder <path>     recursively ingest supported source files\n\
     /query <question>         build local wiki context for a question\n\
     /lint wiki                check frontmatter, wikilinks, and duplicate titles\n\
     /log recent               show recent operation log entries\n\
     /sources                  list ingested source records\n\
     /plan <goal>              shape goal, scope, risks, and verification before acting\n\
     /debug <symptom>          use root-cause debugging before proposing fixes\n\
     /verify                   summarize command evidence before completion claims\n\
     /review                   review recent run risks and missing checks\n\
     /wiki list                list saved wiki pages\n\
     /wiki search <query>      search local wiki\n\
     /tools                    show available tools and approval policy\n\
     /runs                     show command runs from this chat\n\
     /last                     show the last command result\n\
     /retry                    retry the last command proposal\n\
     /clear                    clear in-memory chat history\n\
     /exit                     quit\n"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplCommand {
    Empty,
    Help,
    Prompts,
    Settings,
    Set(SettingUpdate),
    Models,
    ModelUse(String),
    ModelShow,
    Doctor,
    IngestFile(PathBuf),
    IngestFolder(PathBuf),
    Query(String),
    LintWiki,
    LogRecent,
    Sources,
    Plan(String),
    Debug(String),
    Verify,
    Review,
    Tools,
    Runs,
    Last,
    Retry,
    Clear,
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
        "/models" | "/models list" => ReplCommand::Models,
        "/model show" => ReplCommand::ModelShow,
        "/doctor" => ReplCommand::Doctor,
        "/wiki list" => ReplCommand::WikiList,
        "/lint wiki" => ReplCommand::LintWiki,
        "/log recent" => ReplCommand::LogRecent,
        "/sources" | "/sources list" => ReplCommand::Sources,
        "/verify" => ReplCommand::Verify,
        "/review" => ReplCommand::Review,
        "/tools" => ReplCommand::Tools,
        "/runs" => ReplCommand::Runs,
        "/last" => ReplCommand::Last,
        "/retry" => ReplCommand::Retry,
        "/clear" => ReplCommand::Clear,
        _ => parse_parameterized_repl_command(trimmed),
    }
}

pub fn tools_output() -> &'static str {
    "CodeSmith tools\n\
     - shell command proposals: approval required every time\n\
     - runner: streams stdout/stderr and records exit status\n\
     - policy: blocks destructive, privileged, credential, exfiltration, and outside-workspace commands\n\
     - wiki: ingest, query, lint, sources, and operation log\n\
     - workflow: plan before implementing, debug before fixing, verify before completion claims\n\
     - diagnostics: prefer small read-only commands before mutating commands\n\
     - model profiles: Ollama, vLLM, LiteLLM, and OpenAI-compatible local endpoints\n"
}

pub fn apply_setting_update(settings: &mut AppSettings, update: SettingUpdate) -> Result<String> {
    settings.ensure_model_profiles();
    let message = match update {
        SettingUpdate::BaseUrl(value) => {
            ensure_non_empty("base-url", &value)?;
            if let Some(profile) = settings.active_model_profile_mut() {
                profile.base_url = value;
            }
            settings.ensure_model_profiles();
            "base-url updated".to_string()
        }
        SettingUpdate::Model(value) => {
            ensure_non_empty("model", &value)?;
            if let Some(profile) = settings.active_model_profile_mut() {
                profile.model = value;
            }
            settings.ensure_model_profiles();
            "model updated".to_string()
        }
        SettingUpdate::ApiKey(value) => {
            let value = value.filter(|key| !key.trim().is_empty());
            if let Some(profile) = settings.active_model_profile_mut() {
                profile.api_key = value;
            }
            settings.ensure_model_profiles();
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
    let mut settings = settings.clone();
    settings.ensure_model_profiles();
    let profile = settings.active_model_profile();
    format!(
        "Settings\npath: {}\nactive-profile: {}\nbackend: {}\nbase-url: {}\nmodel: {}\napi-key: {}\nprompt: {}\nworkspace: {}\ntimeout: {}s\n",
        settings_path.display(),
        settings.active_profile,
        profile
            .map(|profile| profile.backend_kind.as_str())
            .unwrap_or("missing"),
        settings.llm_base_url,
        settings.llm_model,
        if settings.api_key.as_deref().unwrap_or("").is_empty() {
            "<none>"
        } else {
            "<set>"
        },
        if profile
            .map(|profile| profile.system_prompt != default_system_prompt())
            .unwrap_or(false)
        {
            "custom"
        } else {
            "default"
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

pub fn plan_workflow_prompt(goal: &str) -> String {
    format!(
        "Create a concise implementation plan for this goal before proposing any command: {goal}\n\
         Include: goal, scope, success criteria, risks, smallest safe first steps, and verification commands. \
         Do not claim anything is complete. If a command is needed, prefer a read-only diagnostic command proposal."
    )
}

pub fn debug_workflow_prompt(symptom: &str) -> String {
    format!(
        "Use systematic debugging for this symptom: {symptom}\n\
         Include: reproduction steps, observed evidence, likely root cause hypotheses, one minimal test, \
         and the next safest diagnostic. Do not propose a fix until evidence supports the root cause."
    )
}

pub fn verification_output(runs: &[CommandRun]) -> String {
    let mut output = String::from("Verification\n");
    if runs.is_empty() {
        output.push_str("evidence: none\nstatus: not verified\n");
        output.push_str("next: run a safe diagnostic or test command before claiming completion\n");
        return output;
    }
    for (index, run) in runs.iter().enumerate() {
        output.push_str(&format!(
            "{}. {:?} exit {:?} command: {}\n",
            index + 1,
            run.status,
            run.exit_code,
            compact_command(&run.proposal.command)
        ));
        if !run.stdout.trim().is_empty() {
            output.push_str(&format!("   stdout: {}\n", first_line(&run.stdout)));
        }
        if !run.stderr.trim().is_empty() {
            output.push_str(&format!("   stderr: {}\n", first_line(&run.stderr)));
        }
    }
    let last = runs.last().expect("runs is not empty");
    match last.status {
        CommandStatus::Succeeded => {
            output.push_str("status: verified by latest successful command output\n");
        }
        CommandStatus::Failed | CommandStatus::TimedOut | CommandStatus::Blocked => {
            output.push_str("status: not complete; latest evidence is failing or blocked\n");
            output.push_str("next: inspect stderr/policy reason and run a smaller diagnostic\n");
        }
        CommandStatus::Rejected | CommandStatus::PendingApproval | CommandStatus::Running => {
            output.push_str("status: not verified; command did not complete successfully\n");
        }
        CommandStatus::Cancelled => {
            output.push_str("status: not verified; command was cancelled\n");
        }
    }
    output
}

pub fn review_output(runs: &[CommandRun]) -> String {
    let mut output = String::from("Review\n");
    if runs.is_empty() {
        output.push_str("- no command evidence yet\n");
        output.push_str("- risk: completion claims would be unsupported\n");
        return output;
    }
    let failed = runs
        .iter()
        .filter(|run| {
            matches!(
                run.status,
                CommandStatus::Failed | CommandStatus::TimedOut | CommandStatus::Blocked
            )
        })
        .count();
    output.push_str(&format!("- command runs reviewed: {}\n", runs.len()));
    output.push_str(&format!("- failed/blocked evidence: {failed}\n"));
    if failed > 0 {
        output.push_str(
            "- next safe diagnostic: read stderr or policy reason before proposing a fix\n",
        );
    } else {
        output.push_str(
            "- next safe diagnostic: run the relevant test/check before completion claims\n",
        );
    }
    output
}

pub fn add_local_model_profile(
    settings: &mut AppSettings,
    id: &str,
    backend: BackendKind,
    base_url: &str,
    model: &str,
    name: Option<&str>,
) -> Result<String> {
    ensure_non_empty("id", id)?;
    ensure_non_empty("base-url", base_url)?;
    ensure_non_empty("model", model)?;
    settings.ensure_model_profiles();
    let profile = ModelProfile {
        id: id.to_string(),
        name: name.unwrap_or(model).to_string(),
        backend_kind: backend,
        base_url: base_url.to_string(),
        model: model.to_string(),
        api_key: None,
        system_prompt: prompt_for_model(model),
        temperature: None,
        context_hint: Some(format!("{} local profile", backend.as_str())),
    };
    if let Some(existing) = settings
        .model_profiles
        .iter_mut()
        .find(|profile| profile.id == id)
    {
        *existing = profile;
    } else {
        settings.model_profiles.push(profile);
    }
    Ok(format!("model profile added: {id}\n"))
}

pub fn use_model_profile(settings: &mut AppSettings, id: &str) -> Result<String> {
    settings.ensure_model_profiles();
    if !settings
        .model_profiles
        .iter()
        .any(|profile| profile.id == id)
    {
        anyhow::bail!("model profile '{id}' was not found");
    }
    settings.active_profile = id.to_string();
    settings.ensure_model_profiles();
    Ok(format!("active model profile: {id}\n"))
}

pub fn model_profiles_output(settings: &AppSettings) -> String {
    let mut settings = settings.clone();
    settings.ensure_model_profiles();
    let mut output = String::from("Model profiles\n");
    for profile in &settings.model_profiles {
        let marker = if profile.id == settings.active_profile {
            "*"
        } else {
            "-"
        };
        output.push_str(&format!(
            "{marker} {} [{}] {} @ {}\n",
            profile.id,
            profile.backend_kind.as_str(),
            profile.model,
            profile.base_url
        ));
    }
    output
}

pub fn active_model_profile_output(settings: &AppSettings) -> String {
    let mut settings = settings.clone();
    settings.ensure_model_profiles();
    let Some(profile) = settings.active_model_profile() else {
        return "Active model profile\nmissing\n".to_string();
    };
    format!(
        "Active model profile\nid: {}\nname: {}\nbackend: {}\nbase-url: {}\nmodel: {}\napi-key: {}\ntemperature: {}\nprompt: {}\n",
        profile.id,
        profile.name,
        profile.backend_kind.as_str(),
        profile.base_url,
        profile.model,
        if profile.api_key.as_deref().unwrap_or("").is_empty() {
            "<none>"
        } else {
            "<set>"
        },
        profile
            .temperature
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<default>".to_string()),
        if profile.system_prompt == default_system_prompt() {
            "default"
        } else {
            "custom"
        }
    )
}

pub fn parse_backend_kind(input: &str) -> Result<BackendKind> {
    match input.trim().to_ascii_lowercase().as_str() {
        "ollama" => Ok(BackendKind::Ollama),
        "vllm" => Ok(BackendKind::Vllm),
        "litellm" => Ok(BackendKind::Litellm),
        "openai_compatible" | "openai-compatible" | "custom" => Ok(BackendKind::OpenAiCompatible),
        other => anyhow::bail!("unknown backend kind: {other}"),
    }
}

pub fn prompt_for_model(model: &str) -> String {
    if model == "gag0/qwen35-opus-distil:27b" {
        return "You are CodeSmith running on gag0/qwen35-opus-distil:27b. Maximize correctness by being concise, explicit, and conservative. CodeSmith is execution-only: never imply that a command ran unless tool output proves it. If a shell command is needed, return one strict JSON object only with command, cwd, and reason fields. Do not use Markdown fences, comments, trailing prose, or extra keys around command proposal JSON. Prefer commands that write only inside the configured workspace, and keep debugging steps observable through stdout/stderr."
            .to_string();
    }
    default_system_prompt()
}

pub fn ingest_file_output(
    wiki: &WikiStore,
    storage: &Storage,
    workspace: &Path,
    path: &Path,
) -> Result<String> {
    let result = wiki.ingest_file(workspace, path)?;
    storage.insert_source_record(&result.record)?;
    storage.insert_ingest_job(&IngestJob {
        id: uuid::Uuid::new_v4(),
        source_id: result.record.id,
        status: if result.skipped {
            SourceStatus::Skipped
        } else {
            SourceStatus::Active
        },
        analysis_path: Some(result.raw_path.clone()),
        error: None,
    })?;
    Ok(format!(
        "Ingest file\npath: {}\nhash: {}\nstatus: {}\n",
        result.record.path.display(),
        result.record.hash,
        if result.skipped {
            "skipped"
        } else {
            "ingested"
        }
    ))
}

pub fn ingest_folder_output(
    wiki: &WikiStore,
    storage: &Storage,
    workspace: &Path,
    folder: &Path,
) -> Result<String> {
    let folder = workspace.join(folder).canonicalize()?;
    if !folder.starts_with(workspace.canonicalize()?) {
        anyhow::bail!(
            "folder path is outside trusted workspace: {}",
            folder.display()
        );
    }
    let mut ingested = 0;
    let mut skipped = 0;
    for file in collect_ingestable_files(&folder)? {
        match wiki.ingest_file(workspace, &file) {
            Ok(result) => {
                storage.insert_source_record(&result.record)?;
                storage.insert_ingest_job(&IngestJob {
                    id: uuid::Uuid::new_v4(),
                    source_id: result.record.id,
                    status: if result.skipped {
                        SourceStatus::Skipped
                    } else {
                        SourceStatus::Active
                    },
                    analysis_path: Some(result.raw_path.clone()),
                    error: None,
                })?;
                if result.skipped {
                    skipped += 1;
                } else {
                    ingested += 1;
                }
            }
            Err(error) if error.to_string().contains("unsupported source file type") => {}
            Err(error) => return Err(error),
        }
    }
    Ok(format!(
        "Ingest folder\npath: {}\ningested: {}\nskipped: {}\n",
        folder.display(),
        ingested,
        skipped
    ))
}

pub fn query_output(wiki: &WikiStore, query: &str) -> Result<String> {
    let context = wiki.query_context(query, 4000)?;
    if context.trim().is_empty() {
        return Ok(format!("Query context: {query}\nno context\n"));
    }
    Ok(format!("Query context: {query}\n{context}\n"))
}

pub fn lint_wiki_output(wiki: &WikiStore) -> Result<String> {
    let issues = wiki.lint_wiki()?;
    if issues.is_empty() {
        return Ok("Wiki lint\nno issues\n".to_string());
    }
    Ok(format!(
        "Wiki lint\n{}\n",
        issues
            .into_iter()
            .map(|issue| format!(
                "- {}: {} ({})",
                issue.kind,
                issue.message,
                issue.path.display()
            ))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

pub fn log_recent_output(root: &Path) -> Result<String> {
    let path = root.join("log.md");
    if !path.exists() {
        return Ok("Recent log\nnone\n".to_string());
    }
    let raw = fs::read_to_string(path)?;
    let lines = raw.lines().rev().take(20).collect::<Vec<_>>();
    Ok(format!(
        "Recent log\n{}\n",
        lines.into_iter().rev().collect::<Vec<_>>().join("\n")
    ))
}

pub fn sources_output(storage: &Storage) -> Result<String> {
    let sources = storage.list_source_records()?;
    if sources.is_empty() {
        return Ok("Sources\nnone\n".to_string());
    }
    Ok(format!(
        "Sources\n{}\n",
        sources
            .into_iter()
            .map(|source| format!("- {} {}", source.hash, source.path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
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
    let messages = build_prompt_messages(prompt, settings, wiki);
    let client = OpenAiClient::new(settings.clone());
    let output = client.stream_chat(&messages).await?.concat();
    match parse_cli_agent_output(&output).context("parse agent output")? {
        AgentOutput::Text(text) => Ok(format!("{text}\n")),
        AgentOutput::Command(proposal) => handle_proposal(proposal, settings, yes).await,
    }
}

pub fn build_prompt_messages(
    prompt: &str,
    settings: &AppSettings,
    wiki: Option<&WikiStore>,
) -> Vec<ChatMessage> {
    build_conversation_messages(prompt, &[], settings, wiki)
}

pub fn build_conversation_messages(
    prompt: &str,
    history: &[ChatMessage],
    settings: &AppSettings,
    wiki: Option<&WikiStore>,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    if let Some(profile) = settings.active_model_profile() {
        messages.push(ChatMessage::new(
            ChatRole::System,
            profile.system_prompt.clone(),
        ));
    }
    if let Some(context) = wiki_context(prompt, wiki) {
        messages.push(ChatMessage::new(ChatRole::System, context));
    }
    messages.extend(history.iter().cloned());
    messages.push(ChatMessage::new(ChatRole::User, prompt.to_string()));
    messages
}

pub async fn doctor_output(settings: &AppSettings, settings_path: &Path) -> String {
    let mut settings = settings.clone();
    settings.ensure_model_profiles();
    let connection = match OpenAiClient::new(settings.clone()).test_connection().await {
        Ok(()) => "Connection OK".to_string(),
        Err(error) => format!("Connection failed: {error}"),
    };
    format!(
        "CodeSmith doctor\nsettings: {}\nprofile: {}\nbackend: {}\nbase_url: {}\nmodel: {}\nworkspace: {}\ntimeout_secs: {}\n{}\n",
        settings_path.display(),
        settings.active_profile,
        settings
            .active_model_profile()
            .map(|profile| profile.backend_kind.as_str())
            .unwrap_or("missing"),
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
            .map(|page| format!("- [{}] {}", page.domain, page.title))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

pub async fn handle_proposal(
    mut proposal: CommandProposal,
    settings: &AppSettings,
    yes: bool,
) -> Result<String> {
    proposal.cwd = resolve_proposal_cwd(&proposal.cwd, &settings.default_workspace);
    let mut output = preview_proposal(proposal.clone(), settings, !yes);
    let decision = evaluate(&proposal, &settings.default_workspace);

    if !decision.allowed {
        return Ok(output);
    }

    if !yes {
        return Ok(output);
    }

    let run = run_approved_proposal(proposal, settings).await?;
    output.push_str(&format_command_run(&run));
    Ok(output)
}

pub fn preview_proposal(
    mut proposal: CommandProposal,
    settings: &AppSettings,
    include_approval_hint: bool,
) -> String {
    proposal.cwd = resolve_proposal_cwd(&proposal.cwd, &settings.default_workspace);
    let decision = evaluate(&proposal, &settings.default_workspace);
    let mut output = format!(
        "Command proposal\ncommand: {}\ncwd: {}\nreason: {}\npolicy: {}\nrisk: {:?}\napproval: {}\n",
        proposal.command,
        proposal.cwd.display(),
        proposal.reason,
        decision.reason,
        decision.risk_level,
        if decision.requires_approval {
            "required"
        } else {
            "not required"
        }
    );
    if !decision.allowed {
        output.push_str(&format!("blocked: {}\n", decision.reason));
    } else if include_approval_hint {
        output.push_str(approval_hint());
        output.push('\n');
    }
    output
}

pub fn policy_decision_for_proposal(
    mut proposal: CommandProposal,
    settings: &AppSettings,
) -> (CommandProposal, PolicyDecision) {
    proposal.cwd = resolve_proposal_cwd(&proposal.cwd, &settings.default_workspace);
    let decision = evaluate(&proposal, &settings.default_workspace);
    (proposal, decision)
}

pub async fn run_approved_proposal(
    mut proposal: CommandProposal,
    settings: &AppSettings,
) -> Result<CommandRun> {
    proposal.cwd = resolve_proposal_cwd(&proposal.cwd, &settings.default_workspace);
    let decision = evaluate(&proposal, &settings.default_workspace);
    if !decision.allowed {
        return Ok(CommandRun::new(
            proposal,
            CommandStatus::Blocked,
            String::new(),
            decision.reason,
            None,
        ));
    }

    run_approved_command(
        proposal,
        Duration::from_secs(settings.command_timeout_secs.max(1)),
    )
    .await
}

pub fn format_command_run(run: &CommandRun) -> String {
    format!(
        "status: {:?}\nexit: {:?}\nstdout:\n{}\nstderr:\n{}\n",
        run.status, run.exit_code, run.stdout, run.stderr
    )
}

pub fn save_command_run_evidence(wiki: &WikiStore, run: &CommandRun, root_cause_note: &str) {
    let domain = match run.status {
        CommandStatus::Succeeded => "command",
        CommandStatus::Failed | CommandStatus::TimedOut | CommandStatus::Blocked => "debugging",
        _ => "verification",
    };
    let title = format!(
        "Command run: {:?} {} ({})",
        run.status,
        compact_command(&run.proposal.command),
        run.id
    );
    let body = format!(
        "Command: `{}`\n\nCwd: `{}`\n\nStatus: `{:?}`\n\nExit: `{:?}`\n\nRoot cause note: {}\n\nStdout:\n```text\n{}\n```\n\nStderr:\n```text\n{}\n```",
        run.proposal.command,
        run.proposal.cwd.display(),
        run.status,
        run.exit_code,
        root_cause_note,
        run.stdout,
        run.stderr
    );
    let _ = wiki.save_page(&title, domain, &body);
}

fn resolve_proposal_cwd(cwd: &Path, workspace: &Path) -> PathBuf {
    if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        workspace.join(cwd)
    }
}

fn compact_command(command: &str) -> String {
    const LIMIT: usize = 80;
    if command.chars().count() <= LIMIT {
        return command.to_string();
    }
    let mut compact = command.chars().take(LIMIT - 1).collect::<String>();
    compact.push('…');
    compact
}

fn first_line(value: &str) -> String {
    value.lines().next().unwrap_or_default().to_string()
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
    let context = wiki?.query_context(prompt, 4000).ok()?;
    if context.trim().is_empty() {
        return None;
    }
    Some(format!("Relevant local wiki context:\n{context}"))
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
    if let Some(path) = trimmed.strip_prefix("/ingest file ") {
        let path = path.trim();
        return if path.is_empty() {
            ReplCommand::Unknown(trimmed.to_string())
        } else {
            ReplCommand::IngestFile(PathBuf::from(path))
        };
    }
    if let Some(path) = trimmed.strip_prefix("/ingest folder ") {
        let path = path.trim();
        return if path.is_empty() {
            ReplCommand::Unknown(trimmed.to_string())
        } else {
            ReplCommand::IngestFolder(PathBuf::from(path))
        };
    }
    if let Some(question) = trimmed.strip_prefix("/query ") {
        let question = question.trim();
        return if question.is_empty() {
            ReplCommand::Unknown(trimmed.to_string())
        } else {
            ReplCommand::Query(question.to_string())
        };
    }
    if let Some(goal) = trimmed.strip_prefix("/plan ") {
        let goal = goal.trim();
        return if goal.is_empty() {
            ReplCommand::Unknown(trimmed.to_string())
        } else {
            ReplCommand::Plan(goal.to_string())
        };
    }
    if let Some(symptom) = trimmed.strip_prefix("/debug ") {
        let symptom = symptom.trim();
        return if symptom.is_empty() {
            ReplCommand::Unknown(trimmed.to_string())
        } else {
            ReplCommand::Debug(symptom.to_string())
        };
    }
    if let Some(id) = trimmed.strip_prefix("/model use ") {
        let id = id.trim();
        return if id.is_empty() {
            ReplCommand::Unknown(trimmed.to_string())
        } else {
            ReplCommand::ModelUse(id.to_string())
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

fn collect_ingestable_files(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_ingestable_files_into(folder, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_ingestable_files_into(folder: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let skip_dirs = ["target", ".git", ".worktrees", ".playwright-mcp"];
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if name.starts_with('.') || skip_dirs.contains(&name) {
                continue;
            }
            collect_ingestable_files_into(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
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
        let mut settings = AppSettings {
            llm_base_url: "http://localhost:11434/v1".to_string(),
            llm_model: "local".to_string(),
            api_key: None,
            default_workspace: PathBuf::from("/Users/gim-yonghyeon/CodeSmith"),
            command_timeout_secs: 5,
            ..Default::default()
        };
        settings.model_profiles = vec![ModelProfile::from_legacy(
            "default",
            settings.llm_base_url.clone(),
            settings.llm_model.clone(),
            settings.api_key.clone(),
        )];
        settings.ensure_model_profiles();
        settings
    }

    #[test]
    fn approval_hint_mentions_yes_flag() {
        assert_eq!(
            approval_hint(),
            "approval required: rerun with --yes to execute this allowed command"
        );
    }

    #[test]
    fn approval_prompt_requires_explicit_yes_or_no() {
        let input = b"\nmaybe\ny\n";
        let mut output = Vec::new();

        let approved = read_required_approval(&input[..], &mut output)
            .expect("approval prompt should read a valid answer");

        assert!(approved);
        let output = String::from_utf8(output).expect("prompt output should be utf-8");
        assert!(output.contains("Approve? type y or n: "));
        assert_eq!(
            output.matches("Please type y or n.").count(),
            2,
            "blank and invalid input should both be rejected with a retry"
        );
    }

    #[test]
    fn approval_prompt_accepts_explicit_no() {
        let mut output = Vec::new();

        let approved =
            read_required_approval(&b"n\n"[..], &mut output).expect("explicit no should parse");

        assert!(!approved);
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

    #[tokio::test]
    async fn relative_proposal_cwd_resolves_inside_workspace() {
        let output = handle_proposal_json(
            r#"{"command":"printf relative-ok","cwd":".","reason":"test"}"#,
            &settings(),
            true,
        )
        .await
        .expect("relative cwd should resolve inside workspace");

        assert!(output.contains("cwd: /Users/gim-yonghyeon/CodeSmith"));
        assert!(output.contains("status: Succeeded"));
        assert!(output.contains("stdout:\nrelative-ok"));
    }

    #[test]
    fn prompt_messages_include_matching_wiki_context_before_user_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = codesmith_wiki::WikiStore::open(dir.path()).expect("open wiki");
        wiki.save_page("Command: printf cli-ok", "commands", "stdout cli-ok")
            .expect("save page");

        let settings = settings();
        let messages = build_prompt_messages("cli-ok", &settings, Some(&wiki));

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, codesmith_core::ChatRole::System);
        assert!(messages[0].content.contains("execution-only"));
        assert_eq!(messages[1].role, codesmith_core::ChatRole::System);
        assert!(messages[1].content.contains("Command: printf cli-ok"));
        assert_eq!(messages[2].role, codesmith_core::ChatRole::User);
        assert_eq!(messages[2].content, "cli-ok");
    }

    #[test]
    fn prompt_messages_include_index_even_without_page_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = codesmith_wiki::WikiStore::open(dir.path()).expect("open wiki");
        wiki.save_page("Command: printf cli-ok", "commands", "stdout cli-ok")
            .expect("save page");

        let settings = settings();
        let messages = build_prompt_messages("unmatched", &settings, Some(&wiki));

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, codesmith_core::ChatRole::System);
        assert!(messages[0].content.contains("execution-only"));
        assert_eq!(messages[1].role, codesmith_core::ChatRole::System);
        assert!(messages[1].content.contains("Relevant local wiki context"));
        assert!(messages[1].content.contains("# CodeSmith Wiki Index"));
        assert_eq!(messages[2].role, codesmith_core::ChatRole::User);
        assert_eq!(messages[2].content, "unmatched");
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
    fn wiki_search_output_shows_page_domain_for_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = codesmith_wiki::WikiStore::open(dir.path()).expect("open wiki");
        wiki.save_page(
            "Command run: Failed python",
            "debugging",
            "stderr permission denied",
        )
        .expect("save page");

        let output = wiki_search_output(&wiki, "permission denied").expect("search wiki");

        assert!(output.contains("- [debugging] Command run: Failed python"));
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
            parse_repl_line("/ingest file Cargo.toml"),
            ReplCommand::IngestFile(PathBuf::from("Cargo.toml"))
        );
        assert_eq!(
            parse_repl_line("/ingest folder crates"),
            ReplCommand::IngestFolder(PathBuf::from("crates"))
        );
        assert_eq!(
            parse_repl_line("/query cargo test"),
            ReplCommand::Query("cargo test".to_string())
        );
        assert_eq!(parse_repl_line("/lint wiki"), ReplCommand::LintWiki);
        assert_eq!(parse_repl_line("/log recent"), ReplCommand::LogRecent);
        assert_eq!(parse_repl_line("/sources"), ReplCommand::Sources);
        assert_eq!(
            parse_repl_line("/plan add safer tool workflow"),
            ReplCommand::Plan("add safer tool workflow".to_string())
        );
        assert_eq!(
            parse_repl_line("/debug python SyntaxError"),
            ReplCommand::Debug("python SyntaxError".to_string())
        );
        assert_eq!(parse_repl_line("/verify"), ReplCommand::Verify);
        assert_eq!(parse_repl_line("/review"), ReplCommand::Review);
        assert_eq!(parse_repl_line("/tools"), ReplCommand::Tools);
        assert_eq!(parse_repl_line("/runs"), ReplCommand::Runs);
        assert_eq!(parse_repl_line("/last"), ReplCommand::Last);
        assert_eq!(parse_repl_line("/retry"), ReplCommand::Retry);
        assert_eq!(parse_repl_line("/clear"), ReplCommand::Clear);
        assert_eq!(parse_repl_line("/models"), ReplCommand::Models);
        assert_eq!(parse_repl_line("/model show"), ReplCommand::ModelShow);
        assert_eq!(
            parse_repl_line("/model use qwen35-opus"),
            ReplCommand::ModelUse("qwen35-opus".to_string())
        );
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

        let settings = settings();
        let messages = build_conversation_messages("third", &history, &settings, None);

        assert_eq!(messages.len(), 4);
        assert!(messages[0].content.contains("execution-only"));
        assert_eq!(messages[1].content, "first");
        assert_eq!(messages[2].content, "second");
        assert_eq!(messages[3].content, "third");
    }

    #[test]
    fn default_prompt_contains_superpowers_workflow_policy() {
        let messages = build_conversation_messages("debug this", &[], &settings(), None);

        let system = &messages[0].content;
        assert!(system.contains("intent before action"));
        assert!(system.contains("systematic debugging"));
        assert!(system.contains("verify before declaring success"));
    }

    #[test]
    fn tools_output_includes_superpowers_style_tool_policy() {
        let output = tools_output();

        assert!(output.contains("plan before implementing"));
        assert!(output.contains("debug before fixing"));
        assert!(output.contains("verify before completion claims"));
        assert!(output.contains("read-only commands before mutating commands"));
    }

    #[test]
    fn workflow_prompts_encode_plan_and_debug_shapes() {
        let plan = plan_workflow_prompt("add a feature");
        let debug = debug_workflow_prompt("python SyntaxError");

        assert!(plan.contains("success criteria"));
        assert!(plan.contains("verification commands"));
        assert!(debug.contains("reproduction steps"));
        assert!(debug.contains("root cause"));
        assert!(debug.contains("next safest diagnostic"));
    }

    #[test]
    fn verification_and_review_output_use_command_run_evidence() {
        let failed = CommandRun::new(
            CommandProposal::new(
                "python3 broken.py",
                PathBuf::from("/Users/gim-yonghyeon/CodeSmith"),
                "test stderr",
            ),
            CommandStatus::Failed,
            String::new(),
            "SyntaxError: invalid syntax".to_string(),
            Some(1),
        );
        let verified = verification_output(std::slice::from_ref(&failed));
        let reviewed = review_output(&[failed]);

        assert!(verified.contains("SyntaxError"));
        assert!(verified.contains("not complete"));
        assert!(reviewed.contains("failed/blocked evidence: 1"));
        assert!(reviewed.contains("read stderr"));
    }

    #[test]
    fn command_run_evidence_is_saved_as_debugging_wiki_page() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wiki = codesmith_wiki::WikiStore::open(dir.path()).expect("open wiki");
        let failed = CommandRun::new(
            CommandProposal::new(
                "python3 broken.py",
                PathBuf::from("/Users/gim-yonghyeon/CodeSmith"),
                "test stderr",
            ),
            CommandStatus::Failed,
            String::new(),
            "SyntaxError: invalid syntax".to_string(),
            Some(1),
        );

        save_command_run_evidence(&wiki, &failed, "stderr shows invalid syntax");
        let pages = wiki.search("SyntaxError", 5).expect("search evidence");

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].domain, "debugging");
        assert!(pages[0].body.contains("stderr shows invalid syntax"));
    }

    #[test]
    fn model_profile_helpers_add_switch_and_show_profiles() {
        let mut settings = settings();

        let added = add_local_model_profile(
            &mut settings,
            "qwen35-opus",
            BackendKind::Ollama,
            "http://localhost:11434/v1",
            "gag0/qwen35-opus-distil:27b",
            Some("Qwen 35 Opus Distil"),
        )
        .expect("add profile");
        let switched = use_model_profile(&mut settings, "qwen35-opus").expect("switch profile");
        let profiles = model_profiles_output(&settings);
        let active = active_model_profile_output(&settings);

        assert!(added.contains("qwen35-opus"));
        assert!(switched.contains("qwen35-opus"));
        assert!(profiles.contains("* qwen35-opus"));
        assert!(active.contains("gag0/qwen35-opus-distil:27b"));
        assert_eq!(settings.llm_model, "gag0/qwen35-opus-distil:27b");
        assert!(
            settings
                .active_model_profile()
                .expect("active profile")
                .system_prompt
                .contains("Do not use Markdown fences")
        );
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
