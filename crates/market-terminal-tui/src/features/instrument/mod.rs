mod domain;
mod port;
mod workspace;

pub use domain::{Instrument, InstrumentId, InstrumentKind};
pub use port::InstrumentSearch;
pub use workspace::InstrumentSearchWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("instrument_search");
