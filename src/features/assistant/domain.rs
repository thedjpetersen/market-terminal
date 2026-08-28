pub(crate) const COMMAND_PLANE_SYSTEM_PROMPT: &str = "You are the command plane for a native financial terminal. \
Answer financial and product questions concisely. Use the supplied Market Terminal tools when the answer \
depends on the current interface or portfolio, and use only those tools when the user asks to change the UI. \
Never invent a workspace, command, position, or account value. Prefer the supplied bring-forward action \
when the user asks to prioritize, foreground, rearrange, or put a feature first. Prefer the supplied \
open-workspace action when they only ask to view a feature. Do not invoke shell, filesystem, web, \
connector, MCP, or any other agent tools. Portfolio access is read-only: you cannot access credentials, \
submit trades, or perform external side effects.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantMessage {
    pub role: AssistantRole,
    pub content: String,
}

impl AssistantMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: AssistantRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: AssistantRole::Assistant,
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: AssistantRole::System,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    OpenWorkspace { target: String },
    BringForward { target: String },
    RunCommand { command: String },
    RestoreLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantPosition {
    pub instrument_id: String,
    pub account: String,
    pub symbol: String,
    pub quantity: String,
    pub average_cost: String,
    pub market_value: String,
    pub currency: String,
    pub pnl: String,
    pub weight: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantPortfolioSnapshot {
    pub source: String,
    pub as_of: String,
    pub input_version: String,
    pub methodology: String,
    pub disclosures: Vec<String>,
    pub net_asset_value: String,
    pub available_cash: String,
    pub ytd_return: String,
    pub sharpe: String,
    pub positions: Vec<AssistantPosition>,
}

impl AssistantPortfolioSnapshot {
    pub fn unavailable(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            as_of: "—".to_owned(),
            input_version: "—".to_owned(),
            methodology: "NO PORTFOLIO CONTEXT".to_owned(),
            disclosures: vec!["PORTFOLIO CONTEXT WAS NOT PROVIDED FOR THIS REQUEST".to_owned()],
            net_asset_value: "N/A".to_owned(),
            available_cash: "N/A".to_owned(),
            ytd_return: "N/A".to_owned(),
            sharpe: "N/A".to_owned(),
            positions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantActivityEntry {
    pub activity_id: String,
    pub date: String,
    pub account: String,
    pub kind: String,
    pub symbol: Option<String>,
    pub description: String,
    pub quantity: String,
    pub cash_effect: String,
    pub fees: String,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantActivityCurrencyTotal {
    pub currency: String,
    pub entries: usize,
    pub inflows: String,
    pub outflows: String,
    pub net_cash_effect: String,
    pub dividends: String,
    pub interest: String,
    pub fees: String,
    pub non_cash_entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantActivityLedger {
    pub source: String,
    pub period: String,
    pub input_version: String,
    pub methodology: String,
    pub disclosures: Vec<String>,
    pub net_cash_effect: String,
    pub entries: Vec<AssistantActivityEntry>,
    pub currency_totals: Vec<AssistantActivityCurrencyTotal>,
}

impl AssistantActivityLedger {
    pub fn unavailable(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            period: "—".to_owned(),
            input_version: "—".to_owned(),
            methodology: "NO ACTIVITY CONTEXT".to_owned(),
            disclosures: vec!["ACTIVITY CONTEXT WAS NOT PROVIDED FOR THIS REQUEST".to_owned()],
            net_cash_effect: "N/A".to_owned(),
            entries: Vec::new(),
            currency_totals: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantContextSnapshot {
    pub portfolio: AssistantPortfolioSnapshot,
    pub activity: AssistantActivityLedger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantRequest {
    pub messages: Vec<AssistantMessage>,
    pub active_workspace: String,
    pub available_workspaces: Vec<String>,
    pub portfolio: AssistantPortfolioSnapshot,
    pub activity: AssistantActivityLedger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantResponse {
    pub content: String,
    pub actions: Vec<UiAction>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssistantTokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantStreamEvent {
    TextDelta(String),
    TokenUsage(AssistantTokenUsage),
    ToolStarted(String),
    ToolFinished { name: String, success: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantStatus {
    Ready,
    Loading,
    Streaming,
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_constructors_preserve_roles() {
        assert_eq!(
            AssistantMessage::user("show risk").role,
            AssistantRole::User
        );
        assert_eq!(
            AssistantMessage::assistant("done").role,
            AssistantRole::Assistant
        );
        assert_eq!(
            AssistantMessage::system("rules").role,
            AssistantRole::System
        );
    }
}
