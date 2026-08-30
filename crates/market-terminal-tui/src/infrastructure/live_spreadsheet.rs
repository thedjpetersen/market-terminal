use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use crate::features::spreadsheet::{
    MarketDataPoint, MarketDataRequest, MarketDataState, SpreadsheetMarketData,
};

/// Infrastructure composition for Spreadsheet's provider-specific query port.
///
/// Reported fundamentals come from the SEC adapter while quotes and history
/// remain owned by the operator-selected market-data adapter. Spreadsheet sees
/// one batch port and has no dependency on either provider.
pub struct LiveSpreadsheetMarketData {
    market_data: Arc<dyn SpreadsheetMarketData>,
    fundamentals: Arc<dyn SpreadsheetMarketData>,
}

impl LiveSpreadsheetMarketData {
    pub fn new(
        market_data: Arc<dyn SpreadsheetMarketData>,
        fundamentals: Arc<dyn SpreadsheetMarketData>,
    ) -> Self {
        Self {
            market_data,
            fundamentals,
        }
    }
}

impl SpreadsheetMarketData for LiveSpreadsheetMarketData {
    fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint> {
        let mut market_requests = Vec::new();
        let mut fundamental_requests = Vec::new();
        for (index, request) in requests.iter().enumerate() {
            if request.field.starts_with("FUNDAMENTAL|") {
                fundamental_requests.push((index, request.clone()));
            } else {
                market_requests.push((index, request.clone()));
            }
        }

        let mut points = requests
            .iter()
            .cloned()
            .map(|request| MarketDataPoint {
                request,
                state: MarketDataState::Unavailable {
                    reason: "configured provider returned no result".to_owned(),
                },
            })
            .collect::<Vec<_>>();
        route_batch(self.market_data.as_ref(), &market_requests, &mut points);
        route_batch(
            self.fundamentals.as_ref(),
            &fundamental_requests,
            &mut points,
        );
        points
    }
}

