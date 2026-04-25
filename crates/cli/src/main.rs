use anyhow::Result;
use clap::{Parser, Subcommand};
use codesmith_agent::AgentOutput;
use codesmith_core::{ChatMessage, ChatRole};
use codesmith_llm::OpenAiClient;
use codesmith_storage::{load_settings, save_settings, settings_path};
use codesmith_wiki::WikiStore;
use std::io::{self, Write};
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
    Chat,
    Doctor,
    Proposal {
        #[arg(long)]
        json: String,
        #[arg(long)]
        yes: bool,
    },
    Wiki {
        #[command(subcommand)]
        command: WikiCommand,
    },
}

#[derive(Debug, Subcommand)]
enum WikiCommand {
    List,
    Search { query: String },
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
    let mut history = Vec::<ChatMessage>::new();
    ensure_workspace_trust(&root, &settings.default_workspace)?;

    println!("CodeSmith interactive chat");
    println!("Type /help for commands, /settings to view config, /exit to quit.");
    print!(
        "{}",
        codesmith_cli::settings_summary(&settings, &settings_path())
    );

    loop {
        print!("codesmith> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            println!();
            break;
        }

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
    let messages = codesmith_cli::build_conversation_messages(prompt, history, wiki);
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
            print!(
                "{}",
                codesmith_cli::handle_proposal(proposal.clone(), settings, false).await?
            );
            print!("Approve this command? [y/N] ");
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            if matches!(answer.trim(), "y" | "Y" | "yes" | "YES") {
                print!(
                    "{}",
                    codesmith_cli::handle_proposal(proposal, settings, true).await?
                );
            } else {
                println!("rejected");
            }
            history.push(ChatMessage::new(ChatRole::User, prompt.to_string()));
            history.push(ChatMessage::new(ChatRole::Assistant, output));
        }
    }

    Ok(())
}

fn codesmith_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codesmith")
}
