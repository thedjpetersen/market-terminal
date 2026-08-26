use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::features::assistant::{
    AssistantError, AssistantGateway,
    domain::{
        AssistantRequest, AssistantResponse, AssistantRole, COMMAND_PLANE_SYSTEM_PROMPT, UiAction,
    },
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

    fn parse_output(
        output: &str,
        model: Option<String>,
    ) -> Result<AssistantResponse, AssistantError> {
        let output: CodexOutput = serde_json::from_str(output)
            .map_err(|error| AssistantError::InvalidResponse(error.to_string()))?;
        let actions = output
            .actions
            .into_iter()
            .map(CodexAction::into_ui_action)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AssistantResponse {
            content: output.content,
            actions,
            model: model.or_else(|| Some("codex".to_owned())),
        })
    }
}

impl AssistantGateway for CodexAppServerGateway {
    fn complete(&self, request: AssistantRequest) -> Result<AssistantResponse, AssistantError> {
        if !self.config.configured {
            return Err(AssistantError::NotConfigured);
        }
        let worker = self.worker.as_ref().ok_or_else(|| {
            AssistantError::Transport("Codex background worker could not start".to_owned())
        })?;
        worker.complete(
            request,
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
        request: AssistantRequest,
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
        timeout: Duration,
    ) -> Result<AssistantResponse, AssistantError> {
        let (reply, response) = mpsc::sync_channel(1);
        self.sender
            .send(CodexWorkerMessage::Complete { request, reply })
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
            CodexWorkerMessage::Complete { request, reply } => {
                let _ = reply.send(complete_with_restart(&config, &mut session, request));
            }
            CodexWorkerMessage::Shutdown => break,
        }
    }
}

fn complete_with_restart(
    config: &CodexAppServerConfig,
    session: &mut Option<CodexSession>,
    request: AssistantRequest,
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
            .complete(request.clone());
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
                    "optOutNotificationMethods": [
                        "item/agentMessage/delta",
                        "item/reasoning/summaryTextDelta",
                        "item/reasoning/textDelta",
                        "thread/tokenUsage/updated"
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

    fn complete(&mut self, request: AssistantRequest) -> Result<AssistantResponse, AssistantError> {
        let deadline = Instant::now() + self.config.timeout;
        let thread_request_id = self.next_id();
        let mut thread_params = json!({
            "cwd": self.workdir.path(),
            "approvalPolicy": "never",
            "sandbox": "read-only",
            "ephemeral": true,
            "serviceName": "market_terminal",
            "baseInstructions": COMMAND_PLANE_SYSTEM_PROMPT,
            "developerInstructions": "Return only the requested structured response. Do not invoke any agent tools."
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
                "input": [{ "type": "text", "text": request_prompt(&request) }],
                "outputSchema": output_schema()
            }
        }))?;
        let output = self.server.wait_for_turn(turn_request_id, deadline)?;
        CodexAppServerGateway::parse_output(&output, self.config.model.clone())
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
        "instruction": "Answer the final user message. Return UI actions only when the user requested them.",
        "active_workspace": request.active_workspace,
        "available_workspaces": request.available_workspaces,
        "conversation": messages
    })
    .to_string()
}

fn output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "content": { "type": "string" },
            "actions": {
                "type": "array",
                "maxItems": 4,
                "items": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": [
                                "open_workspace",
                                "bring_workspace_forward",
                                "run_terminal_command",
                                "restore_workspace_layout"
                            ]
                        },
                        "target": { "type": ["string", "null"] },
                        "command": { "type": ["string", "null"] }
                    },
                    "required": ["type", "target", "command"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["content", "actions"],
        "additionalProperties": false
    })
}

#[derive(Deserialize)]
struct CodexOutput {
    content: String,
    actions: Vec<CodexAction>,
}

#[derive(Deserialize)]
struct CodexAction {
    #[serde(rename = "type")]
    kind: String,
    target: Option<String>,
    command: Option<String>,
}

