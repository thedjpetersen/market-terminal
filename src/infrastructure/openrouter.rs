use std::{env, sync::mpsc::Sender, time::Duration};

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::features::assistant::{
    domain::{
        AssistantRequest, AssistantResponse, AssistantRole, AssistantStreamEvent,
        AssistantTokenUsage, UiAction, COMMAND_PLANE_SYSTEM_PROMPT,
    },
    AssistantError, AssistantGateway,
};

const DEFAULT_ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";
const DEFAULT_MODEL: &str = "openrouter/auto";

#[derive(Clone)]
pub struct OpenRouterConfig {
    pub api_key: Option<String>,
    pub model: String,
    pub endpoint: String,
    pub timeout: Duration,
}

impl OpenRouterConfig {
    pub fn from_env() -> Self {
        Self {
            api_key: env::var("OPENROUTER_API_KEY")
                .ok()
                .filter(|key| !key.trim().is_empty()),
            model: env::var("OPENROUTER_MODEL")
                .ok()
                .filter(|model| !model.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_MODEL.to_owned()),
            endpoint: env::var("OPENROUTER_BASE_URL")
                .ok()
                .filter(|endpoint| !endpoint.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned()),
            timeout: Duration::from_secs(45),
        }
    }
}

pub struct OpenRouterGateway {
    config: OpenRouterConfig,
}

impl OpenRouterGateway {
    pub fn new(config: OpenRouterConfig) -> Self {
        Self { config }
    }

    fn request_body(&self, request: AssistantRequest) -> Value {
        let workspace_catalog = request.available_workspaces.join(", ");
        let portfolio = portfolio_context(&request);
        let mut messages = vec![json!({
            "role": "system",
            "content": format!(
                "{COMMAND_PLANE_SYSTEM_PROMPT}\nFor this provider, the portfolio snapshot is supplied directly below; do not request a separate read tool.\nCurrent workspace: {}\nWorkspace order: {workspace_catalog}\nCurrent read-only portfolio snapshot: {portfolio}",
                request.active_workspace
            )
        })];
        messages.extend(request.messages.into_iter().map(|message| {
            let role = match message.role {
                AssistantRole::User => "user",
                AssistantRole::Assistant => "assistant",
                AssistantRole::System => "system",
            };
            json!({ "role": role, "content": message.content })
        }));

        json!({
            "model": self.config.model,
            "messages": messages,
            "tools": tool_definitions(),
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "max_tokens": 700,
            "temperature": 0.2
        })
    }

    #[cfg(test)]
    fn parse_response(body: &str) -> Result<AssistantResponse, AssistantError> {
        Ok(Self::parse_completion(body)?.response)
    }

    fn parse_completion(body: &str) -> Result<OpenRouterCompletion, AssistantError> {
        let response: ChatCompletion = serde_json::from_str(body)
            .map_err(|error| AssistantError::InvalidResponse(error.to_string()))?;
        if let Some(error) = response.error {
            return Err(AssistantError::Provider(error.message));
        }
        let choice = response.choices.into_iter().next().ok_or_else(|| {
            AssistantError::InvalidResponse("missing completion choice".to_owned())
        })?;
        let actions = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(parse_tool_call)
            .collect::<Result<Vec<_>, _>>()?;

        let usage = response.usage.map(Usage::into_assistant_usage);
        Ok(OpenRouterCompletion {
            response: AssistantResponse {
                content: choice.message.content.unwrap_or_default(),
                actions,
                model: response.model,
            },
            usage,
        })
    }

    fn send_request(
        &self,
        request: AssistantRequest,
    ) -> Result<OpenRouterCompletion, AssistantError> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(AssistantError::NotConfigured)?;
        let client = Client::builder()
            .timeout(self.config.timeout)
            .build()
            .map_err(|error| AssistantError::Transport(error.to_string()))?;
        let response = client
            .post(&self.config.endpoint)
            .bearer_auth(api_key)
            .header(
                "HTTP-Referer",
                "https://github.com/thedjpetersen/market-terminal",
            )
            .header("X-OpenRouter-Title", "Market Terminal")
            .json(&self.request_body(request))
            .send()
            .map_err(|error| AssistantError::Transport(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|error| AssistantError::Transport(error.to_string()))?;
        if !status.is_success() {
            let message = serde_json::from_str::<ProviderFailure>(&body)
                .ok()
                .map(|failure| failure.error.message)
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(AssistantError::Provider(message));
        }
        Self::parse_completion(&body)
    }
}

