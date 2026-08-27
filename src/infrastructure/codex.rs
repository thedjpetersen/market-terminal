use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};

use crate::features::assistant::{
    domain::{
        AssistantRequest, AssistantResponse, AssistantRole, AssistantStreamEvent,
        AssistantTokenUsage, UiAction, COMMAND_PLANE_SYSTEM_PROMPT,
    },
    AssistantError, AssistantGateway,
};

const DEFAULT_TIMEOUT_SECS: u64 = 90;
const MAX_DIAGNOSTIC_BYTES: usize = 8_192;
static NEXT_WORKDIR_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct CodexAppServerConfig {
    binary: PathBuf,
    model: Option<String>,
    timeout: Duration,
    configured: bool,
    configuration_hint: String,
}

impl CodexAppServerConfig {
    pub fn from_env() -> Self {
        let binary = env::var_os("CODEX_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("codex"));
        let model = nonempty_env("CODEX_MODEL");
        let timeout = nonempty_env("CODEX_TIMEOUT_SECS")
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.clamp(15, 300))
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        let (configured, configuration_hint) = chatgpt_login_status(&binary);
        Self {
            binary,
            model,
            timeout: Duration::from_secs(timeout),
            configured,
            configuration_hint,
        }
    }

    #[cfg(test)]
    fn live() -> Self {
        Self::from_env()
    }
}

pub struct CodexAppServerGateway {
    config: CodexAppServerConfig,
    model_label: String,
    worker: Option<CodexWorker>,
}

impl CodexAppServerGateway {
    pub fn new(config: CodexAppServerConfig) -> Self {
        let model_label = config
            .model
            .clone()
            .unwrap_or_else(|| "CHATGPT PRO / CODEX".to_owned());
        let worker = config
            .configured
            .then(|| CodexWorker::spawn(config.clone()))
            .flatten();
        Self {
            config,
            model_label,
            worker,
        }
    }
}

impl AssistantGateway for CodexAppServerGateway {
    fn complete(&self, request: AssistantRequest) -> Result<AssistantResponse, AssistantError> {
        let (updates, _receiver) = mpsc::channel();
        self.complete_stream(request, updates)
    }

    fn complete_stream(
        &self,
        request: AssistantRequest,
        updates: Sender<AssistantStreamEvent>,
    ) -> Result<AssistantResponse, AssistantError> {
        if !self.config.configured {
            return Err(AssistantError::NotConfigured);
        }
        let worker = self.worker.as_ref().ok_or_else(|| {
            AssistantError::Transport("Codex background worker could not start".to_owned())
        })?;
        worker.complete(
            request,
            updates,
            self.config.timeout.saturating_mul(3) + Duration::from_secs(5),
        )
    }

    fn model_label(&self) -> &str {
        &self.model_label
    }

    fn is_configured(&self) -> bool {
        self.config.configured
    }

    fn configuration_hint(&self) -> &str {
        &self.config.configuration_hint
    }
}

enum CodexWorkerMessage {
    Complete {
        request: Box<AssistantRequest>,
        updates: Sender<AssistantStreamEvent>,
        reply: SyncSender<Result<AssistantResponse, AssistantError>>,
    },
    Shutdown,
}

struct CodexWorker {
    sender: SyncSender<CodexWorkerMessage>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl CodexWorker {
    fn spawn(config: CodexAppServerConfig) -> Option<Self> {
        let (sender, receiver) = mpsc::sync_channel(2);
        let handle = thread::Builder::new()
            .name("market-terminal-codex".to_owned())
            .spawn(move || run_codex_worker(config, receiver))
            .ok()?;
        Some(Self {
            sender,
            handle: Mutex::new(Some(handle)),
        })
    }

    fn complete(
        &self,
        request: AssistantRequest,
        updates: Sender<AssistantStreamEvent>,
        timeout: Duration,
    ) -> Result<AssistantResponse, AssistantError> {
        let (reply, response) = mpsc::sync_channel(1);
        self.sender
            .send(CodexWorkerMessage::Complete {
                request: Box::new(request),
                updates,
                reply,
            })
            .map_err(|_| AssistantError::Transport("Codex background worker stopped".to_owned()))?;
        response
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    AssistantError::Transport("Codex background request timed out".to_owned())
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    AssistantError::Transport("Codex background worker disconnected".to_owned())
                }
            })?
    }
}

