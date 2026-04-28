use anyhow::Result;
use clap::{Parser, Subcommand};
use codesmith_agent::AgentOutput;
use codesmith_core::{ChatMessage, ChatRole, CommandProposal, CommandRun, CommandStatus};
use codesmith_llm::OpenAiClient;
use codesmith_storage::{Storage, load_settings, save_settings, settings_path};
use codesmith_wiki::WikiStore;
use rustyline::{DefaultEditor, error::ReadlineError};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "codesmith-cli")]
#[command(about = "CodeSmith local execution agent CLI")]
struct Cli {
    #[arg(short = 'p', long = "print")]
    prompt: Option<String>,

    #[arg(long)]
    yes: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Start interactive CLI chat")]
    Chat,
    #[command(about = "Test local LLM connection and settings")]
    Doctor,
    #[command(about = "Ingest trusted workspace sources into the local wiki")]
    Ingest {
        #[command(subcommand)]
        command: IngestCommand,
    },
    #[command(about = "Build local wiki context for a question")]
    Query { question: String },
    #[command(about = "Run non-mutating checks")]
    Lint {
        #[command(subcommand)]
        command: LintCommand,
    },
    #[command(about = "Inspect operation logs")]
    Log {
        #[command(subcommand)]
        command: LogCommand,
    },
    #[command(about = "List ingested source records")]
    Sources,
    #[command(about = "Manage local model profiles")]
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
    #[command(about = "Preview or approve a strict JSON command proposal")]
    Proposal {
        #[arg(long)]
        json: String,
        #[arg(long)]
        yes: bool,
    },
    #[command(about = "Inspect saved wiki pages")]
    Wiki {
        #[command(subcommand)]
        command: WikiCommand,
    },
}

#[derive(Debug, Subcommand)]
enum WikiCommand {
    #[command(about = "List saved wiki pages")]
    List,
    #[command(about = "Search saved wiki pages")]
    Search { query: String },
}

#[derive(Debug, Subcommand)]
enum IngestCommand {
    #[command(about = "Ingest one trusted workspace file")]
    File { path: PathBuf },
    #[command(about = "Recursively ingest supported files in a folder")]
    Folder { path: PathBuf },
}

#[derive(Debug, Subcommand)]
enum LintCommand {
    #[command(about = "Check wiki frontmatter, wikilinks, and duplicate titles")]
    Wiki,
}

#[derive(Debug, Subcommand)]
enum LogCommand {
    #[command(about = "Show recent operation log entries")]
    Recent,
}

#[derive(Debug, Subcommand)]
enum ModelsCommand {
    #[command(about = "List model profiles")]
    List,
    #[command(about = "Show the active model profile")]
    Show,
    #[command(about = "Switch the active model profile")]
    Use { id: String },
    #[command(about = "Add or replace a local OpenAI-compatible model profile")]
    AddLocal {
        #[arg(long)]
        id: String,
        #[arg(long)]
        backend: String,
        #[arg(long = "base-url")]
        base_url: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Default)]
struct ReplState {
    last_proposal: Option<CommandProposal>,
    runs: Vec<CommandRun>,
}

