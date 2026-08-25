mod domain;
mod port;
mod workspace;

pub use domain::{
    ChartInstrument, ChartPeriod, ChartSpecError, ChartSpecification, HistoryQuality,
    HistorySeries, Normalization, PriceBar, Study, MAX_COMPARISONS,
};
pub use port::{ChartHistoryQuery, HistoryError, HistoryRequest};
pub use workspace::ChartingWorkspace;

use crate::app::WorkspaceId;

pub const ID: WorkspaceId = WorkspaceId::new("charting");
