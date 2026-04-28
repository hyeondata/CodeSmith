use anyhow::Result;
use clap::{Parser, Subcommand};
use codesmith_agent::AgentOutput;
use codesmith_core::{ChatMessage, ChatRole};
use codesmith_llm::OpenAiClient;
use codesmith_storage::{Storage, load_settings, save_settings, settings_path};
use codesmith_wiki::WikiStore;
use rustyline::{DefaultEditor, error::ReadlineError};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

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
    let mut history = Vec::<ChatMessage>::new();
    ensure_workspace_trust(&root, &settings.default_workspace)?;

    println!("CodeSmith interactive chat");
    println!("Type /help for commands, /settings to view config, /exit to quit.");
    print!(
        "{}",
        codesmith_cli::settings_summary(&settings, &settings_path())
    );

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
                handle_interactive_prompt(&expanded_prompt, &settings, wiki.as_ref(), &mut history)
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
    history: &mut Vec<ChatMessage>,
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
            history.push(ChatMessage::new(ChatRole::User, prompt.to_string()));
            history.push(ChatMessage::new(ChatRole::Assistant, text));
        }
        AgentOutput::Command(proposal) => {
            let preview = codesmith_cli::handle_proposal(proposal.clone(), settings, false).await?;
            print!("{preview}");
            let approved = codesmith_cli::read_required_approval(io::stdin().lock(), io::stdout())?;
            let command_result = if approved {
                let command_result =
                    codesmith_cli::handle_proposal(proposal, settings, true).await?;
                print!("{command_result}");
                command_result
            } else {
                println!("rejected");
                "Command proposal rejected by user.\n".to_string()
            };
            history.push(ChatMessage::new(ChatRole::User, prompt.to_string()));
            history.push(ChatMessage::new(
                ChatRole::Assistant,
                format!("{output}\n\nCommand execution result:\n{command_result}"),
            ));
        }
    }

    Ok(())
}

fn codesmith_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codesmith")
}