#[derive(Clone, Copy)]
struct CommandContext<'a> {
    settings: &'a codesmith_core::AppSettings,
    wiki: Option<&'a WikiStore>,
    storage: Option<&'a Storage>,
    session_id: Option<Uuid>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(Command::Doctor) = cli.command {
        let settings = load_settings()?;
        print!(
            "{}",
            codesmith_cli::doctor_output(&settings, &settings_path()).await
        );
    } else if let Some(Command::Ingest { command }) = cli.command {
        let settings = load_settings()?;
        let root = codesmith_root();
        let wiki = WikiStore::open(&root)?;
        let storage = Storage::open(&root)?;
        match command {
            IngestCommand::File { path } => print!(
                "{}",
                codesmith_cli::ingest_file_output(
                    &wiki,
                    &storage,
                    &settings.default_workspace,
                    &path
                )?
            ),
            IngestCommand::Folder { path } => print!(
                "{}",
                codesmith_cli::ingest_folder_output(
                    &wiki,
                    &storage,
                    &settings.default_workspace,
                    &path
                )?
            ),
        }
    } else if let Some(Command::Query { question }) = cli.command {
        let root = codesmith_root();
        let wiki = WikiStore::open(&root)?;
        print!("{}", codesmith_cli::query_output(&wiki, &question)?);
    } else if let Some(Command::Lint { command }) = cli.command {
        let root = codesmith_root();
        let wiki = WikiStore::open(&root)?;
        match command {
            LintCommand::Wiki => print!("{}", codesmith_cli::lint_wiki_output(&wiki)?),
        }
    } else if let Some(Command::Log { command }) = cli.command {
        let root = codesmith_root();
        match command {
            LogCommand::Recent => print!("{}", codesmith_cli::log_recent_output(&root)?),
        }
    } else if let Some(Command::Sources) = cli.command {
        let root = codesmith_root();
        let storage = Storage::open(&root)?;
        print!("{}", codesmith_cli::sources_output(&storage)?);
    } else if let Some(Command::Models { command }) = cli.command {
        let mut settings = load_settings()?;
        match command {
            ModelsCommand::List => print!("{}", codesmith_cli::model_profiles_output(&settings)),
            ModelsCommand::Show => {
                print!("{}", codesmith_cli::active_model_profile_output(&settings))
            }
            ModelsCommand::Use { id } => {
                print!("{}", codesmith_cli::use_model_profile(&mut settings, &id)?);
                save_settings(&settings)?;
            }
            ModelsCommand::AddLocal {
                id,
                backend,
                base_url,
                model,
                name,
            } => {
                let backend = codesmith_cli::parse_backend_kind(&backend)?;
                print!(
                    "{}",
                    codesmith_cli::add_local_model_profile(
                        &mut settings,
                        &id,
                        backend,
                        &base_url,
                        &model,
                        name.as_deref(),
                    )?
                );
                save_settings(&settings)?;
            }
        }
    } else if let Some(Command::Chat) = cli.command {
        run_chat().await?;
    } else if let Some(Command::Proposal { json, yes }) = cli.command {
        let settings = load_settings()?;
        print!(
            "{}",
            codesmith_cli::handle_proposal_json(&json, &settings, yes || cli.yes).await?
        );
    } else if let Some(Command::Wiki { command }) = cli.command {
        let root = codesmith_root();
        let wiki = WikiStore::open(&root)?;
        match command {
            WikiCommand::List => print!("{}", codesmith_cli::wiki_list_output(&wiki)?),
            WikiCommand::Search { query } => {
                print!("{}", codesmith_cli::wiki_search_output(&wiki, &query)?)
            }
        }
    } else if let Some(prompt) = cli.prompt {
        let settings = load_settings()?;
        let root = codesmith_root();
        let wiki = WikiStore::open(&root).ok();
        print!(
            "{}",
            codesmith_cli::handle_print_prompt(&prompt, &settings, wiki.as_ref(), cli.yes).await?
        );
    } else {
        println!("{}", codesmith_cli::approval_hint());
    }
    Ok(())
}