fn route_batch(
    provider: &dyn SpreadsheetMarketData,
    indexed_requests: &[(usize, MarketDataRequest)],
    output: &mut [MarketDataPoint],
) {
    if indexed_requests.is_empty() {
        return;
    }
    let requests = indexed_requests
        .iter()
        .map(|(_, request)| request.clone())
        .collect::<Vec<_>>();
    let mut returned = HashMap::<MarketDataRequest, VecDeque<MarketDataPoint>>::new();
    for point in provider.load_batch(&requests) {
        returned
            .entry(point.request.clone())
            .or_default()
            .push_back(point);
    }
    for (index, request) in indexed_requests {
        if let Some(point) = returned.get_mut(request).and_then(VecDeque::pop_front) {
            output[*index] = point;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::{Duration as ChronoDuration, Utc};

    use super::*;
    use crate::{
        features::spreadsheet::{MarketDataProvenance, MarketDataQuality, MarketDataState},
        infrastructure::{AlphaVantageMarketData, LiveSecurityQuery},
    };

    struct RecordingProvider {
        value: f64,
        omitted_field: Option<&'static str>,
        requests: Mutex<Vec<MarketDataRequest>>,
    }

    impl RecordingProvider {
        fn new(value: f64) -> Self {
            Self {
                value,
                omitted_field: None,
                requests: Mutex::new(Vec::new()),
            }
        }

        fn omitting(value: f64, field: &'static str) -> Self {
            Self {
                value,
                omitted_field: Some(field),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl SpreadsheetMarketData for RecordingProvider {
        fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint> {
            self.requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .extend_from_slice(requests);
            requests
                .iter()
                .filter(|request| self.omitted_field != Some(request.field.as_str()))
                .cloned()
                .map(|request| {
                    MarketDataPoint::ready(
                        request,
                        self.value,
                        MarketDataProvenance {
                            provider: "RECORDING PROVIDER".to_owned(),
                            observed_at: "2026-08-27".to_owned(),
                            received_at: "2026-08-27T00:00:00Z".to_owned(),
                            quality: MarketDataQuality::Demo,
                        },
                    )
                })
                .collect()
        }
    }

    #[test]
    fn preserves_batch_order_while_routing_fundamentals_to_sec_boundary() {
        let market = Arc::new(RecordingProvider::new(10.0));
        let fundamentals = Arc::new(RecordingProvider::new(20.0));
        let adapter = LiveSpreadsheetMarketData::new(market.clone(), fundamentals.clone());
        let requests = vec![
            MarketDataRequest::new("IBM US Equity", "PX_LAST"),
            MarketDataRequest::new("IBM US Equity", "FUNDAMENTAL|REVENUE|FY2025"),
            MarketDataRequest::new("IBM US Equity", "HISTORY|PX_LAST|2026-08-01|2026-08-27"),
        ];

        let points = adapter.load_batch(&requests);

        assert_eq!(points.len(), requests.len());
        assert_eq!(points[0].request, requests[0]);
        assert_eq!(points[1].request, requests[1]);
        assert_eq!(points[2].request, requests[2]);
        assert!(matches!(
            points[0].state,
            MarketDataState::Ready { value: 10.0, .. }
        ));
        assert!(matches!(
            points[1].state,
            MarketDataState::Ready { value: 20.0, .. }
        ));
        assert!(matches!(
            points[2].state,
            MarketDataState::Ready { value: 10.0, .. }
        ));
        assert_eq!(
            market
                .requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            2
        );
        assert_eq!(
            fundamentals
                .requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            1
        );
    }

    #[test]
    fn sparse_provider_results_match_by_request_instead_of_shifting_rows() {
        let market = Arc::new(RecordingProvider::omitting(10.0, "UNSUPPORTED"));
        let fundamentals = Arc::new(RecordingProvider::new(20.0));
        let adapter = LiveSpreadsheetMarketData::new(market, fundamentals);
        let requests = vec![
            MarketDataRequest::new("IBM US Equity", "UNSUPPORTED"),
            MarketDataRequest::new("IBM US Equity", "PX_LAST"),
        ];

        let points = adapter.load_batch(&requests);

        assert_eq!(points[0].request, requests[0]);
        assert!(matches!(
            points[0].state,
            MarketDataState::Unavailable { .. }
        ));
        assert_eq!(points[1].request, requests[1]);
        assert!(matches!(
            points[1].state,
            MarketDataState::Ready { value: 10.0, .. }
        ));
    }

    #[test]
    #[ignore = "live Alpha Vantage + SEC Spreadsheet contract test"]
    fn live_ibm_history_and_fundamental_flow_through_composite() {
        let alpha = Arc::new(AlphaVantageMarketData::from_env());
        let sec = Arc::new(LiveSecurityQuery::from_env(alpha.clone(), alpha.clone()));
        let adapter = LiveSpreadsheetMarketData::new(alpha, sec);
        let end = Utc::now().date_naive();
        let start = end - ChronoDuration::days(60);
        let requests = vec![
            MarketDataRequest::new("IBM US Equity", format!("HISTORY|PX_LAST|{start}|{end}")),
            MarketDataRequest::new("IBM US Equity", "FUNDAMENTAL|REVENUE|FY2024"),
        ];

        let points = adapter.load_batch(&requests);

        assert_eq!(points.len(), 2);
        let MarketDataState::Ready {
            value: history,
            provenance: history_provenance,
        } = &points[0].state
        else {
            panic!("live history was not ready: {:?}", points[0].state);
        };
        assert!(*history > 0.0);
        assert_eq!(
            history_provenance.provider,
            "ALPHA VANTAGE · TIME_SERIES_DAILY"
        );
        let observed_history =
            chrono::NaiveDate::parse_from_str(&history_provenance.observed_at, "%Y-%m-%d")
                .expect("Alpha Vantage observation date");
        assert!(observed_history >= start && observed_history <= end);
        assert_eq!(history_provenance.quality, MarketDataQuality::Delayed);
        let MarketDataState::Ready {
            value: revenue,
            provenance: fundamental_provenance,
        } = &points[1].state
        else {
            panic!("live fundamental was not ready: {:?}", points[1].state);
        };
        assert!(*revenue > 1_000_000_000.0);
        assert_eq!(fundamental_provenance.provider, "SEC EDGAR · COMPANYFACTS");
        assert!(fundamental_provenance.observed_at.starts_with("2024-"));
        assert_eq!(fundamental_provenance.quality, MarketDataQuality::Delayed);
    }
}
