mod domain;
mod workspace;

pub use domain::{
    price_option, OptionAnalytics, OptionModelError, OptionModelInput, OptionRight, OptionScenario,
    MODEL_VERSION,
};
pub use workspace::OptionsWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("options");
