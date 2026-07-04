use super::OutputLevel;
use super::fields::FieldBag;

pub(crate) const HUMAN_COLOR_NEUTRAL: &str = "1;37";
pub(crate) const HUMAN_COLOR_DIM: &str = "2";
pub(crate) const HUMAN_COLOR_WARN: &str = "1;33";
pub(crate) const HUMAN_COLOR_ERROR: &str = "1;31";
pub(crate) const HUMAN_COLOR_ACTION_CREATED: &str = "1;32";
pub(crate) const HUMAN_COLOR_ACTION_MODIFIED: &str = "1;33";
pub(crate) const HUMAN_COLOR_ACTION_DELETED: &str = "1;31";
pub(crate) const HUMAN_COLOR_ACTION_RENAMED: &str = "1;35";
pub(crate) const HUMAN_COLOR_TOPIC_INDEX: &str = "1;38;2;130;170;255";
pub(crate) const HUMAN_COLOR_TOPIC_SEMANTIC: &str = "1;38;2;90;205;210";
pub(crate) const HUMAN_COLOR_TOPIC_PRECISE: &str = "1;38;2;185;150;255";
pub(crate) const HUMAN_COLOR_TOPIC_STORAGE: &str = "1;38;2;205;180;120";
pub(crate) const HUMAN_COLOR_TOPIC_WATCH: &str = "1;38;2;110;200;255";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HumanTopic {
    Index,
    Semantic,
    Precise,
    Storage,
    Watch,
}

impl HumanTopic {
    pub(crate) const fn color(self) -> &'static str {
        match self {
            Self::Index => HUMAN_COLOR_TOPIC_INDEX,
            Self::Semantic => HUMAN_COLOR_TOPIC_SEMANTIC,
            Self::Precise => HUMAN_COLOR_TOPIC_PRECISE,
            Self::Storage => HUMAN_COLOR_TOPIC_STORAGE,
            Self::Watch => HUMAN_COLOR_TOPIC_WATCH,
        }
    }
}

pub(crate) fn human_title_accent_color(
    level: OutputLevel,
    fields: FieldBag<'_>,
    topic: Option<HumanTopic>,
    title: &str,
) -> &'static str {
    let status = fields.value("status").unwrap_or_default();
    if let Some(color) = human_severity_accent_color(level, status) {
        return color;
    }
    if let Some(topic) = topic
        .or_else(|| human_field_topic(fields, title))
        .or_else(|| human_title_topic(title))
    {
        return topic.color();
    }
    human_state_fallback_accent_color(level, status).unwrap_or_else(|| title_color(level))
}

pub(crate) fn human_sidecar_line_color(
    fields: FieldBag<'_>,
    topic: Option<HumanTopic>,
    title: &str,
    fallback: &'static str,
) -> &'static str {
    topic
        .or_else(|| human_field_topic(fields, title))
        .or_else(|| human_title_topic(title))
        .map(HumanTopic::color)
        .unwrap_or(fallback)
}

pub(crate) fn human_severity_accent_color(
    level: OutputLevel,
    status: &str,
) -> Option<&'static str> {
    match (level, status) {
        (OutputLevel::Error, _) | (_, "failed") | (_, "blocked") => Some(HUMAN_COLOR_ERROR),
        (OutputLevel::Warn, _) | (_, "retry") | (_, "stale") => Some(HUMAN_COLOR_WARN),
        _ => None,
    }
}

fn human_state_fallback_accent_color(level: OutputLevel, status: &str) -> Option<&'static str> {
    match (level, status) {
        (OutputLevel::Skip, _) | (_, "empty") | (_, "skipped") | (_, "fresh") => {
            Some(HUMAN_COLOR_DIM)
        }
        _ => None,
    }
}

fn human_field_topic(fields: FieldBag<'_>, title: &str) -> Option<HumanTopic> {
    if let Some(phase) = fields.value("phase") {
        return match phase {
            "initialize_storage" | "checkpoint_wal" | "prune_manifest_snapshots" => {
                Some(HumanTopic::Storage)
            }
            "semantic_refresh" => Some(HumanTopic::Semantic),
            "load_manifest"
            | "build_manifest"
            | "build_plan"
            | "persist_manifest_snapshot"
            | "refresh_retrieval_projections" => Some(HumanTopic::Index),
            _ => None,
        };
    }
    if title.starts_with("Running ")
        && (fields.value("generator").is_some()
            || fields.value("tool").is_some()
            || fields.value("language").is_some())
    {
        return Some(HumanTopic::Precise);
    }
    None
}

pub(crate) fn human_title_topic(title: &str) -> Option<HumanTopic> {
    let normalized = title.to_ascii_lowercase();
    if normalized.starts_with("semantic") || normalized.starts_with("embedding") {
        return Some(HumanTopic::Semantic);
    }
    if normalized.starts_with("precise") {
        return Some(HumanTopic::Precise);
    }
    if normalized.starts_with("storage")
        || normalized.contains("storage")
        || normalized.contains("snapshot")
        || normalized.contains("checkpoint")
        || normalized.starts_with("prune")
    {
        return Some(HumanTopic::Storage);
    }
    if normalized.starts_with("watch") {
        return Some(HumanTopic::Watch);
    }
    if normalized.starts_with("index")
        || normalized.starts_with("search")
        || normalized.starts_with("manifest")
        || normalized.starts_with("files")
        || normalized.starts_with("file ")
        || normalized.starts_with("walking")
        || normalized.starts_with("planning")
        || normalized.starts_with("writing")
        || normalized.starts_with("loading manifest")
        || normalized.starts_with("refreshing search")
    {
        return Some(HumanTopic::Index);
    }
    None
}

fn title_color(level: OutputLevel) -> &'static str {
    match level {
        OutputLevel::Ok | OutputLevel::Info => HUMAN_COLOR_NEUTRAL,
        OutputLevel::Warn => HUMAN_COLOR_WARN,
        OutputLevel::Error => HUMAN_COLOR_ERROR,
        OutputLevel::Skip => HUMAN_COLOR_DIM,
    }
}

pub(crate) fn action_color(action: &str) -> &'static str {
    match action {
        "created" | "create" | "added" | "add" | "new" => HUMAN_COLOR_ACTION_CREATED,
        "modified" | "changed" | "updated" | "update" => HUMAN_COLOR_ACTION_MODIFIED,
        "deleted" | "delete" | "removed" | "remove" => HUMAN_COLOR_ACTION_DELETED,
        "renamed" | "moved" => HUMAN_COLOR_ACTION_RENAMED,
        "skipped" | "unchanged" => HUMAN_COLOR_DIM,
        _ => "1;36",
    }
}
