use codesmith_agent::{AgentOutput, parse_agent_output};
use codesmith_core::{
    AppSettings, ChatMessage, ChatRole, CommandProposal, CommandRun, CommandStatus, RiskLevel,
    RunnerEvent,
};
use codesmith_llm::OpenAiClient;
use codesmith_policy::evaluate;
use codesmith_runner::run_approved_command_streaming;
use codesmith_storage::{Storage, load_settings, save_settings};
use codesmith_wiki::WikiStore;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use uuid::Uuid;

const CJK_FONT_NAME: &str = "codesmith_cjk_fallback";

const BG: egui::Color32 = egui::Color32::from_rgb(10, 11, 13);
const PANEL: egui::Color32 = egui::Color32::from_rgb(16, 18, 22);
const PANEL_SOFT: egui::Color32 = egui::Color32::from_rgb(24, 27, 32);
const PANEL_RAISED: egui::Color32 = egui::Color32::from_rgb(30, 34, 40);
const BORDER: egui::Color32 = egui::Color32::from_rgb(48, 54, 63);
const TEXT: egui::Color32 = egui::Color32::from_rgb(232, 235, 240);
const MUTED: egui::Color32 = egui::Color32::from_rgb(145, 153, 166);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(88, 166, 255);
const SUCCESS: egui::Color32 = egui::Color32::from_rgb(63, 185, 116);
const DANGER: egui::Color32 = egui::Color32::from_rgb(255, 107, 107);
const WARNING: egui::Color32 = egui::Color32::from_rgb(228, 179, 99);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Muted,
    Accent,
    Success,
    Danger,
}

enum UiEvent {
    AssistantText(String),
    CommandStream(Uuid, RunnerEvent),
    CommandFinished(Uuid, CommandRun),
    Error(String),
    ConnectionTest(String),
}

pub struct CodeSmithApp {
    settings: AppSettings,
    prompt: String,
    messages: Vec<ChatMessage>,
    proposals: Vec<CommandProposal>,
    runs: Vec<CommandRun>,
    wiki_hits: Vec<String>,
    wiki_pages: Vec<String>,
    status: String,
    session_id: Option<Uuid>,
    storage: Option<Storage>,
    wiki: Option<WikiStore>,
    tx: Sender<UiEvent>,
    rx: Receiver<UiEvent>,
}

