use std::cmp::Ordering;

pub const MAX_DISCOVERY_ITEMS: usize = 256;
pub const MAX_DISCOVERY_RESULTS: usize = 128;
pub const MAX_DISCOVERY_QUERY_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiscoveryKind {
    Workspace,
    SavedView,
    Launchpad,
    Command,
}

impl DiscoveryKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Workspace => "WORKSPACE",
            Self::SavedView => "SAVED VIEW",
            Self::Launchpad => "LAUNCHPAD",
            Self::Command => "COMMAND",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Workspace => "WS",
            Self::SavedView => "VIEW",
            Self::Launchpad => "GO",
            Self::Command => "CMD",
        }
    }
}

/// One stable, directly invokable destination in the global discovery surface.
///
/// The shell owns commands, workspaces, and saved views. Features may contribute
/// bounded items through the same type without exposing their internal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryItem {
    pub id: String,
    pub kind: DiscoveryKind,
    pub label: String,
    pub command: String,
    pub owner: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub keywords: Vec<String>,
    pub entity_id: Option<u64>,
    pub revision: Option<u64>,
}

impl DiscoveryItem {
    pub fn new(
        id: impl Into<String>,
        kind: DiscoveryKind,
        label: impl Into<String>,
        command: impl Into<String>,
        owner: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            label: label.into(),
            command: command.into(),
            owner: owner.into(),
            description: description.into(),
            aliases: Vec::new(),
            keywords: Vec::new(),
            entity_id: None,
            revision: None,
        }
    }

    pub fn with_aliases(mut self, aliases: impl IntoIterator<Item = String>) -> Self {
        self.aliases = aliases.into_iter().collect();
        self
    }

    pub fn with_keywords(mut self, keywords: impl IntoIterator<Item = String>) -> Self {
        self.keywords = keywords.into_iter().collect();
        self
    }

    pub const fn with_identity(mut self, entity_id: u64, revision: u64) -> Self {
        self.entity_id = Some(entity_id);
        self.revision = Some(revision);
        self
    }

    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 160
            && !self.label.trim().is_empty()
            && self.label.len() <= 160
            && !self.command.trim().is_empty()
            && self.command.len() <= 512
            && !self.owner.trim().is_empty()
            && self.owner.len() <= 96
            && !self.description.trim().is_empty()
            && self.description.len() <= 512
            && self.aliases.len() <= 32
            && self.keywords.len() <= 32
            && self
                .aliases
                .iter()
                .chain(&self.keywords)
                .all(|value| !value.trim().is_empty() && value.len() <= 160)
    }
}

pub fn search(items: impl IntoIterator<Item = DiscoveryItem>, query: &str) -> Vec<DiscoveryItem> {
    let tokens = query
        .split_whitespace()
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut matches = items
        .into_iter()
        .filter_map(|item| match_score(&item, &tokens).map(|score| (score, item)))
        .collect::<Vec<_>>();
    matches.sort_by(|(left_score, left), (right_score, right)| {
        left_score
            .cmp(right_score)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| compare_case_insensitive(&left.label, &right.label))
            .then_with(|| left.id.cmp(&right.id))
    });
    matches
        .into_iter()
        .take(MAX_DISCOVERY_RESULTS)
        .map(|(_, item)| item)
        .collect()
}

fn match_score(item: &DiscoveryItem, tokens: &[String]) -> Option<u32> {
    if tokens.is_empty() {
        return Some(0);
    }
    let mut fields = vec![
        (item.label.to_ascii_lowercase(), 0_u32),
        (
            item.command.to_ascii_lowercase(),
            if item.kind == DiscoveryKind::Command {
                0
            } else {
                2
            },
        ),
        (item.owner.to_ascii_lowercase(), 5),
    ];
    fields.extend(
        item.aliases
            .iter()
            .map(|value| (value.to_ascii_lowercase(), 3)),
    );
    fields.extend(
        item.keywords
            .iter()
            .map(|value| (value.to_ascii_lowercase(), 8)),
    );
    tokens.iter().try_fold(0_u32, |total, token| {
        fields
            .iter()
            .filter_map(|(field, bias)| {
                field_score(field, token).map(|score| score.saturating_add(*bias))
            })
            .min()
            .map(|score| total.saturating_add(score))
    })
}

fn field_score(field: &str, token: &str) -> Option<u32> {
    if field == token {
        Some(0)
    } else if field.starts_with(token) {
        Some(10)
    } else if field
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| word.starts_with(token))
    {
        Some(20)
    } else if field.contains(token) {
        Some(30)
    } else {
        None
    }
}

fn compare_case_insensitive(left: &str, right: &str) -> Ordering {
    left.to_ascii_lowercase()
        .cmp(&right.to_ascii_lowercase())
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, kind: DiscoveryKind, label: &str, command: &str) -> DiscoveryItem {
        DiscoveryItem::new(id, kind, label, command, kind.label(), "Test destination")
    }

    #[test]
    fn search_requires_every_literal_token_and_ranks_exact_prefixes_first() {
        let items = vec![
            item("command:news", DiscoveryKind::Command, "NEWS", "NEWS"),
            item(
                "view:1",
                DiscoveryKind::SavedView,
                "Asia Technology News",
                "VIEW RESTORE Asia",
            ),
            item("launch:1", DiscoveryKind::Launchpad, "News Reader", "NEWS"),
        ];

        let matches = search(items.clone(), "news");
        assert_eq!(matches[0].id, "command:news");
        assert_eq!(matches.len(), 3);
        assert_eq!(search(items, "asia news")[0].id, "view:1");
    }

    #[test]
    fn empty_search_has_stable_kind_label_and_identity_order() {
        let matches = search(
            vec![
                item("command:z", DiscoveryKind::Command, "ZETA", "ZETA"),
                item("workspace:b", DiscoveryKind::Workspace, "BETA", "BETA"),
                item("workspace:a", DiscoveryKind::Workspace, "ALPHA", "ALPHA"),
            ],
            "",
        );
        assert_eq!(
            matches
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["workspace:a", "workspace:b", "command:z"]
        );
    }
}