impl AssistantGateway for OpenRouterGateway {
    fn complete(&self, request: AssistantRequest) -> Result<AssistantResponse, AssistantError> {
        Ok(self.send_request(request)?.response)
    }

    fn complete_stream(
        &self,
        request: AssistantRequest,
        updates: Sender<AssistantStreamEvent>,
    ) -> Result<AssistantResponse, AssistantError> {
        let completion = self.send_request(request)?;
        if let Some(usage) = completion.usage {
            let _ = updates.send(AssistantStreamEvent::TokenUsage(usage));
        }
        if !completion.response.content.is_empty() {
            let _ = updates.send(AssistantStreamEvent::TextDelta(
                completion.response.content.clone(),
            ));
        }
        Ok(completion.response)
    }

    fn model_label(&self) -> &str {
        &self.config.model
    }

    fn is_configured(&self) -> bool {
        self.config.api_key.is_some()
    }

    fn configuration_hint(&self) -> &str {
        "SET OPENROUTER_API_KEY TO ENABLE AI"
    }
}

fn portfolio_context(request: &AssistantRequest) -> String {
    let positions = request
        .portfolio
        .positions
        .iter()
        .take(100)
        .map(|position| {
            json!({
                "symbol": position.symbol,
                "quantity": position.quantity,
                "average_cost": position.average_cost,
                "market_value": position.market_value,
                "pnl": position.pnl,
                "weight": position.weight,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "source": request.portfolio.source,
        "as_of": request.portfolio.as_of,
        "net_asset_value": request.portfolio.net_asset_value,
        "available_cash": request.portfolio.available_cash,
        "ytd_return": request.portfolio.ytd_return,
        "sharpe": request.portfolio.sharpe,
        "position_count": request.portfolio.positions.len(),
        "positions": positions,
        "truncated": request.portfolio.positions.len() > 100,
    })
    .to_string()
}

fn parse_tool_call(call: ToolCall) -> Result<UiAction, AssistantError> {
    match call.function.name.as_str() {
        "open_workspace" => {
            let arguments: TargetArguments = parse_arguments(&call.function.arguments)?;
            Ok(UiAction::OpenWorkspace {
                target: arguments.target,
            })
        }
        "bring_workspace_forward" => {
            let arguments: TargetArguments = parse_arguments(&call.function.arguments)?;
            Ok(UiAction::BringForward {
                target: arguments.target,
            })
        }
        "run_terminal_command" => {
            let arguments: CommandArguments = parse_arguments(&call.function.arguments)?;
            Ok(UiAction::RunCommand {
                command: arguments.command,
            })
        }
        "restore_workspace_layout" => Ok(UiAction::RestoreLayout),
        unknown => Err(AssistantError::InvalidResponse(format!(
            "unsupported tool call: {unknown}"
        ))),
    }
}

fn parse_arguments<T: for<'de> Deserialize<'de>>(arguments: &str) -> Result<T, AssistantError> {
    serde_json::from_str(arguments)
        .map_err(|error| AssistantError::InvalidResponse(error.to_string()))
}

fn tool_definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "open_workspace",
                "description": "Focus an existing terminal workspace without changing navigation order.",
                "parameters": {
                    "type": "object",
                    "properties": { "target": { "type": "string", "description": "Exact workspace ID, label, or command alias." } },
                    "required": ["target"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "bring_workspace_forward",
                "description": "Move an existing workspace to the front of navigation and focus it.",
                "parameters": {
                    "type": "object",
                    "properties": { "target": { "type": "string", "description": "Exact workspace ID, label, or command alias." } },
                    "required": ["target"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "run_terminal_command",
                "description": "Run an existing terminal command, including its subject and qualifiers.",
                "parameters": {
                    "type": "object",
                    "properties": { "command": { "type": "string", "description": "A command accepted by the terminal command bar." } },
                    "required": ["command"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "restore_workspace_layout",
                "description": "Restore the default terminal workspace navigation order.",
                "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
            }
        }
    ])
}

#[derive(Deserialize)]
struct ChatCompletion {
    #[serde(default)]
    choices: Vec<Choice>,
    model: Option<String>,
    error: Option<ProviderError>,
    usage: Option<Usage>,
}

struct OpenRouterCompletion {
    response: AssistantResponse,
    usage: Option<AssistantTokenUsage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
    prompt_tokens_details: Option<PromptTokenDetails>,
    completion_tokens_details: Option<CompletionTokenDetails>,
}

impl Usage {
    fn into_assistant_usage(self) -> AssistantTokenUsage {
        AssistantTokenUsage {
            input_tokens: self.prompt_tokens,
            cached_input_tokens: self
                .prompt_tokens_details
                .map_or(0, |details| details.cached_tokens),
            output_tokens: self.completion_tokens,
            reasoning_output_tokens: self
                .completion_tokens_details
                .map_or(0, |details| details.reasoning_tokens),
            total_tokens: self.total_tokens,
        }
    }
}

#[derive(Deserialize)]
struct PromptTokenDetails {
    #[serde(default)]
    cached_tokens: u64,
}

#[derive(Deserialize)]
struct CompletionTokenDetails {
    #[serde(default)]
    reasoning_tokens: u64,
}

#[derive(Deserialize)]
struct Choice {
    message: CompletionMessage,
}

#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Deserialize)]
struct ToolCall {
    function: FunctionCall,
}

#[derive(Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct TargetArguments {
    target: String,
}

#[derive(Deserialize)]
struct CommandArguments {
    command: String,
}

#[derive(Deserialize)]
struct ProviderFailure {
    error: ProviderError,
}

#[derive(Deserialize)]
struct ProviderError {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_whitelisted_ui_tool_calls() {
        let response = OpenRouterGateway::parse_response(
            r#"{
                "model":"openai/test",
                "choices":[{"message":{
                    "content":"Opening the portfolio.",
                    "tool_calls":[{"function":{
                        "name":"bring_workspace_forward",
                        "arguments":"{\"target\":\"portfolio\"}"
                    }}]
                }}]
            }"#,
        )
        .expect("valid response");

        assert_eq!(response.model.as_deref(), Some("openai/test"));
        assert_eq!(
            response.actions,
            vec![UiAction::BringForward {
                target: "portfolio".to_owned()
            }]
        );
    }

    #[test]
    fn rejects_unknown_tool_calls() {
        let error = OpenRouterGateway::parse_response(
            r#"{"choices":[{"message":{"content":null,"tool_calls":[{"function":{"name":"shell","arguments":"{}"}}]}}]}"#,
        )
        .expect_err("unknown tools must be rejected");

        assert!(matches!(error, AssistantError::InvalidResponse(_)));
    }

    #[test]
    fn request_includes_available_workspaces_and_tools() {
        let gateway = OpenRouterGateway::new(OpenRouterConfig {
            api_key: Some("test".to_owned()),
            model: "test/model".to_owned(),
            endpoint: "http://localhost".to_owned(),
            timeout: Duration::from_secs(1),
        });
        let body = gateway.request_body(AssistantRequest {
            messages: vec![crate::features::assistant::domain::AssistantMessage::user(
                "show markets",
            )],
            active_workspace: "overview".to_owned(),
            available_workspaces: vec!["overview".to_owned(), "markets".to_owned()],
            portfolio: crate::features::portfolio::PortfolioSnapshot::empty("TEST"),
        });

        assert_eq!(body["model"], "test/model");
        assert_eq!(body["parallel_tool_calls"], false);
        assert_eq!(body["tools"].as_array().map(Vec::len), Some(4));
        assert!(body["messages"][0]["content"]
            .as_str()
            .is_some_and(|content| content.contains("overview, markets")));
    }
}
