use crate::features::charting::{
    ChartHistoryQuery, HistoryError, HistoryQuality, HistoryRequest, HistorySeries, PriceBar,
};

/// Deterministic replay data for the native Charting workspace.
///
/// Values are generated solely from the canonical instrument identifier,
/// period, and observation index. Repeated requests are byte-for-byte stable.
#[derive(Debug, Default, Clone, Copy)]
pub struct DemoChartHistory;

impl ChartHistoryQuery for DemoChartHistory {
    fn load_history(&self, request: &HistoryRequest) -> Result<HistorySeries, HistoryError> {
        let seed = stable_hash(request.instrument.canonical_id.as_str());
        let count = request.period.sample_count();
        let interval = request.period.sample_interval_seconds();
        let as_of = 1_725_014_400_i64; // 2024-08-30 00:00:00 UTC
        let start = as_of - (count.saturating_sub(1) as i64 * interval);
        let anchor = anchor_price(&request.instrument.symbol, seed);
        let drift = ((seed % 17) as f64 - 6.0) / 10_000.0;
        let phase = (seed % 31) as f64 / 7.0;
        let mut previous_close = anchor;
        let bars = (0..count)
            .map(|index| {
                let position = index as f64;
                let daily_wave = ((position / 5.5) + phase).sin() * 0.006;
                let slower_wave = ((position / 19.0) + phase / 2.0).cos() * 0.009;
                let close = (anchor * (1.0 + drift * position + daily_wave + slower_wave))
                    .max(anchor * 0.35);
                let open = previous_close;
                let intrabar = 0.0025 + ((index as u64 + seed) % 7) as f64 * 0.00035;
                let high = open.max(close) * (1.0 + intrabar);
                let low = open.min(close) * (1.0 - intrabar);
                let volume = 900_000
                    + (seed % 2_700_000)
                    + ((index as u64 * 7919 + seed % 97_000) % 1_800_000);
                previous_close = close;
                PriceBar {
                    timestamp: start + index as i64 * interval,
                    open,
                    high,
                    low,
                    close,
                    volume,
                }
            })
            .collect();

        Ok(HistorySeries {
            instrument: request.instrument.clone(),
            bars,
            quality: HistoryQuality::Replayed,
            source: "DEMO EOD".to_owned(),
        })
    }
}

fn anchor_price(symbol: &str, seed: u64) -> f64 {
    match symbol.split_whitespace().next().unwrap_or_default() {
        "AAPL" => 187.20,
        "MSFT" => 421.35,
        "NVDA" => 118.40,
        "SPY" => 515.25,
        "QQQ" => 438.10,
        _ => 45.0 + (seed % 25_000) as f64 / 100.0,
    }
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(14_695_981_039_346_656_037, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::charting::{ChartInstrument, ChartPeriod};

    #[test]
    fn repeated_requests_return_identical_history() {
        let adapter = DemoChartHistory;
        let request = HistoryRequest::new(
            ChartInstrument::from_terminal_subject("AAPL"),
            ChartPeriod::OneYear,
        );

        assert_eq!(
            adapter.load_history(&request).unwrap(),
            adapter.load_history(&request).unwrap()
        );
    }

    #[test]
    fn periods_have_stable_expected_observation_counts() {
        let adapter = DemoChartHistory;
        for period in ChartPeriod::ALL {
            let request = HistoryRequest::new(
                ChartInstrument::from_terminal_subject("MSFT"),
                period,
            );
            let history = adapter.load_history(&request).unwrap();
            assert_eq!(history.bars.len(), period.sample_count());
            assert!(history.bars.windows(2).all(|bars| bars[0].timestamp < bars[1].timestamp));
            assert!(history
                .bars
                .iter()
                .all(|bar| bar.low <= bar.open && bar.open <= bar.high));
        }
    }

    #[test]
    fn canonical_ids_produce_distinct_replay_shapes() {
        let adapter = DemoChartHistory;
        let aapl = adapter
            .load_history(&HistoryRequest::new(
                ChartInstrument::new("us:xnas:aapl", "TEST"),
                ChartPeriod::OneMonth,
            ))
            .unwrap();
        let msft = adapter
            .load_history(&HistoryRequest::new(
                ChartInstrument::new("us:xnas:msft", "TEST"),
                ChartPeriod::OneMonth,
            ))
            .unwrap();
        assert_ne!(aapl.bars, msft.bars);
    }
}
