use std::sync::Arc;

use crate::{
    app::{App, WorkspaceRegistry},
    features::{
        assistant::{AssistantGateway, AssistantWorkspace},
        instrument::{InstrumentSearch, InstrumentSearchWorkspace},
        markets::{MarketsQuery, MarketsWorkspace},
        news::{NewsQuery, NewsWorkspace},
        overview::{OverviewQuery, OverviewWorkspace, ID as OVERVIEW},
        portfolio::{PortfolioQuery, PortfolioWorkspace},
        security::{SecurityQuery, SecurityWorkspace},
        spreadsheet::{SpreadsheetMarketData, SpreadsheetWorkspace},
    },
    infrastructure::{
        DemoData, DemoInstrumentSearch, DemoSpreadsheetMarketData, OpenRouterConfig,
        OpenRouterGateway,
    },
};

pub fn demo_app() -> App {
    let data = Arc::new(DemoData);
    let overview_query: Arc<dyn OverviewQuery> = data.clone();
    let markets_query: Arc<dyn MarketsQuery> = data.clone();
    let security_query: Arc<dyn SecurityQuery> = data.clone();
    let portfolio_query: Arc<dyn PortfolioQuery> = data.clone();
    let news_query: Arc<dyn NewsQuery> = data;
    let spreadsheet_market_data: Arc<dyn SpreadsheetMarketData> =
        Arc::new(DemoSpreadsheetMarketData);
    let assistant_gateway: Arc<dyn AssistantGateway> =
        Arc::new(OpenRouterGateway::new(OpenRouterConfig::from_env()));
    let instrument_search: Arc<dyn InstrumentSearch> = Arc::new(DemoInstrumentSearch);

    let workspaces = WorkspaceRegistry::new(vec![
        Box::new(OverviewWorkspace::new(overview_query)),
        Box::new(AssistantWorkspace::new(
            assistant_gateway,
            vec![
                "overview".to_owned(),
                "assistant".to_owned(),
                "instrument_search".to_owned(),
                "markets".to_owned(),
                "security".to_owned(),
                "portfolio".to_owned(),
                "news".to_owned(),
                "spreadsheet".to_owned(),
            ],
        )),
        Box::new(InstrumentSearchWorkspace::new(instrument_search)),
        Box::new(MarketsWorkspace::new(markets_query)),
        Box::new(SecurityWorkspace::new(security_query)),
        Box::new(PortfolioWorkspace::new(portfolio_query)),
        Box::new(NewsWorkspace::new(news_query)),
        Box::new(SpreadsheetWorkspace::new(spreadsheet_market_data)),
    ]);
    App::new(workspaces, OVERVIEW)
}