impl CodeSmithApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_codex_theme(&cc.egui_ctx);
        install_cjk_fonts(&cc.egui_ctx);
        let settings = load_settings().unwrap_or_default();
        let root = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".codesmith");
        let storage = Storage::open(&root).ok();
        let session_id = storage.as_ref().and_then(|store| {
            store
                .list_sessions()
                .ok()
                .and_then(|sessions| sessions.first().map(|session| session.id))
                .or_else(|| store.create_session("Local Session").ok())
        });
        let wiki = WikiStore::open(&root).ok();
        let wiki_pages = load_wiki_page_titles(wiki.as_ref());
        let (tx, rx) = std::sync::mpsc::channel();
        let messages = storage
            .as_ref()
            .zip(session_id)
            .and_then(|(store, session_id)| store.load_transcript(session_id).ok())
            .unwrap_or_default();
        let runs = storage
            .as_ref()
            .zip(session_id)
            .and_then(|(store, session_id)| store.list_command_runs(session_id).ok())
            .unwrap_or_default();
        Self {
            settings,
            prompt: String::new(),
            messages,
            proposals: Vec::new(),
            runs,
            wiki_hits: Vec::new(),
            wiki_pages,
            status: "Ready".to_string(),
            session_id,
            storage,
            wiki,
            tx,
            rx,
        }
    }

    fn submit_prompt(&mut self, ctx: egui::Context) {
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.prompt.clear();
        let wiki_context = self.refresh_wiki_hits(&prompt);
        let message = ChatMessage::new(ChatRole::User, prompt);
        self.persist_message(&message);
        self.messages.push(message);
        self.status = "Waiting for local LLM".to_string();

        let tx = self.tx.clone();
        let settings = self.settings.clone();
        let mut messages = self.messages.clone();
        if let Some(context) = wiki_context {
            messages.insert(0, ChatMessage::new(ChatRole::System, context));
        }
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.to_string()));
                    return;
                }
            };
            runtime.block_on(async move {
                let client = OpenAiClient::new(settings);
                match client.stream_chat(&messages).await {
                    Ok(chunks) => {
                        let output = chunks.concat();
                        let _ = tx.send(UiEvent::AssistantText(output));
                        ctx.request_repaint();
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.to_string()));
                        ctx.request_repaint();
                    }
                }
            });
        });
    }

    fn test_connection(&mut self, ctx: egui::Context) {
        let tx = self.tx.clone();
        let settings = self.settings.clone();
        self.status = "Testing connection".to_string();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.send(UiEvent::ConnectionTest(error.to_string()));
                    return;
                }
            };
            runtime.block_on(async move {
                let client = OpenAiClient::new(settings);
                let message = match client.test_connection().await {
                    Ok(()) => "Connection OK".to_string(),
                    Err(error) => format!("Connection failed: {error}"),
                };
                let _ = tx.send(UiEvent::ConnectionTest(message));
                ctx.request_repaint();
            });
        });
    }

    fn approve_command(&mut self, index: usize, ctx: egui::Context) {
        let Some(proposal) = take_proposal(&mut self.proposals, index) else {
            return;
        };
        let decision = evaluate(&proposal, &self.settings.default_workspace);
        if !decision.allowed {
            self.runs.push(CommandRun::new(
                proposal,
                CommandStatus::Blocked,
                String::new(),
                decision.reason,
                None,
            ));
            return;
        }

        let tx = self.tx.clone();
        let timeout = Duration::from_secs(self.settings.command_timeout_secs);
        let running_run = CommandRun::new(
            proposal.clone(),
            CommandStatus::Running,
            String::new(),
            String::new(),
            None,
        );
        let run_id = running_run.id;
        self.runs.push(running_run);
        self.status = "Running approved command".to_string();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.to_string()));
                    return;
                }
            };
            runtime.block_on(async move {
                let (runner_tx, mut runner_rx) = tokio::sync::mpsc::unbounded_channel();
                let stream_tx = tx.clone();
                let stream_ctx = ctx.clone();
                tokio::spawn(async move {
                    while let Some(event) = runner_rx.recv().await {
                        let _ = stream_tx.send(UiEvent::CommandStream(run_id, event));
                        stream_ctx.request_repaint();
                    }
                });
                match run_approved_command_streaming(proposal, timeout, runner_tx).await {
                    Ok(mut run) => {
                        run.id = run_id;
                        let _ = tx.send(UiEvent::CommandFinished(run_id, run));
                        ctx.request_repaint();
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.to_string()));
                        ctx.request_repaint();
                    }
                }
            });
        });
    }

    fn reject_command(&mut self, index: usize) {
        if let Some(proposal) = take_proposal(&mut self.proposals, index) {
            self.runs.push(CommandRun::new(
                proposal,
                CommandStatus::Rejected,
                String::new(),
                String::new(),
                None,
            ));
        }
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UiEvent::AssistantText(text) => {
                    match parse_agent_output(&text) {
                        Ok(AgentOutput::Command(mut proposal)) => {
                            proposal.cwd = resolve_proposal_cwd(
                                &proposal.cwd,
                                &self.settings.default_workspace,
                            );
                            self.proposals.push(proposal);
                        }
                        Ok(AgentOutput::Text(text)) => {
                            let message = ChatMessage::new(ChatRole::Assistant, text);
                            self.persist_message(&message);
                            self.messages.push(message);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                    self.status = "Ready".to_string();
                }
                UiEvent::CommandStream(run_id, event) => {
                    if let Some(run) = self.runs.iter_mut().find(|run| run.id == run_id) {
                        match event {
                            RunnerEvent::Stdout(chunk) => run.stdout.push_str(&chunk),
                            RunnerEvent::Stderr(chunk) => run.stderr.push_str(&chunk),
                            RunnerEvent::Finished(status) => run.status = status,
                        }
                    }
                }
                UiEvent::CommandFinished(run_id, run) => {
                    self.persist_run(&run);
                    if let Some(existing) =
                        self.runs.iter_mut().find(|existing| existing.id == run_id)
                    {
                        *existing = run;
                    } else {
                        self.runs.push(run);
                    }
                    self.status = "Command finished".to_string();
                }
                UiEvent::Error(error) => self.status = error,
                UiEvent::ConnectionTest(message) => self.status = message,
            }
        }
    }

    fn persist_message(&self, message: &ChatMessage) {
        if let (Some(store), Some(session_id)) = (&self.storage, self.session_id) {
            let _ = store.append_message(session_id, message);
        }
    }

    fn persist_run(&self, run: &CommandRun) {
        if let (Some(store), Some(session_id)) = (&self.storage, self.session_id) {
            let _ = store.insert_command_run(session_id, run);
        }
    }

    fn refresh_wiki_hits(&mut self, query: &str) -> Option<String> {
        let Some(wiki) = &self.wiki else {
            return None;
        };
        let pages = wiki.search(query, 5).unwrap_or_default();
        self.wiki_hits = pages.iter().map(|page| page.title.clone()).collect();
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
}

