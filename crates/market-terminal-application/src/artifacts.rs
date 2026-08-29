use std::{fmt, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{ApplicationError, ApplicationErrorCode, ExecutionContext, TenantId};

pub const ARTIFACT_QUERY_SCHEMA_VERSION: u16 = 1;
pub const MAX_ARTIFACT_ID_BYTES: usize = 128;
pub const MAX_ARTIFACT_TITLE_BYTES: usize = 256;
pub const MAX_ARTIFACT_LABEL_BYTES: usize = 128;
pub const MAX_ARTIFACT_CURSOR_BYTES: usize = MAX_ARTIFACT_ID_BYTES;
pub const DEFAULT_ARTIFACT_PAGE_SIZE: usize = 25;
pub const MAX_ARTIFACT_PAGE_SIZE: usize = 100;
pub const MAX_ARTIFACT_DOCUMENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactCapabilitySet {
    read_research_artifacts: bool,
}

impl ArtifactCapabilitySet {
    pub const fn read_only() -> Self {
        Self {
            read_research_artifacts: true,
        }
    }

    pub const fn none() -> Self {
        Self {
            read_research_artifacts: false,
        }
    }

    pub const fn allows_read(self) -> bool {
        self.read_research_artifacts
    }

    pub fn allowed_names(self) -> Vec<&'static str> {
        self.allows_read()
            .then_some("read_research_artifacts")
            .into_iter()
            .collect()
    }
}

