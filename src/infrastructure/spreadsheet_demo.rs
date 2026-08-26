use crate::features::spreadsheet::{
    MarketDataPoint, MarketDataProvenance, MarketDataQuality, MarketDataRequest,
    SpreadsheetMarketData,
};

#[derive(Debug, Default)]
pub struct DemoSpreadsheetMarketData;

impl SpreadsheetMarketData for DemoSpreadsheetMarketData {
    fn load_batch(&self, requests: &[MarketDataRequest]) -> Vec<MarketDataPoint> {
        requests
            .iter()
            .filter_map(|request| {
                let value = match (request.security.as_str(), request.field.as_str()) {
                    ("IBM US Equity", "PX_LAST") => 234.19,
                    ("IBM US Equity", "CHG_PCT_1D") => 1.36,
                    ("SPY US Equity", "PX_LAST") => 530.47,
                    ("QQQ US Equity", "PX_LAST") => 455.18,
                    ("AVGO US Equity", "PX_LAST") => 176.42,
                    ("NVDA US Equity", "PX_LAST") => 119.31,
                    ("SPY US Equity", "CHG_PCT_1D") => 0.86,
                    ("QQQ US Equity", "CHG_PCT_1D") => 1.00,
                    ("AVGO US Equity", "CHG_PCT_1D") => 1.72,
                    ("NVDA US Equity", "CHG_PCT_1D") => 2.14,
                    _ => return None,
                };
                Some(MarketDataPoint::ready(
                    request.clone(),
                    value,
                    MarketDataProvenance {
                        provider: "BUILT-IN DEMO SNAPSHOT".to_owned(),
                        observed_at: "2026-08-26T13:00:00-07:00".to_owned(),
                        received_at: "2026-08-26T13:00:00-07:00".to_owned(),
                        quality: MarketDataQuality::Demo,
                    },
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::spreadsheet::MarketDataState;

    #[test]
    fn batch_adapter_preserves_known_request_order_and_skips_unknown_fields() {
        let requests = vec![
            MarketDataRequest::new("QQQ US Equity", "CHG_PCT_1D"),
            MarketDataRequest::new("UNKNOWN", "PX_LAST"),
            MarketDataRequest::new("SPY US Equity", "PX_LAST"),
        ];
        let values = DemoSpreadsheetMarketData.load_batch(&requests);
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].request, requests[0]);
        assert!(matches!(
            values[0].state,
            MarketDataState::Ready { value: 1.0, .. }
        ));
        assert_eq!(values[1].request, requests[2]);
        assert!(matches!(
            values[1].state,
            MarketDataState::Ready { value: 530.47, .. }
        ));
    }
}