impl eframe::App for CodeSmithApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();

        self.render_left_sidebar(ctx);
        self.render_right_panel(ctx);
        self.render_status_bar(ctx);
        self.render_chat(ctx);
    }
}

impl CodeSmithApp {
    fn render_left_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("claurst_sidebar")
            .resizable(true)
            .default_width(286.0)
            .width_range(248.0..=380.0)
            .frame(panel_frame(PANEL))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 9.0);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("CodeSmith")
                            .size(21.0)
                            .strong()
                            .color(TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        status_pill(ui, "LOCAL", SUCCESS);
                    });
                });
                ui.label(
                    egui::RichText::new("terminal coding agent / execution only")
                        .color(MUTED)
                        .small(),
                );
                ui.add_space(12.0);
                section_header(ui, "Session");
                sidebar_item(ui, "main  /  local workspace", true);
                sidebar_item(ui, "approval-gated tools", false);
                sidebar_item(ui, "wiki memory enabled", false);

                ui.add_space(12.0);
                section_header(ui, "Model");
                self.settings.ensure_model_profiles();
                key_value_line(ui, "profile", &self.settings.active_profile);
                key_value_line(ui, "backend", active_backend_label(&self.settings));
                field_label(ui, "Base URL");
                let mut base_url = self.settings.llm_base_url.clone();
                if ui.text_edit_singleline(&mut base_url).changed() {
                    if let Some(profile) = self.settings.active_model_profile_mut() {
                        profile.base_url = base_url;
                    }
                    self.settings.ensure_model_profiles();
                }
                field_label(ui, "Model");
                let mut model = self.settings.llm_model.clone();
                if ui.text_edit_singleline(&mut model).changed() {
                    if let Some(profile) = self.settings.active_model_profile_mut() {
                        profile.model = model;
                    }
                    self.settings.ensure_model_profiles();
                }
                field_label(ui, "API key");
                let mut api_key = self.settings.api_key.clone().unwrap_or_default();
                if ui.text_edit_singleline(&mut api_key).changed() {
                    let api_key = if api_key.is_empty() {
                        None
                    } else {
                        Some(api_key)
                    };
                    if let Some(profile) = self.settings.active_model_profile_mut() {
                        profile.api_key = api_key;
                    }
                    self.settings.ensure_model_profiles();
                }
                field_label(ui, "Workspace");
                let mut workspace = self.settings.default_workspace.display().to_string();
                if ui.text_edit_singleline(&mut workspace).changed() {
                    self.settings.default_workspace = PathBuf::from(workspace);
                }
                ui.add(
                    egui::Slider::new(&mut self.settings.command_timeout_secs, 1..=600)
                        .text("timeout"),
                );
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.status = match save_settings(&self.settings) {
                            Ok(()) => "Settings saved".to_string(),
                            Err(error) => format!("Settings save failed: {error}"),
                        };
                    }
                    if ui.button("Test").clicked() {
                        self.test_connection(ctx.clone());
                    }
                });
                ui.add_space(12.0);
                section_header(ui, "Commands");
                command_hint(ui, "/settings", "show runtime settings");
                command_hint(ui, "/models", "list local model profiles");
                command_hint(ui, "/wiki search", "search local memory");
                command_hint(ui, "JSON proposal", "approve before execution");
            });
    }

    fn render_right_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("claurst_tools")
            .resizable(true)
            .default_width(356.0)
            .width_range(300.0..=500.0)
            .frame(panel_frame(PANEL))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Tools").size(18.0).strong().color(TEXT));
                ui.label(
                    egui::RichText::new("approvals, command runs, and wiki context").color(MUTED),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    status_pill(ui, &format!("{} pending", self.proposals.len()), WARNING);
                    status_pill(ui, &format!("{} runs", self.runs.len()), ACCENT);
                    status_pill(ui, &format!("{} wiki", self.wiki_pages.len()), SUCCESS);
                });
                ui.add_space(12.0);
                self.render_command_proposals(ui, ctx);
                ui.add_space(12.0);
                self.render_runs(ui);
                ui.add_space(12.0);
                section_header(ui, "Wiki");
                if self.wiki_hits.is_empty() {
                    ui.label(egui::RichText::new("No context loaded").color(MUTED));
                } else {
                    ui.label(egui::RichText::new("Context loaded for current prompt").color(MUTED));
                    for hit in &self.wiki_hits {
                        subtle_card(ui, |ui| {
                            ui.label(egui::RichText::new(hit).color(TEXT));
                        });
                    }
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Saved pages").color(MUTED).small());
                if self.wiki_pages.is_empty() {
                    ui.label(egui::RichText::new("No saved wiki pages").color(MUTED));
                } else {
                    for title in &self.wiki_pages {
                        ui.label(egui::RichText::new(format!("• {title}")).color(TEXT));
                    }
                }
            });
    }

    fn render_command_proposals(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section_header(ui, "Command proposals");
        let mut action = None;
        if self.proposals.is_empty() {
            ui.label(egui::RichText::new(empty_pending_proposals_text()).color(MUTED));
            return;
        }
        for (index, proposal) in self.proposals.iter().enumerate() {
            let decision = evaluate(proposal, &self.settings.default_workspace);
            tool_card(ui, |ui| {
                ui.horizontal(|ui| {
                    status_pill(
                        ui,
                        if decision.allowed {
                            "APPROVAL"
                        } else {
                            "BLOCKED"
                        },
                        if decision.allowed { WARNING } else { DANGER },
                    );
                    ui.label(egui::RichText::new("shell command").color(MUTED).small());
                });
                code_block(ui, &proposal.command);
                ui.label(
                    egui::RichText::new(format!("cwd {}", proposal.cwd.display())).color(MUTED),
                );
                ui.label(egui::RichText::new(&proposal.reason).color(TEXT));
                if !decision.allowed || proposal.risk_level == RiskLevel::Blocked {
                    ui.colored_label(DANGER, decision.reason);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(decision.allowed, egui::Button::new("Approve"))
                        .clicked()
                    {
                        action = Some((true, index));
                    }
                    if ui.button("Reject").clicked() {
                        action = Some((false, index));
                    }
                });
            });
        }
        if let Some((approve, index)) = action {
            if approve {
                self.approve_command(index, ctx.clone());
            } else {
                self.reject_command(index);
            }
        }
    }

    fn render_runs(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Runs");
        let mut run_action = None;
        if self.runs.is_empty() {
            ui.label(egui::RichText::new("No command runs yet").color(MUTED));
            return;
        }
        for (index, run) in self.runs.iter().enumerate() {
            let color = tone_color(status_tone(run.status));
            ui.collapsing(
                egui::RichText::new(format!(
                    "{:?}  {}",
                    run.status,
                    compact_command(&run.proposal.command)
                ))
                .color(color),
                |ui| {
                    ui.label(egui::RichText::new(format!("exit {:?}", run.exit_code)).color(MUTED));
                    ui.label(egui::RichText::new("stdout").color(MUTED).small());
                    code_block(ui, &run.stdout);
                    ui.label(egui::RichText::new("stderr").color(MUTED).small());
                    code_block(ui, &run.stderr);
                    ui.horizontal(|ui| {
                        if ui.button("Copy").clicked() {
                            run_action = Some(("copy", index));
                        }
                        if ui.button("Save to wiki").clicked() {
                            run_action = Some(("wiki", index));
                        }
                        if ui.button("Ask follow-up").clicked() {
                            run_action = Some(("follow", index));
                        }
                    });
                },
            );
        }
        if let Some((kind, index)) = run_action
            && let Some(run) = self.runs.get(index)
        {
            match kind {
                "copy" => {
                    ui.ctx().copy_text(format!(
                        "$ {}\n\nstdout:\n{}\n\nstderr:\n{}",
                        run.proposal.command, run.stdout, run.stderr
                    ));
                    self.status = "Copied run output".to_string();
                }
                "wiki" => {
                    if let Some(wiki) = &self.wiki {
                        self.status = match wiki.save_page(
                            &format!("Command: {}", run.proposal.command),
                            "commands",
                            &format!(
                                "Command: `{}`\n\ncwd: `{}`\n\nstdout:\n```\n{}\n```\n\nstderr:\n```\n{}\n```",
                                run.proposal.command,
                                run.proposal.cwd.display(),
                                run.stdout,
                                run.stderr
                            ),
                        ) {
                            Ok(page) => {
                                self.wiki_pages.push(page.title);
                                self.wiki_pages.sort();
                                "Saved run to wiki".to_string()
                            }
                            Err(error) => format!("Wiki save failed: {error}"),
                        };
                    }
                }
                "follow" => {
                    self.prompt = format!(
                        "Explain this command result and suggest the next safe command:\n{}",
                        run.stdout
                    );
                }
                _ => {}
            }
        }
    }

    fn render_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("claurst_status")
            .frame(panel_frame(BG))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    status_pill(ui, "STATUS", ACCENT);
                    ui.label(egui::RichText::new(&self.status).monospace().color(TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(&self.settings.llm_model)
                                .monospace()
                                .color(MUTED),
                        );
                    });
                });
            });
    }

    fn render_chat(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(BG))
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("CodeSmith")
                                    .size(19.0)
                                    .strong()
                                    .color(TEXT),
                            );
                            status_pill(ui, "CHAT", ACCENT);
                            status_pill(ui, "NO AUTO-RUN", SUCCESS);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            self.settings.default_workspace.display().to_string(),
                                        )
                                        .monospace()
                                        .small()
                                        .color(MUTED),
                                    );
                                },
                            );
                        });
                    });
                ui.add_space(8.0);
                let scroll_height = chat_scroll_height(ui.available_height());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(scroll_height)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_max_width(900.0);
                        for message in &self.messages {
                            message_row(ui, message);
                        }
                        if let Some(text) = llm_wait_indicator_text(&self.status) {
                            wait_indicator_row(ui, text);
                        }
                    });
                ui.add_space(8.0);
                composer(ui, &mut self.prompt, |prompt_empty, ui| {
                    let send = ui
                        .add_enabled(!prompt_empty, egui::Button::new("Send"))
                        .clicked();
                    let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                    send || enter
                })
                .then(|| self.submit_prompt(ctx.clone()));
            });
    }
}