impl Default for ArtifactCapabilitySet {
    fn default() -> Self {
        Self::none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchArtifactKind {
    BacktestRun,
    BacktestComparison,
    ScreenResult,
    NewsSnapshot,
    SecurityResearch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchArtifactSummary {
    pub schema_version: u16,
    pub tenant_id: TenantId,
    pub artifact_id: String,
    pub kind: ResearchArtifactKind,
    pub title: String,
    pub created_at_epoch_ms: u64,
    pub input_version: String,
    pub source: String,
    pub quality: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchArtifactDocument {
    #[serde(flatten)]
    pub summary: ResearchArtifactSummary,
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantArtifactListKey {
    tenant_id: TenantId,
    kind: Option<ResearchArtifactKind>,
    cursor: Option<String>,
    limit: usize,
}

impl TenantArtifactListKey {
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub const fn kind(&self) -> Option<ResearchArtifactKind> {
        self.kind
    }

    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantArtifactKey {
    tenant_id: TenantId,
    artifact_id: String,
}

impl TenantArtifactKey {
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn artifact_id(&self) -> &str {
        &self.artifact_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchArtifactPage {
    pub schema_version: u16,
    pub items: Vec<ResearchArtifactSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactQueryFailure {
    Unavailable,
    Corrupt(String),
}

impl fmt::Display for ArtifactQueryFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("artifact query service is unavailable"),
            Self::Corrupt(message) => {
                write!(formatter, "artifact repository is corrupt: {message}")
            }
        }
    }
}

impl std::error::Error for ArtifactQueryFailure {}

/// Read-only tenant artifact boundary. Callers cannot construct a key without
/// an application service first binding it to an authenticated tenant.
pub trait ResearchArtifactQuery: Send + Sync {
    fn list(
        &self,
        key: &TenantArtifactListKey,
    ) -> Result<ResearchArtifactPage, ArtifactQueryFailure>;

    fn get(
        &self,
        key: &TenantArtifactKey,
    ) -> Result<Option<ResearchArtifactDocument>, ArtifactQueryFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArtifactListRequest {
    pub kind: Option<ResearchArtifactKind>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone)]
pub struct ResearchArtifactApplicationService {
    query: Arc<dyn ResearchArtifactQuery>,
}

impl ResearchArtifactApplicationService {
    pub fn new(query: Arc<dyn ResearchArtifactQuery>) -> Self {
        Self { query }
    }

    pub fn list(
        &self,
        context: &ExecutionContext,
        request: ArtifactListRequest,
    ) -> Result<ResearchArtifactPage, ApplicationError> {
        authorize_artifact_read(context)?;
        let limit = request.limit.unwrap_or(DEFAULT_ARTIFACT_PAGE_SIZE);
        if !(1..=MAX_ARTIFACT_PAGE_SIZE).contains(&limit) {
            return Err(invalid_artifact_request(format!(
                "artifact page limit {limit} must be between 1 and {MAX_ARTIFACT_PAGE_SIZE}"
            )));
        }
        if request
            .cursor
            .as_ref()
            .is_some_and(|cursor| validate_artifact_id(cursor).is_err())
        {
            return Err(invalid_artifact_request("artifact cursor is invalid"));
        }
        let key = TenantArtifactListKey {
            tenant_id: context.tenant_id().clone(),
            kind: request.kind,
            cursor: request.cursor,
            limit,
        };
        let page = self.query.list(&key).map_err(map_artifact_failure)?;
        validate_artifact_page(&page, context.tenant_id(), key.kind, limit)?;
        Ok(page)
    }

    pub fn get(
        &self,
        context: &ExecutionContext,
        artifact_id: impl Into<String>,
    ) -> Result<Option<ResearchArtifactDocument>, ApplicationError> {
        authorize_artifact_read(context)?;
        let artifact_id = artifact_id.into();
        validate_artifact_id(&artifact_id)
            .map_err(|message| invalid_artifact_request(message.to_owned()))?;
        let key = TenantArtifactKey {
            tenant_id: context.tenant_id().clone(),
            artifact_id,
        };
        let document = self.query.get(&key).map_err(map_artifact_failure)?;
        if let Some(document) = &document {
            validate_artifact_document(document, context.tenant_id(), key.artifact_id())?;
        }
        Ok(document)
    }
}

fn authorize_artifact_read(context: &ExecutionContext) -> Result<(), ApplicationError> {
    if context.artifact_capabilities().allows_read() {
        return Ok(());
    }
    Err(ApplicationError {
        code: ApplicationErrorCode::CapabilityDenied,
        message: format!(
            "principal {} in tenant {} cannot execute read_research_artifacts",
            context.principal_id(),
            context.tenant_id()
        ),
    })
}

fn invalid_artifact_request(message: impl Into<String>) -> ApplicationError {
    ApplicationError {
        code: ApplicationErrorCode::InvalidArtifactRequest,
        message: message.into(),
    }
}

fn map_artifact_failure(error: ArtifactQueryFailure) -> ApplicationError {
    match error {
        ArtifactQueryFailure::Unavailable => ApplicationError {
            code: ApplicationErrorCode::ArtifactServiceUnavailable,
            message: "artifact query service is unavailable".to_owned(),
        },
        ArtifactQueryFailure::Corrupt(_) => ApplicationError {
            code: ApplicationErrorCode::ArtifactContractViolation,
            message: "artifact query service returned invalid data".to_owned(),
        },
    }
}

fn validate_artifact_page(
    page: &ResearchArtifactPage,
    tenant_id: &TenantId,
    requested_kind: Option<ResearchArtifactKind>,
    limit: usize,
) -> Result<(), ApplicationError> {
    if page.schema_version != ARTIFACT_QUERY_SCHEMA_VERSION
        || page.items.len() > limit
        || page.next_cursor.as_ref().is_some_and(|cursor| {
            validate_artifact_id(cursor).is_err()
                || page.items.last().map(|item| &item.artifact_id) != Some(cursor)
        })
        || page.items.iter().any(|summary| {
            validate_artifact_summary(summary, tenant_id).is_err()
                || requested_kind.is_some_and(|kind| summary.kind != kind)
        })
    {
        return Err(contract_violation());
    }
    Ok(())
}

fn validate_artifact_document(
    document: &ResearchArtifactDocument,
    tenant_id: &TenantId,
    artifact_id: &str,
) -> Result<(), ApplicationError> {
    if validate_artifact_summary(&document.summary, tenant_id).is_err()
        || document.summary.artifact_id != artifact_id
        || serde_json::to_vec(document)
            .map(|encoded| encoded.len() > MAX_ARTIFACT_DOCUMENT_BYTES)
            .unwrap_or(true)
    {
        return Err(contract_violation());
    }
    Ok(())
}

fn contract_violation() -> ApplicationError {
    ApplicationError {
        code: ApplicationErrorCode::ArtifactContractViolation,
        message: "artifact query service returned invalid data".to_owned(),
    }
}

fn validate_artifact_summary(
    summary: &ResearchArtifactSummary,
    tenant_id: &TenantId,
) -> Result<(), ()> {
    if summary.schema_version != ARTIFACT_QUERY_SCHEMA_VERSION
        || &summary.tenant_id != tenant_id
        || validate_artifact_id(&summary.artifact_id).is_err()
        || summary.title.is_empty()
        || summary.title.len() > MAX_ARTIFACT_TITLE_BYTES
        || !valid_artifact_label(&summary.input_version)
        || !valid_artifact_label(&summary.source)
        || !valid_artifact_label(&summary.quality)
        || !valid_artifact_digest(&summary.content_digest)
    {
        return Err(());
    }
    Ok(())
}

fn validate_artifact_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > MAX_ARTIFACT_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err("artifact identity is invalid");
    }
    Ok(())
}

fn valid_artifact_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ARTIFACT_LABEL_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
}

fn valid_artifact_digest(value: &str) -> bool {
    (8..=MAX_ARTIFACT_LABEL_BYTES).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}
