use std::sync::Arc;

use crate::features::{
    backtesting::{
        BacktestBar, BacktestHistoryError, BacktestHistoryQuery, BacktestHistoryRequest,
        BacktestHistorySnapshot,
    },
    charting::{ChartHistoryQuery, ChartInstrument, ChartPeriod, HistoryError, HistoryRequest},
};

/// Composition-root translator from chart-ready history to Backtesting's own
/// immutable input contract. Neither bounded context imports the other.
pub struct ChartBacktestHistory {
    chart_history: Arc<dyn ChartHistoryQuery>,
}

impl ChartBacktestHistory {
    pub fn new(chart_history: Arc<dyn ChartHistoryQuery>) -> Self {
        Self { chart_history }
    }
}

impl BacktestHistoryQuery for ChartBacktestHistory {
    fn load_history(
        &self,
        request: &BacktestHistoryRequest,
    ) -> Result<BacktestHistorySnapshot, BacktestHistoryError> {
        let series = self
            .chart_history
            .load_history(&HistoryRequest::new(
                ChartInstrument::new(&request.instrument_id, &request.symbol),
                ChartPeriod::OneYear,
            ))
            .map_err(|error| match error {
                HistoryError::Unavailable(message) => BacktestHistoryError::Unavailable(message),
                HistoryError::PermissionDenied(message) => {
                    BacktestHistoryError::PermissionDenied(message)
                }
            })?;
        if series.instrument.canonical_id.as_str() != request.instrument_id
            || series.instrument.symbol != request.symbol
        {
            return Err(BacktestHistoryError::Invalid(
                "provider returned a different instrument identity".to_owned(),
            ));
        }
        let bars = series
            .bars
            .into_iter()
            .enumerate()
            .map(|(index, bar)| {
                Ok(BacktestBar {
                    timestamp: bar.timestamp,
                    open_micros: price_micros(bar.open, index, "open")?,
                    high_micros: price_micros(bar.high, index, "high")?,
                    low_micros: price_micros(bar.low, index, "low")?,
                    close_micros: price_micros(bar.close, index, "close")?,
                    volume: bar.volume,
                })
            })
            .collect::<Result<Vec<_>, BacktestHistoryError>>()?;
        let input_version = history_digest(&request.instrument_id, &bars);
        Ok(BacktestHistorySnapshot {
            instrument_id: request.instrument_id.clone(),
            symbol: request.symbol.clone(),
            bars,
            source: series.source,
            quality: series.quality.label().to_owned(),
            input_version,
        })
    }
}

fn price_micros(value: f64, index: usize, field: &str) -> Result<i64, BacktestHistoryError> {
    if !value.is_finite() || value <= 0.0 || value > i64::MAX as f64 / 1_000_000.0 {
        return Err(BacktestHistoryError::Invalid(format!(
            "bar {index} has invalid {field} price"
        )));
    }
    Ok((value * 1_000_000.0).round() as i64)
}

fn history_digest(instrument_id: &str, bars: &[BacktestBar]) -> String {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in instrument_id.bytes().chain(bars.iter().flat_map(|bar| {
        bar.timestamp
            .to_le_bytes()
            .into_iter()
            .chain(bar.open_micros.to_le_bytes())
            .chain(bar.high_micros.to_le_bytes())
            .chain(bar.low_micros.to_le_bytes())
            .chain(bar.close_micros.to_le_bytes())
            .chain(bar.volume.to_le_bytes())
    })) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211);
    }
    format!("HISTORY-FNV1A64-{hash:016X}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::DemoChartHistory;

    #[test]
    fn translation_is_exact_and_reproducible() {
        let query = ChartBacktestHistory::new(Arc::new(DemoChartHistory));
        let request = BacktestHistoryRequest {
            instrument_id: "terminal:aapl".to_owned(),
            symbol: "AAPL".to_owned(),
        };
        let first = query.load_history(&request).unwrap();
        let second = query.load_history(&request).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.bars.len(), 252);
        assert!(first.input_version.starts_with("HISTORY-FNV1A64-"));
    }
}