fn install_codex_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = egui::Color32::from_rgb(8, 9, 11);
    visuals.faint_bg_color = PANEL_SOFT;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.inactive.bg_fill = PANEL_SOFT;
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 42, 48);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(46, 51, 59);
    visuals.widgets.inactive.fg_stroke.color = TEXT;
    visuals.widgets.noninteractive.fg_stroke.color = TEXT;
    visuals.selection.bg_fill = egui::Color32::from_rgb(38, 91, 134);
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);
}

fn take_proposal(proposals: &mut Vec<CommandProposal>, index: usize) -> Option<CommandProposal> {
    if index < proposals.len() {
        Some(proposals.remove(index))
    } else {
        None
    }
}

fn resolve_proposal_cwd(cwd: &Path, workspace: &Path) -> PathBuf {
    if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        workspace.join(cwd)
    }
}

fn role_label(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "System",
        ChatRole::User => "You",
        ChatRole::Assistant => "CodeSmith",
        ChatRole::Tool => "Tool",
    }
}

fn status_tone(status: CommandStatus) -> Tone {
    match status {
        CommandStatus::Succeeded => Tone::Success,
        CommandStatus::Failed | CommandStatus::TimedOut | CommandStatus::Blocked => Tone::Danger,
        CommandStatus::Running => Tone::Accent,
        _ => Tone::Muted,
    }
}

