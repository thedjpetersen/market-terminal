use std::sync::Arc;

use crate::{
    app::{App, WorkspaceRegistry},
    features::{
        markets::{MarketsQuery, MarketsWorkspace},
        news::{NewsQuery, NewsWorkspace},
        overview::{OverviewQuery, OverviewWorkspace, ID as OVERVIEW},
        portfolio::{PortfolioQuery, PortfolioWorkspace},
        security::{SecurityQuery, SecurityWorkspace},
    },
    infrastructure::DemoData,
};

pub fn demo_app() -> App {
    let data = Arc::new(DemoData);
    let overview_query: Arc<dyn OverviewQuery> = data.clone();
    let markets_query: Arc<dyn MarketsQuery> = data.clone();
    let security_query: Arc<dyn SecurityQuery> = data.clone();
    let portfolio_query: Arc<dyn PortfolioQuery> = data.clone();
    let news_query: Arc<dyn NewsQuery> = data;

    let workspaces = WorkspaceRegistry::new(vec![
        Box::new(OverviewWorkspace::new(overview_query)),
        Box::new(MarketsWorkspace::new(markets_query)),
        Box::new(SecurityWorkspace::new(security_query)),
        Box::new(PortfolioWorkspace::new(portfolio_query)),
        Box::new(NewsWorkspace::new(news_query)),
    ]);
    App::new(workspaces, OVERVIEW)
}