impl Drop for CodexWorker {
    fn drop(&mut self) {
        let _ = self.sender.try_send(CodexWorkerMessage::Shutdown);
        let _ = self.handle.lock().expect("Codex worker handle lock").take();
    }
}

fn run_codex_worker(config: CodexAppServerConfig, receiver: Receiver<CodexWorkerMessage>) {
    let mut session = CodexSession::connect(&config).ok();
    while let Ok(message) = receiver.recv() {
        match message {
            CodexWorkerMessage::Complete {
                request,
                updates,
                reply,
            } => {
                let _ = reply.send(complete_with_restart(
                    &config,
                    &mut session,
                    *request,
                    updates,
                ));
            }
            CodexWorkerMessage::Shutdown => break,
        }
    }
}

fn complete_with_restart(
    config: &CodexAppServerConfig,
    session: &mut Option<CodexSession>,
    request: AssistantRequest,
    updates: Sender<AssistantStreamEvent>,
) -> Result<AssistantResponse, AssistantError> {
    for attempt in 0..2 {
        if session.is_none() {
            match CodexSession::connect(config) {
                Ok(connected) => *session = Some(connected),
                Err(_) if attempt == 0 => continue,
                Err(error) => return Err(error),
            }
        }
        let result = session
            .as_mut()
            .expect("Codex session connected")
            .complete(request.clone(), updates.clone());
        if matches!(result, Err(AssistantError::Transport(_))) {
            *session = None;
            if attempt == 0 {
                continue;
            }
        }
        return result;
    }
    Err(AssistantError::Transport(
        "Codex background session could not connect".to_owned(),
    ))
}

struct CodexSession {
    config: CodexAppServerConfig,
    workdir: EphemeralWorkdir,
    server: AppServerProcess,
    next_request_id: u64,
}

impl CodexSession {
    fn connect(config: &CodexAppServerConfig) -> Result<Self, AssistantError> {
        let workdir = EphemeralWorkdir::new().map_err(transport)?;
        let mut server = AppServerProcess::spawn(&config.binary, workdir.path())?;
        let deadline = Instant::now() + config.timeout;
        server.send(&json!({
            "method": "initialize",
            "id": 0,
            "params": {
                "clientInfo": {
                    "name": "market_terminal",
                    "title": "Market Terminal",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true,
                    "optOutNotificationMethods": [
                        "item/reasoning/summaryTextDelta",
                        "item/reasoning/textDelta"
                    ]
                }
            }
        }))?;
        server.wait_for_response(0, deadline)?;
        server.send(&json!({ "method": "initialized", "params": {} }))?;
        Ok(Self {
            config: config.clone(),
            workdir,
            server,
            next_request_id: 1,
        })
    }