fn tone_color(tone: Tone) -> egui::Color32 {
    match tone {
        Tone::Muted => MUTED,
        Tone::Accent => ACCENT,
        Tone::Success => SUCCESS,
        Tone::Danger => DANGER,
    }
}

fn chat_scroll_height(available_height: f32) -> f32 {
    (available_height - 76.0).max(120.0)
}

fn llm_wait_indicator_text(status: &str) -> Option<&'static str> {
    if status == "Waiting for local LLM" {
        Some("CodeSmith is generating a response...")
    } else {
        None
    }
}

fn empty_pending_proposals_text() -> &'static str {
    "No pending approvals. Approved or rejected proposals are recorded under Runs."
}

fn load_wiki_page_titles(wiki: Option<&WikiStore>) -> Vec<String> {
    wiki.and_then(|wiki| wiki.list_pages().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|page| page.title)
        .collect()
}

fn panel_frame(fill: egui::Color32) -> egui::Frame {
    egui::Frame::default()
        .fill(fill)
        .inner_margin(egui::Margin::same(8))
        .stroke(egui::Stroke::new(1.0, BORDER))
}

fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .small()
            .strong()
            .color(MUTED),
    );
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().color(MUTED));
}

fn sidebar_item(ui: &mut egui::Ui, text: &str, selected: bool) {
    let fill = if selected { PANEL_RAISED } else { PANEL };
    egui::Frame::default()
        .fill(fill)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).monospace().color(TEXT));
        });
}