async fn run_chat() -> Result<()> {
    let mut settings = load_settings()?;
    let root = codesmith_root();
    let wiki = WikiStore::open(&root).ok();
    let storage = Storage::open(&root).ok();
    let session_id = storage
        .as_ref()
        .and_then(|store| store.create_session("CLI Chat").ok());
    let mut history = Vec::<ChatMessage>::new();
    let mut repl_state = ReplState::default();
    ensure_workspace_trust(&root, &settings.default_workspace)?;

    print_chat_banner(&settings, wiki.as_ref(), session_id);

    let mut editor = if io::stdin().is_terminal() {
        Some(DefaultEditor::new()?)
    } else {
        None
    };

    loop {
        let line = match read_repl_line(editor.as_mut())? {
            Some(line) => line,
            None => break,
        };

        match codesmith_cli::parse_repl_line(&line) {
            codesmith_cli::ReplCommand::Empty => {}
            codesmith_cli::ReplCommand::Help => print!("{}", codesmith_cli::repl_help()),
            codesmith_cli::ReplCommand::Prompts => {
                print!("{}", codesmith_cli::recommended_prompts_output());
            }
            codesmith_cli::ReplCommand::Settings => {
                print!(
                    "{}",
                    codesmith_cli::settings_summary(&settings, &settings_path())
                );
            }
            codesmith_cli::ReplCommand::Models => {
                print!("{}", codesmith_cli::model_profiles_output(&settings));
            }
            codesmith_cli::ReplCommand::ModelShow => {
                print!("{}", codesmith_cli::active_model_profile_output(&settings));
            }
            codesmith_cli::ReplCommand::ModelUse(id) => {
                print!("{}", codesmith_cli::use_model_profile(&mut settings, &id)?);
                save_settings(&settings)?;
            }
            codesmith_cli::ReplCommand::Set(update) => {
                let message = codesmith_cli::apply_setting_update(&mut settings, update)?;
                save_settings(&settings)?;
                println!("{message}");
                print!(
                    "{}",
                    codesmith_cli::settings_summary(&settings, &settings_path())
                );
            }
            codesmith_cli::ReplCommand::Doctor => {
                print!(
                    "{}",
                    codesmith_cli::doctor_output(&settings, &settings_path()).await
                );
            }
            codesmith_cli::ReplCommand::IngestFile(path) => {
                if let (Some(wiki), Some(storage)) = (wiki.as_ref(), storage.as_ref()) {
                    print!(
                        "{}",
                        codesmith_cli::ingest_file_output(
                            wiki,
                            storage,
                            &settings.default_workspace,
                            &path
                        )?
                    );
                } else {
                    println!("wiki unavailable");
                }
            }
            codesmith_cli::ReplCommand::IngestFolder(path) => {
                if let (Some(wiki), Some(storage)) = (wiki.as_ref(), storage.as_ref()) {
                    print!(
                        "{}",
                        codesmith_cli::ingest_folder_output(
                            wiki,
                            storage,
                            &settings.default_workspace,
                            &path
                        )?
                    );
                } else {
                    println!("wiki unavailable");
                }
            }
            codesmith_cli::ReplCommand::Query(question) => {
                if let Some(wiki) = wiki.as_ref() {
                    print!("{}", codesmith_cli::query_output(wiki, &question)?);
                } else {
                    println!("wiki unavailable");
                }
            }
            codesmith_cli::ReplCommand::LintWiki => {
                if let Some(wiki) = wiki.as_ref() {
                    print!("{}", codesmith_cli::lint_wiki_output(wiki)?);
                } else {
                    println!("wiki unavailable");
                }
            }
            codesmith_cli::ReplCommand::LogRecent => {
                print!("{}", codesmith_cli::log_recent_output(&root)?);
            }
            codesmith_cli::ReplCommand::Sources => {
                if let Some(storage) = storage.as_ref() {
                    print!("{}", codesmith_cli::sources_output(storage)?);
                } else {
                    println!("storage unavailable");
                }
            }
            codesmith_cli::ReplCommand::Plan(goal) => {
                let prompt = codesmith_cli::plan_workflow_prompt(&goal);
                handle_interactive_prompt(
                    &prompt,
                    &settings,
                    wiki.as_ref(),
                    storage.as_ref(),
                    session_id,
                    &mut history,
                    &mut repl_state,
                )
                .await?;
            }
            codesmith_cli::ReplCommand::Debug(symptom) => {
                let prompt = codesmith_cli::debug_workflow_prompt(&symptom);
                handle_interactive_prompt(
                    &prompt,
                    &settings,
                    wiki.as_ref(),
                    storage.as_ref(),
                    session_id,
                    &mut history,
                    &mut repl_state,
                )
                .await?;
            }
            codesmith_cli::ReplCommand::Verify => {
                print!("{}", codesmith_cli::verification_output(&repl_state.runs));
            }
            codesmith_cli::ReplCommand::Review => {
                print!("{}", codesmith_cli::review_output(&repl_state.runs));
            }
            codesmith_cli::ReplCommand::Tools => print!("{}", codesmith_cli::tools_output()),
            codesmith_cli::ReplCommand::Runs => print!("{}", runs_output(&repl_state)),
            codesmith_cli::ReplCommand::Last => print!("{}", last_run_output(&repl_state)),
            codesmith_cli::ReplCommand::Retry => {
                retry_last_proposal(
                    CommandContext {
                        settings: &settings,
                        wiki: wiki.as_ref(),
                        storage: storage.as_ref(),
                        session_id,
                    },
                    &mut history,
                    &mut repl_state,
                )
                .await?;
            }
            codesmith_cli::ReplCommand::Clear => {
                history.clear();
                repl_state = ReplState::default();
                println!("chat history cleared");
            }
            codesmith_cli::ReplCommand::WikiList => {
                if let Some(wiki) = wiki.as_ref() {
                    print!("{}", codesmith_cli::wiki_list_output(wiki)?);
                } else {
                    println!("wiki unavailable");
                }
            }
            codesmith_cli::ReplCommand::WikiSearch(query) => {
                if let Some(wiki) = wiki.as_ref() {
                    print!("{}", codesmith_cli::wiki_search_output(wiki, &query)?);
                } else {
                    println!("wiki unavailable");
                }
            }
            codesmith_cli::ReplCommand::Exit => break,
            codesmith_cli::ReplCommand::Unknown(command) => {
                println!("unknown command: {command}");
                println!("Type /help for available commands.");
            }
            codesmith_cli::ReplCommand::Prompt(prompt) => {
                let expanded_prompt =
                    codesmith_cli::expand_at_mentions(&prompt, &settings.default_workspace)?;
                handle_interactive_prompt(
                    &expanded_prompt,
                    &settings,
                    wiki.as_ref(),
                    storage.as_ref(),
                    session_id,
                    &mut history,
                    &mut repl_state,
                )
                .await?;
            }
        }
    }

    Ok(())
}