    fn complete(
        &mut self,
        request: AssistantRequest,
        updates: Sender<AssistantStreamEvent>,
    ) -> Result<AssistantResponse, AssistantError> {
        let deadline = Instant::now() + self.config.timeout;
        let thread_request_id = self.next_id();
        let mut thread_params = json!({
            "cwd": self.workdir.path(),
            "approvalPolicy": "never",
            "sandbox": "read-only",
            "ephemeral": true,
            "serviceName": "market_terminal",
            "baseInstructions": COMMAND_PLANE_SYSTEM_PROMPT,
            "developerInstructions": "Use only the injected market_terminal_* and portfolio_* dynamic tools. Never use built-in shell, filesystem, web, MCP, connector, or trade tools.",
            "dynamicTools": dynamic_tool_definitions()
        });
        if let Some(model) = &self.config.model {
            thread_params["model"] = json!(model);
        }
        self.server.send(&json!({
            "method": "thread/start",
            "id": thread_request_id,
            "params": thread_params
        }))?;
        let thread = self.server.wait_for_response(thread_request_id, deadline)?;
        let thread_id = thread
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AssistantError::InvalidResponse("Codex did not return a thread id".to_owned())
            })?;

        let turn_request_id = self.next_id();
        self.server.send(&json!({
            "method": "turn/start",
            "id": turn_request_id,
            "params": {
                "threadId": thread_id,
                "input": [{ "type": "text", "text": request_prompt(&request) }]
            }
        }))?;
        let output = self
            .server
            .wait_for_turn(turn_request_id, deadline, &request, &updates)?;
        Ok(AssistantResponse {
            content: output.message,
            actions: output.actions,
            model: self
                .config
                .model
                .clone()
                .or_else(|| Some("codex".to_owned())),
        })
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        id
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn chatgpt_login_status(binary: &Path) -> (bool, String) {
    match Command::new(binary).args(["login", "status"]).output() {
        Ok(output) => {
            let mut status = String::from_utf8_lossy(&output.stdout).into_owned();
            status.push_str(&String::from_utf8_lossy(&output.stderr));
            if output.status.success() && status.contains("Logged in using ChatGPT") {
                (true, "".to_owned())
            } else {
                (
                    false,
                    "RUN CODEX LOGIN AND SIGN IN WITH CHATGPT TO ENABLE AI".to_owned(),
                )
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            false,
            "INSTALL CODEX CLI OR SET CODEX_BIN TO ENABLE AI".to_owned(),
        ),
        Err(_) => (false, "CODEX LOGIN STATUS COULD NOT BE VERIFIED".to_owned()),
    }
}

fn request_prompt(request: &AssistantRequest) -> String {
    let messages = request
        .messages
        .iter()
        .map(|message| {
            let role = match message.role {
                AssistantRole::User => "user",
                AssistantRole::Assistant => "assistant",
                AssistantRole::System => "system",
            };
            json!({ "role": role, "content": message.content })
        })
        .collect::<Vec<_>>();
    json!({
        "instruction": "Answer the final user message. Use the injected read tools for current terminal or portfolio facts. Use action tools only when the user requests an interface change.",
        "active_workspace": request.active_workspace,
        "available_workspaces": request.available_workspaces,
        "conversation": messages
    })
    .to_string()
}

fn dynamic_tool_definitions() -> Value {
    json!([
        dynamic_tool(
            "market_terminal_get_state",
            "Read the active workspace and the exact workspaces currently available in Market Terminal.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false })
        ),
        dynamic_tool(
            "market_terminal_open_workspace",
            "Focus an existing workspace without changing navigation order.",
            target_schema()
        ),
        dynamic_tool(
            "market_terminal_bring_workspace_forward",
            "Move an existing workspace to the front of navigation and focus it.",
            target_schema()
        ),
        dynamic_tool(
            "market_terminal_run_command",
            "Dispatch an existing Market Terminal command through the validated command bar.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "minLength": 1, "maxLength": 512 }
                },
                "required": ["command"],
                "additionalProperties": false
            })
        ),
        dynamic_tool(
            "market_terminal_restore_layout",
            "Restore the default workspace navigation order.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false })
        ),
        dynamic_tool(
            "portfolio_get_positions",
            "Read the user's current imported portfolio summary and positions. This is read-only.",
            json!({
                "type": "object",
                "properties": {
                    "symbols": {
                        "type": "array",
                        "items": { "type": "string" },
                        "maxItems": 20
                    },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                },
                "additionalProperties": false
            })
        ),
        dynamic_tool(
            "portfolio_open_position",
            "Open the Security workspace for a symbol that exists in the user's current portfolio.",
            json!({
                "type": "object",
                "properties": { "symbol": { "type": "string", "minLength": 1, "maxLength": 32 } },
                "required": ["symbol"],
                "additionalProperties": false
            })
        )
    ])
}

fn dynamic_tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn target_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": { "type": "string", "minLength": 1, "maxLength": 64 }
        },
        "required": ["target"],
        "additionalProperties": false
    })
}

struct DynamicToolOutcome {
    success: bool,
    text: String,
    action: Option<UiAction>,
}

impl DynamicToolOutcome {
    fn read(value: Value) -> Self {
        Self {
            success: true,
            text: value.to_string(),
            action: None,
        }
    }

    fn action(message: impl Into<String>, action: UiAction) -> Self {
        Self {
            success: true,
            text: message.into(),
            action: Some(action),
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            text: message.into(),
            action: None,
        }
    }
}