fn key_value_line(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).monospace().small().color(MUTED));
        ui.label(egui::RichText::new(value).monospace().small().color(TEXT));
    });
}

fn command_hint(ui: &mut egui::Ui, command: &str, description: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(command).monospace().color(ACCENT));
        ui.label(egui::RichText::new(description).small().color(MUTED));
    });
}

fn status_pill(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::default()
        .fill(egui::Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            34,
        ))
        .stroke(egui::Stroke::new(1.0, color))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .monospace()
                    .small()
                    .strong()
                    .color(color),
            );
        });
}

fn subtle_card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::default()
        .fill(PANEL_SOFT)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, add_contents)
        .inner
}

fn tool_card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::default()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, add_contents)
        .inner
}

fn code_block(ui: &mut egui::Ui, text: &str) {
    let body = if text.is_empty() { " " } else { text };
    egui::Frame::default()
        .fill(egui::Color32::from_rgb(5, 6, 8))
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(3.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(body).monospace().color(TEXT));
        });
}

fn message_row(ui: &mut egui::Ui, message: &ChatMessage) {
    let is_user = matches!(message.role, ChatRole::User);
    let fill = if is_user { PANEL_SOFT } else { BG };
    egui::Frame::default()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, if is_user { BORDER } else { BG }))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.set_min_width(860.0);
                ui.label(
                    egui::RichText::new(format!("{} >", role_label(message.role).to_lowercase()))
                        .monospace()
                        .strong()
                        .color(if is_user { ACCENT } else { SUCCESS }),
                );
                ui.label(egui::RichText::new(&message.content).color(TEXT));
            });
        });
    ui.add_space(6.0);
}

fn wait_indicator_row(ui: &mut egui::Ui, text: &str) {
    egui::Frame::default()
        .fill(BG)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(text).color(MUTED));
            });
        });
    ui.add_space(8.0);
}

fn composer(
    ui: &mut egui::Ui,
    prompt: &mut String,
    add_send: impl FnOnce(bool, &mut egui::Ui) -> bool,
) -> bool {
    let mut submitted = false;
    egui::Frame::default()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(">").monospace().strong().color(SUCCESS));
                let response = ui.add_sized(
                    [(ui.available_width() - 80.0).max(160.0), 34.0],
                    egui::TextEdit::singleline(prompt)
                        .hint_text("ask, attach @file, or request a command proposal"),
                );
                let prompt_empty = prompt.trim().is_empty();
                if add_send(prompt_empty, ui)
                    || (response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    submitted = true;
                }
            });
        });
    submitted
}

fn compact_command(command: &str) -> String {
    const LIMIT: usize = 42;
    if command.chars().count() <= LIMIT {
        return command.to_string();
    }
    let mut compact = command.chars().take(LIMIT - 1).collect::<String>();
    compact.push('…');
    compact
}