fn read_repl_line(editor: Option<&mut DefaultEditor>) -> Result<Option<String>> {
    if let Some(editor) = editor {
        match editor.readline("codesmith> ") {
            Ok(line) => {
                if !line.trim().is_empty() {
                    let _ = editor.add_history_entry(line.as_str());
                }
                Ok(Some(line))
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                Ok(Some(String::new()))
            }
            Err(ReadlineError::Eof) => {
                println!();
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    } else {
        print!("codesmith> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            println!();
            Ok(None)
        } else {
            Ok(Some(line))
        }
    }
}

fn print_chat_banner(
    settings: &codesmith_core::AppSettings,
    wiki: Option<&WikiStore>,
    session_id: Option<Uuid>,
) {
    let mut settings = settings.clone();
    settings.ensure_model_profiles();
    let profile = settings.active_model_profile();
    println!("CodeSmith Rich REPL");
    println!("Type /help for commands, /tools for tool policy, /exit to quit.");
    println!(
        "profile: {}  backend: {}  model: {}",
        settings.active_profile,
        profile
            .map(|profile| profile.backend_kind.as_str())
            .unwrap_or("missing"),
        settings.llm_model
    );
    println!(
        "workspace: {}  timeout: {}s  session: {}",
        settings.default_workspace.display(),
        settings.command_timeout_secs,
        session_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "transient".to_string())
    );
    println!(
        "wiki: {}  tools: approval-gated shell runner\n",
        if wiki.is_some() {
            "available"
        } else {
            "unavailable"
        }
    );
}

fn runs_output(state: &ReplState) -> String {
    if state.runs.is_empty() {
        return "Command runs\nnone\n".to_string();
    }
    let lines = state
        .runs
        .iter()
        .enumerate()
        .map(|(index, run)| {
            format!(
                "{}. {:?} exit {:?}  {}",
                index + 1,
                run.status,
                run.exit_code,
                compact_command(&run.proposal.command)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("Command runs\n{lines}\n")
}

fn last_run_output(state: &ReplState) -> String {
    let Some(run) = state.runs.last() else {
        return "Last command run\nnone\n".to_string();
    };
    format!(
        "Last command run\ncommand: {}\ncwd: {}\n{}",
        run.proposal.command,
        run.proposal.cwd.display(),
        codesmith_cli::format_command_run(run)
    )
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

async fn retry_last_proposal(
    context: CommandContext<'_>,
    history: &mut Vec<ChatMessage>,
    state: &mut ReplState,
) -> Result<()> {
    let Some(proposal) = state
        .last_proposal
        .clone()
        .or_else(|| state.runs.last().map(|run| run.proposal.clone()))
    else {
        println!("no command proposal to retry");
        return Ok(());
    };
    handle_command_proposal(
        "Retry last command proposal.",
        proposal,
        context,
        history,
        state,
    )
    .await
}

fn ensure_workspace_trust(root: &std::path::Path, workspace: &std::path::Path) -> Result<()> {
    let trust_file = codesmith_cli::trusted_workspaces_path(root);
    if codesmith_cli::is_workspace_trusted(&trust_file, workspace)? {
        return Ok(());
    }

    print!("{}", codesmith_cli::workspace_trust_prompt(workspace));
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if matches!(answer.trim(), "yes" | "YES" | "y" | "Y") {
        codesmith_cli::trust_workspace(&trust_file, workspace)?;
        println!("workspace trusted");
        Ok(())
    } else {
        anyhow::bail!("workspace was not trusted");
    }
}

async fn handle_interactive_prompt(
    prompt: &str,
    settings: &codesmith_core::AppSettings,
    wiki: Option<&WikiStore>,
    storage: Option<&Storage>,
    session_id: Option<Uuid>,
    history: &mut Vec<ChatMessage>,
    state: &mut ReplState,
) -> Result<()> {
    println!("CodeSmith is generating a response...");
    let messages = codesmith_cli::build_conversation_messages(prompt, history, settings, wiki);
    let output = OpenAiClient::new(settings.clone())
        .stream_chat(&messages)
        .await?
        .concat();

    match codesmith_cli::parse_cli_agent_output(&output)? {
        AgentOutput::Text(text) => {
            println!("{text}");
            push_message(
                storage,
                session_id,
                history,
                ChatRole::User,
                prompt.to_string(),
            );
            push_message(storage, session_id, history, ChatRole::Assistant, text);
        }
        AgentOutput::Command(proposal) => {
            let context = CommandContext {
                settings,
                wiki,
                storage,
                session_id,
            };
            handle_command_proposal(prompt, proposal, context, history, state).await?;
        }
    }

    Ok(())
}

async fn handle_command_proposal(
    prompt: &str,
    proposal: CommandProposal,
    context: CommandContext<'_>,
    history: &mut Vec<ChatMessage>,
    state: &mut ReplState,
) -> Result<()> {
    let (proposal, decision) =
        codesmith_cli::policy_decision_for_proposal(proposal, context.settings);
    state.last_proposal = Some(proposal.clone());
    let preview = codesmith_cli::preview_proposal(proposal.clone(), context.settings, false);
    print!("{preview}");

    if !decision.allowed {
        let run = CommandRun::new(
            proposal,
            CommandStatus::Blocked,
            String::new(),
            decision.reason,
            None,
        );
        let command_result = codesmith_cli::format_command_run(&run);
        state.runs.push(run);
        if let Some(wiki) = context.wiki {
            let run = state.runs.last().expect("blocked run was just pushed");
            codesmith_cli::save_command_run_evidence(
                wiki,
                run,
                "blocked by policy before approval",
            );
        }
        push_message(
            context.storage,
            context.session_id,
            history,
            ChatRole::User,
            prompt.to_string(),
        );
        push_message(
            context.storage,
            context.session_id,
            history,
            ChatRole::Assistant,
            format!("Command blocked by policy.\n\nCommand execution result:\n{command_result}"),
        );
        return Ok(());
    }

    let approved = codesmith_cli::read_required_approval(io::stdin().lock(), io::stdout())?;
    let command_result = if approved {
        let run = codesmith_cli::run_approved_proposal(proposal, context.settings).await?;
        let output = codesmith_cli::format_command_run(&run);
        print!("{output}");
        persist_run(context.storage, context.session_id, &run);
        if let Some(wiki) = context.wiki {
            codesmith_cli::save_command_run_evidence(
                wiki,
                &run,
                "tool execution completed; inspect stdout, stderr, and exit status",
            );
        }
        state.runs.push(run);
        output
    } else {
        println!("rejected");
        let run = CommandRun::new(
            proposal,
            CommandStatus::Rejected,
            String::new(),
            String::new(),
            None,
        );
        let output = codesmith_cli::format_command_run(&run);
        if let Some(wiki) = context.wiki {
            codesmith_cli::save_command_run_evidence(wiki, &run, "user rejected before execution");
        }
        state.runs.push(run);
        output
    };

    push_message(
        context.storage,
        context.session_id,
        history,
        ChatRole::User,
        prompt.to_string(),
    );
    push_message(
        context.storage,
        context.session_id,
        history,
        ChatRole::Assistant,
        format!("Command execution result:\n{command_result}"),
    );
    Ok(())
}

fn push_message(
    storage: Option<&Storage>,
    session_id: Option<Uuid>,
    history: &mut Vec<ChatMessage>,
    role: ChatRole,
    content: String,
) {
    let message = ChatMessage::new(role, content);
    if let (Some(storage), Some(session_id)) = (storage, session_id) {
        let _ = storage.append_message(session_id, &message);
    }
    history.push(message);
}

fn persist_run(storage: Option<&Storage>, session_id: Option<Uuid>, run: &CommandRun) {
    if let (Some(storage), Some(session_id)) = (storage, session_id) {
        let _ = storage.insert_command_run(session_id, run);
    }
}

fn codesmith_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codesmith")
}