fn execute_dynamic_tool(params: &Value, request: &AssistantRequest) -> DynamicToolOutcome {
    let tool = params["tool"].as_str().unwrap_or_default();
    let arguments = &params["arguments"];
    match tool {
        "market_terminal_get_state" => DynamicToolOutcome::read(json!({
            "active_workspace": request.active_workspace,
            "available_workspaces": request.available_workspaces,
        })),
        "market_terminal_open_workspace" => workspace_action(arguments, request, false),
        "market_terminal_bring_workspace_forward" => workspace_action(arguments, request, true),
        "market_terminal_run_command" => {
            let Some(command) = argument_string(arguments, "command") else {
                return DynamicToolOutcome::error("command is required");
            };
            if command.len() > 512
                || command
                    .chars()
                    .any(|character| matches!(character, '\r' | '\n'))
            {
                return DynamicToolOutcome::error("command is too long or contains a newline");
            }
            DynamicToolOutcome::action(
                format!("Queued validated terminal command: {command}"),
                UiAction::RunCommand { command },
            )
        }
        "market_terminal_restore_layout" => DynamicToolOutcome::action(
            "Queued restoration of the default workspace layout",
            UiAction::RestoreLayout,
        ),
        "portfolio_get_positions" => portfolio_positions(arguments, request),
        "portfolio_open_position" => open_portfolio_position(arguments, request),
        _ => DynamicToolOutcome::error(format!("unsupported Market Terminal tool: {tool}")),
    }
}

fn workspace_action(
    arguments: &Value,
    request: &AssistantRequest,
    bring_forward: bool,
) -> DynamicToolOutcome {
    let Some(target) = argument_string(arguments, "target") else {
        return DynamicToolOutcome::error("target is required");
    };
    let Some(canonical) = request
        .available_workspaces
        .iter()
        .find(|workspace| workspace.eq_ignore_ascii_case(&target))
        .cloned()
    else {
        return DynamicToolOutcome::error(format!(
            "unknown workspace {target}; read market_terminal_get_state for valid IDs"
        ));
    };
    if bring_forward {
        DynamicToolOutcome::action(
            format!("Queued {canonical} to move forward and receive focus"),
            UiAction::BringForward { target: canonical },
        )
    } else {
        DynamicToolOutcome::action(
            format!("Queued {canonical} to receive focus"),
            UiAction::OpenWorkspace { target: canonical },
        )
    }
}