fn active_backend_label(settings: &AppSettings) -> &str {
    settings
        .active_model_profile()
        .map(|profile| profile.backend_kind.as_str())
        .unwrap_or("missing")
}

fn install_cjk_fonts(ctx: &egui::Context) {
    let Some(font_bytes) = cjk_font_candidates()
        .into_iter()
        .find_map(|path| std::fs::read(path).ok())
    else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    insert_cjk_font_data(&mut fonts, font_bytes);
    ctx.set_fonts(fonts);
}

fn cjk_font_candidates() -> Vec<PathBuf> {
    [
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        "C:/Windows/Fonts/malgun.ttf",
        "C:/Windows/Fonts/msgothic.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansKR-Regular.otf",
    ]
    .into_iter()
    .map(Path::new)
    .map(Path::to_path_buf)
    .collect()
}

fn insert_cjk_font_data(fonts: &mut egui::FontDefinitions, font_bytes: Vec<u8>) {
    fonts.font_data.insert(
        CJK_FONT_NAME.to_string(),
        Arc::new(egui::FontData::from_owned(font_bytes)),
    );

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let entries = fonts.families.entry(family).or_default();
        if !entries.iter().any(|name| name == CJK_FONT_NAME) {
            entries.push(CJK_FONT_NAME.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_font_candidates_include_macos_korean_font() {
        let candidates = cjk_font_candidates();

        assert!(
            candidates
                .iter()
                .any(|path| path.ends_with("AppleSDGothicNeo.ttc"))
        );
    }

    #[test]
    fn cjk_font_data_is_registered_as_proportional_and_monospace_fallback() {
        let mut fonts = egui::FontDefinitions::default();
        insert_cjk_font_data(&mut fonts, vec![0, 1, 2, 3]);

        assert!(fonts.font_data.contains_key(CJK_FONT_NAME));
        assert!(
            fonts
                .families
                .get(&egui::FontFamily::Proportional)
                .expect("proportional family")
                .contains(&CJK_FONT_NAME.to_string())
        );
        assert!(
            fonts
                .families
                .get(&egui::FontFamily::Monospace)
                .expect("monospace family")
                .contains(&CJK_FONT_NAME.to_string())
        );
    }

    #[test]
    fn role_labels_are_compact_for_chat_headers() {
        assert_eq!(role_label(ChatRole::User), "You");
        assert_eq!(role_label(ChatRole::Assistant), "CodeSmith");
        assert_eq!(role_label(ChatRole::System), "System");
        assert_eq!(role_label(ChatRole::Tool), "Tool");
    }

    #[test]
    fn command_status_tone_marks_blocked_and_failed_as_error() {
        assert_eq!(status_tone(CommandStatus::Blocked), Tone::Danger);
        assert_eq!(status_tone(CommandStatus::Failed), Tone::Danger);
        assert_eq!(status_tone(CommandStatus::Succeeded), Tone::Success);
        assert_eq!(status_tone(CommandStatus::Running), Tone::Accent);
    }

    #[test]
    fn taking_proposal_removes_it_from_pending_list() {
        let mut proposals = vec![
            CommandProposal::new("echo first", PathBuf::from("."), "first"),
            CommandProposal::new("echo second", PathBuf::from("."), "second"),
        ];

        let proposal = take_proposal(&mut proposals, 0).expect("proposal should exist");

        assert_eq!(proposal.command, "echo first");
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].command, "echo second");
        assert!(take_proposal(&mut proposals, 99).is_none());
    }

    #[test]
    fn chat_scroll_height_reserves_space_for_composer() {
        assert_eq!(chat_scroll_height(800.0), 724.0);
        assert_eq!(chat_scroll_height(100.0), 120.0);
    }

    #[test]
    fn llm_wait_indicator_only_shows_while_waiting_for_response() {
        assert_eq!(
            llm_wait_indicator_text("Waiting for local LLM"),
            Some("CodeSmith is generating a response...")
        );
        assert_eq!(llm_wait_indicator_text("Ready"), None);
        assert_eq!(llm_wait_indicator_text("Running approved command"), None);
    }

    #[test]
    fn empty_pending_proposals_text_explains_where_approved_items_go() {
        assert_eq!(
            empty_pending_proposals_text(),
            "No pending approvals. Approved or rejected proposals are recorded under Runs."
        );
    }
}