impl CodexAction {
    fn into_ui_action(self) -> Result<UiAction, AssistantError> {
        match self.kind.as_str() {
            "open_workspace" => Ok(UiAction::OpenWorkspace {
                target: required(self.target, "open_workspace target")?,
            }),
            "bring_workspace_forward" => Ok(UiAction::BringForward {
                target: required(self.target, "bring_workspace_forward target")?,
            }),
            "run_terminal_command" => Ok(UiAction::RunCommand {
                command: required(self.command, "run_terminal_command command")?,
            }),
            "restore_workspace_layout" => Ok(UiAction::RestoreLayout),
            unknown => Err(AssistantError::InvalidResponse(format!(
                "unsupported Codex action: {unknown}"
            ))),
        }
    }
}

fn required(value: Option<String>, field: &str) -> Result<String, AssistantError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AssistantError::InvalidResponse(format!("missing {field}")))
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
    ) -> Result<String, AssistantError> {
        let mut final_message = None;
        let mut turn_error = None;
        loop {
            let message = self.receive(deadline)?;
            self.reject_server_request(&message)?;
            if message.get("id").and_then(Value::as_u64) == Some(request_id) {
                if let Some(error) = message.get("error") {
                    return Err(AssistantError::Provider(rpc_error(error)));
                }
                continue;
            }
            match message.get("method").and_then(Value::as_str) {
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
                    return final_message.ok_or_else(|| {
                        AssistantError::InvalidResponse(
                            "Codex completed without a final agent message".to_owned(),
                        )
                    });
                }
                _ => {}
            }
        }
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

    #[test]
    fn parses_structured_codex_actions() {
        let response = CodexAppServerGateway::parse_output(
            r#"{
                "content":"Opening the portfolio.",
                "actions":[{
                    "type":"bring_workspace_forward",
                    "target":"portfolio",
                    "command":null
                }]
            }"#,
            Some("test-model".to_owned()),
        )
        .expect("valid response");

        assert_eq!(response.model.as_deref(), Some("test-model"));
        assert_eq!(
            response.actions,
            vec![UiAction::BringForward {
                target: "portfolio".to_owned()
            }]
        );
    }

    #[test]
    fn rejects_actions_missing_required_arguments() {
        let error = CodexAppServerGateway::parse_output(
            r#"{
                "content":"",
                "actions":[{
                    "type":"open_workspace",
                    "target":null,
                    "command":null
                }]
            }"#,
            None,
        )
        .expect_err("missing target must fail");

        assert!(matches!(error, AssistantError::InvalidResponse(_)));
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
      printf '%s\n' '{{"method":"item/completed","params":{{"item":{{"type":"agentMessage","phase":"final_answer","text":"{{\"content\":\"ok\",\"actions\":[]}}"}}}}}}'
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
        let request = AssistantRequest {
            messages: vec![crate::features::assistant::domain::AssistantMessage::user(
                "hello",
            )],
            active_workspace: "overview".to_owned(),
            available_workspaces: vec!["overview".to_owned()],
        };

        assert_eq!(gateway.complete(request.clone()).unwrap().content, "ok");
        assert_eq!(gateway.complete(request).unwrap().content, "ok");

        assert_eq!(fs::read_to_string(&launches).unwrap().lines().count(), 1);
        drop(gateway);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires a local Codex CLI signed in with ChatGPT"]
    fn live_chatgpt_subscription_tool_call() {
        let gateway = CodexAppServerGateway::new(CodexAppServerConfig::live());
        let response = gateway
            .complete(AssistantRequest {
                messages: vec![crate::features::assistant::domain::AssistantMessage::user(
                    "Open the portfolio workspace.",
                )],
                active_workspace: "overview".to_owned(),
                available_workspaces: vec!["overview".to_owned(), "portfolio".to_owned()],
            })
            .expect("ChatGPT-backed Codex completion");

        assert_eq!(
            response.actions,
            vec![UiAction::OpenWorkspace {
                target: "portfolio".to_owned()
            }]
        );
    }
}