fn portfolio_positions(arguments: &Value, request: &AssistantRequest) -> DynamicToolOutcome {
    let requested_symbols = arguments["symbols"]
        .as_array()
        .map(|symbols| {
            symbols
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|symbol| !symbol.is_empty())
                .map(str::to_ascii_uppercase)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let limit = arguments["limit"].as_u64().unwrap_or(100).clamp(1, 100) as usize;
    let matching = request
        .portfolio
        .positions
        .iter()
        .filter(|position| {
            requested_symbols.is_empty()
                || requested_symbols
                    .iter()
                    .any(|symbol| position.symbol.eq_ignore_ascii_case(symbol))
        })
        .collect::<Vec<_>>();
    let matching_count = matching.len();
    let returned = matching_count.min(limit);
    let positions = matching
        .into_iter()
        .take(limit)
        .map(|position| {
            json!({
                "instrument_id": position.instrument_id.as_str(),
                "account": position.account_id.as_str(),
                "symbol": position.symbol,
                "quantity": position.quantity_label(),
                "average_cost": position.average_cost_label(),
                "market_value": position.market_value_label(),
                "currency": position.currency().to_string(),
                "pnl": position.pnl_label(),
                "weight": position.weight_label(),
            })
        })
        .collect::<Vec<_>>();
    DynamicToolOutcome::read(json!({
        "source": request.portfolio.source,
        "as_of": request.portfolio.as_of,
        "input_version": request.portfolio.input_version,
        "methodology": request.portfolio.methodology,
        "disclosures": request.portfolio.disclosures,
        "net_asset_value": request.portfolio.net_asset_value_label(),
        "available_cash": request.portfolio.available_cash_label(),
        "ytd_return": request.portfolio.ytd_return_label(),
        "sharpe": request.portfolio.sharpe_label(),
        "total_position_count": request.portfolio.positions.len(),
        "matching_position_count": matching_count,
        "returned_position_count": returned,
        "truncated": matching_count > returned,
        "positions": positions,
    }))
}

fn open_portfolio_position(arguments: &Value, request: &AssistantRequest) -> DynamicToolOutcome {
    let Some(symbol) = argument_string(arguments, "symbol") else {
        return DynamicToolOutcome::error("symbol is required");
    };
    let Some(position) = request
        .portfolio
        .positions
        .iter()
        .find(|position| position.symbol.eq_ignore_ascii_case(&symbol))
    else {
        return DynamicToolOutcome::error(format!("{symbol} is not in the current portfolio"));
    };
    if position.symbol.len() > 32
        || !position
            .symbol
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-^/".contains(character))
    {
        return DynamicToolOutcome::error("the held symbol is not safe to open as a command");
    }
    DynamicToolOutcome::action(
        format!("Queued the Security workspace for {}", position.symbol),
        UiAction::RunCommand {
            command: format!("SEC {}", position.symbol),
        },
    )
}

fn argument_string(arguments: &Value, name: &str) -> Option<String> {
    arguments[name]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

struct CodexTurnOutput {
    message: String,
    actions: Vec<UiAction>,
}

struct EphemeralWorkdir(PathBuf);

impl EphemeralWorkdir {
    fn new() -> std::io::Result<Self> {
        for _ in 0..16 {
            let id = NEXT_WORKDIR_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("market-terminal-codex-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create an isolated Codex working directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for EphemeralWorkdir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct AppServerProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Result<Value, String>>,
    stdout_thread: Option<thread::JoinHandle<()>>,
    stderr_thread: Option<thread::JoinHandle<String>>,
}

impl AppServerProcess {
    fn spawn(binary: &Path, cwd: &Path) -> Result<Self, AssistantError> {
        let mut command = Command::new(binary);
        command
            .args(["app-server", "--stdio"])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.env_clear();
        for name in [
            "HOME",
            "PATH",
            "CODEX_HOME",
            "USER",
            "LOGNAME",
            "TMPDIR",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
        ] {
            if let Some(value) = env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command.spawn().map_err(transport)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AssistantError::Transport("Codex stdin unavailable".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AssistantError::Transport("Codex stdout unavailable".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AssistantError::Transport("Codex stderr unavailable".to_owned()))?;
        let (sender, messages) = mpsc::channel();
        let stdout_thread = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let message = line.map_err(|error| error.to_string()).and_then(|line| {
                    serde_json::from_str(&line).map_err(|error| error.to_string())
                });
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        let stderr_thread = thread::spawn(move || bounded_read(stderr));
        Ok(Self {
            child,
            stdin: Some(stdin),
            messages,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
        })
    }

    fn send(&mut self, message: &Value) -> Result<(), AssistantError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AssistantError::Transport("Codex stdin closed".to_owned()))?;
        serde_json::to_writer(&mut *stdin, message)
            .map_err(|error| AssistantError::Transport(error.to_string()))?;
        stdin.write_all(b"\n").map_err(transport)?;
        stdin.flush().map_err(transport)
    }

    fn receive(&self, deadline: Instant) -> Result<Value, AssistantError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AssistantError::Transport(
                "Codex request timed out".to_owned(),
            ));
        }
        match self.messages.recv_timeout(remaining) {
            Ok(Ok(message)) => Ok(message),
            Ok(Err(error)) => Err(AssistantError::InvalidResponse(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AssistantError::Transport(
                "Codex request timed out".to_owned(),
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AssistantError::Transport(
                "Codex app server disconnected".to_owned(),
            )),
        }
    }

    fn wait_for_response(&mut self, id: u64, deadline: Instant) -> Result<Value, AssistantError> {
        loop {
            let message = self.receive(deadline)?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                self.reject_server_request(&message)?;
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(AssistantError::Provider(rpc_error(error)));
            }
            return message.get("result").cloned().ok_or_else(|| {
                AssistantError::InvalidResponse("missing JSON-RPC result".to_owned())
            });
        }
    }

    fn wait_for_turn(
        &mut self,
        request_id: u64,
        deadline: Instant,
        request: &AssistantRequest,
        updates: &Sender<AssistantStreamEvent>,
    ) -> Result<CodexTurnOutput, AssistantError> {
        let mut final_message = None;
        let mut turn_error = None;
        let mut actions = Vec::new();
        let mut commentary_items = Vec::new();
        loop {
            let message = self.receive(deadline)?;
            if message.get("method").and_then(Value::as_str) == Some("item/tool/call") {
                self.handle_dynamic_tool_call(&message, request, updates, &mut actions)?;
                continue;
            }
            self.reject_server_request(&message)?;
            if message.get("id").and_then(Value::as_u64) == Some(request_id) {
                if let Some(error) = message.get("error") {
                    return Err(AssistantError::Provider(rpc_error(error)));
                }
                continue;
            }
            match message.get("method").and_then(Value::as_str) {
                Some("item/started") => {
                    let item = &message["params"]["item"];
                    if item["type"] == "agentMessage"
                        && item["phase"].as_str() == Some("commentary")
                    {
                        if let Some(item_id) = item["id"].as_str() {
                            commentary_items.push(item_id.to_owned());
                        }
                    }
                }
                Some("item/agentMessage/delta") => {
                    let params = &message["params"];
                    let is_commentary = params["itemId"]
                        .as_str()
                        .is_some_and(|item_id| commentary_items.iter().any(|id| id == item_id));
                    if !is_commentary {
                        if let Some(delta) =
                            params["delta"].as_str().filter(|delta| !delta.is_empty())
                        {
                            let _ = updates.send(AssistantStreamEvent::TextDelta(delta.to_owned()));
                        }
                    }
                }
                Some("thread/tokenUsage/updated") => {
                    if let Some(usage) = token_usage(&message) {
                        let _ = updates.send(AssistantStreamEvent::TokenUsage(usage));
                    }
                }
                Some("item/completed") => {
                    let item = &message["params"]["item"];
                    if item["type"] == "agentMessage"
                        && item["phase"].as_str() != Some("commentary")
                    {
                        final_message = item["text"].as_str().map(str::to_owned);
                    }
                }
                Some("error") => {
                    turn_error = message
                        .pointer("/params/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                Some("turn/completed") => {
                    let turn = &message["params"]["turn"];
                    if turn["status"] != "completed" {
                        let message = turn
                            .pointer("/error/message")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .or(turn_error)
                            .unwrap_or_else(|| "Codex turn did not complete".to_owned());
                        return Err(AssistantError::Provider(message));
                    }
                    let message = final_message.ok_or_else(|| {
                        AssistantError::InvalidResponse(
                            "Codex completed without a final agent message".to_owned(),
                        )
                    })?;
                    return Ok(CodexTurnOutput { message, actions });
                }
                _ => {}
            }
        }
    }

    fn handle_dynamic_tool_call(
        &mut self,
        message: &Value,
        request: &AssistantRequest,
        updates: &Sender<AssistantStreamEvent>,
        actions: &mut Vec<UiAction>,
    ) -> Result<(), AssistantError> {
        let Some(id) = message.get("id").cloned() else {
            return Err(AssistantError::InvalidResponse(
                "Codex dynamic tool call did not include an id".to_owned(),
            ));
        };
        let params = &message["params"];
        let name = params["tool"].as_str().unwrap_or("unknown").to_owned();
        let _ = updates.send(AssistantStreamEvent::ToolStarted(name.clone()));
        let mut outcome = execute_dynamic_tool(params, request);
        if outcome.action.is_some() && actions.len() >= 8 {
            outcome = DynamicToolOutcome::error("too many UI actions requested in one turn");
        }
        if let Some(action) = outcome.action.take() {
            actions.push(action);
        }
        self.send(&json!({
            "id": id,
            "result": {
                "success": outcome.success,
                "contentItems": [{ "type": "inputText", "text": outcome.text }]
            }
        }))?;
        let _ = updates.send(AssistantStreamEvent::ToolFinished {
            name,
            success: outcome.success,
        });
        Ok(())
    }

    fn reject_server_request(&mut self, message: &Value) -> Result<(), AssistantError> {
        if message.get("method").is_some() {
            if let Some(id) = message.get("id") {
                self.send(&json!({
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": "Market Terminal does not allow Codex tool requests"
                    }
                }))?;
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
        let _ = self
            .stderr_thread
            .take()
            .and_then(|handle| handle.join().ok());
    }
}

fn token_usage(message: &Value) -> Option<AssistantTokenUsage> {
    let usage = message.pointer("/params/tokenUsage/last")?;
    Some(AssistantTokenUsage {
        input_tokens: usage["inputTokens"].as_u64()?,
        cached_input_tokens: usage["cachedInputTokens"].as_u64()?,
        output_tokens: usage["outputTokens"].as_u64()?,
        reasoning_output_tokens: usage["reasoningOutputTokens"].as_u64()?,
        total_tokens: usage["totalTokens"].as_u64()?,
    })
}

impl Drop for AppServerProcess {
    fn drop(&mut self) {
        self.shutdown()
    }
}

fn bounded_read(mut reader: impl Read) -> String {
    let mut captured = Vec::new();
    let mut chunk = [0_u8; 1_024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(length) if captured.len() < MAX_DIAGNOSTIC_BYTES => {
                let remaining = MAX_DIAGNOSTIC_BYTES - captured.len();
                captured.extend_from_slice(&chunk[..length.min(remaining)]);
            }
            Ok(_) => {}
        }
    }
    String::from_utf8_lossy(&captured).trim().to_owned()
}

fn rpc_error(error: &Value) -> String {
    error["message"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| error.to_string())
}

fn transport(error: std::io::Error) -> AssistantError {
    AssistantError::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::{
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn request_with_portfolio() -> AssistantRequest {
        let usd = crate::foundation::Currency::new("USD").unwrap();
        let mut portfolio = crate::features::portfolio::PortfolioSnapshot::empty("TEST");
        portfolio
            .positions
            .push(crate::features::portfolio::Position {
                instrument_id: crate::foundation::InstrumentId::new("us:xnas:aapl"),
                account_id: crate::features::portfolio::PortfolioAccountId::new("ACCOUNT 1"),
                symbol: "AAPL".to_owned(),
                currency: usd,
                quantity: crate::features::portfolio::PositionQuantity::from_scaled_units(
                    10_000_000,
                ),
                average_cost: Some(crate::foundation::Money::from_minor_units(15_000, usd)),
                market_value: Some(crate::foundation::Money::from_minor_units(200_000, usd)),
                unrealized_return_bps: Some(3_333),
                weight_bps: Some(2_500),
                cash: false,
            });
        portfolio.currency_totals = vec![crate::features::portfolio::PortfolioCurrencyTotal {
            currency: usd,
            net_asset_value: crate::foundation::Money::from_minor_units(800_000, usd),
            available_cash: crate::foundation::Money::from_minor_units(100_000, usd),
            priced_positions: 1,
            unpriced_positions: 0,
        }];
        portfolio.ytd_return_bps = Some(500);
        portfolio.sharpe_hundredths = Some(120);
        AssistantRequest {
            messages: vec![crate::features::assistant::domain::AssistantMessage::user(
                "hello",
            )],
            active_workspace: "overview".to_owned(),
            available_workspaces: vec!["overview".to_owned(), "portfolio".to_owned()],
            portfolio,
        }
    }

    #[test]
    fn dynamic_tools_read_assets_and_queue_validated_ui_actions() {
        let request = request_with_portfolio();
        let read = execute_dynamic_tool(
            &json!({ "tool": "portfolio_get_positions", "arguments": {} }),
            &request,
        );
        assert!(read.success);
        assert!(read.text.contains("AAPL"));
        assert_eq!(read.action, None);

        let open = execute_dynamic_tool(
            &json!({
                "tool": "portfolio_open_position",
                "arguments": { "symbol": "aapl" }
            }),
            &request,
        );
        assert_eq!(
            open.action,
            Some(UiAction::RunCommand {
                command: "SEC AAPL".to_owned()
            })
        );

        let unknown = execute_dynamic_tool(
            &json!({
                "tool": "market_terminal_open_workspace",
                "arguments": { "target": "invented" }
            }),
            &request,
        );
        assert!(!unknown.success);
        assert_eq!(dynamic_tool_definitions().as_array().map(Vec::len), Some(7));
    }

    #[cfg(unix)]
    #[test]
    fn reuses_one_app_server_process_for_multiple_requests() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = env::temp_dir().join(format!("market-terminal-codex-worker-{unique}"));
        fs::create_dir(&directory).unwrap();
        let binary = directory.join("fake-codex");
        let launches = directory.join("launches");
        let script = format!(
            r#"#!/bin/sh
printf 'launch\n' >> '{}'
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"id":%s,"result":{{}}}}\n' "$id"
      ;;
    *'"method":"thread/start"'*)
      printf '{{"id":%s,"result":{{"thread":{{"id":"test-thread"}}}}}}\n' "$id"
      ;;
    *'"method":"turn/start"'*)
      printf '{{"id":%s,"result":{{}}}}\n' "$id"
      printf '%s\n' '{{"method":"item/started","params":{{"item":{{"id":"answer-1","type":"agentMessage","phase":"final_answer","text":""}}}}}}'
      printf '%s\n' '{{"method":"item/agentMessage/delta","params":{{"threadId":"test-thread","turnId":"test-turn","itemId":"answer-1","delta":"ok"}}}}'
      printf '%s\n' '{{"method":"thread/tokenUsage/updated","params":{{"threadId":"test-thread","turnId":"test-turn","tokenUsage":{{"last":{{"cachedInputTokens":2,"inputTokens":10,"outputTokens":3,"reasoningOutputTokens":1,"totalTokens":13}},"total":{{"cachedInputTokens":2,"inputTokens":10,"outputTokens":3,"reasoningOutputTokens":1,"totalTokens":13}},"modelContextWindow":1000}}}}}}'
      printf '%s\n' '{{"method":"item/completed","params":{{"item":{{"id":"answer-1","type":"agentMessage","phase":"final_answer","text":"ok"}}}}}}'
      printf '%s\n' '{{"method":"turn/completed","params":{{"turn":{{"status":"completed"}}}}}}'
      ;;
  esac
done
"#,
            launches.display()
        );
        fs::write(&binary, script).unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&binary, permissions).unwrap();
        let gateway = CodexAppServerGateway::new(CodexAppServerConfig {
            binary,
            model: Some("test-model".to_owned()),
            timeout: Duration::from_secs(2),
            configured: true,
            configuration_hint: String::new(),
        });
        let request = request_with_portfolio();

        assert_eq!(gateway.complete(request.clone()).unwrap().content, "ok");
        assert_eq!(gateway.complete(request).unwrap().content, "ok");

        assert_eq!(fs::read_to_string(&launches).unwrap().lines().count(), 1);
        drop(gateway);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires a local Codex CLI signed in with ChatGPT"]
    fn live_chatgpt_subscription_streams_usage_and_dynamic_tools() {
        let gateway = CodexAppServerGateway::new(CodexAppServerConfig::live());
        let (updates, received_updates) = mpsc::channel();
        let response = gateway
            .complete_stream(
                AssistantRequest {
                messages: vec![crate::features::assistant::domain::AssistantMessage::user(
                    "Use the portfolio tools to identify my only holding, then open that held position.",
                )],
                active_workspace: "overview".to_owned(),
                available_workspaces: vec!["overview".to_owned(), "portfolio".to_owned()],
                portfolio: request_with_portfolio().portfolio,
                },
                updates,
            )
            .expect("ChatGPT-backed Codex completion");

        assert_eq!(
            response.actions,
            vec![UiAction::RunCommand {
                command: "SEC AAPL".to_owned()
            }]
        );
        assert!(response.content.contains("AAPL"));
        let events = received_updates.try_iter().collect::<Vec<_>>();
        assert!(events.iter().any(|event| matches!(
            event,
            AssistantStreamEvent::ToolStarted(name) if name == "portfolio_get_positions"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AssistantStreamEvent::TextDelta(delta) if !delta.is_empty()
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AssistantStreamEvent::TokenUsage(usage) if usage.output_tokens > 0
        )));
    }
}
