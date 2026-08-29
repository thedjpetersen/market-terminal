use std::fmt;

pub const DEFAULT_INITIAL_CASH_MICROS: i64 = 100_000_000_000;
pub const MAX_BACKTEST_BARS: usize = 20_000;
pub const MAX_MOVING_AVERAGE_WINDOW: usize = 500;
const BPS_SCALE: i128 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BacktestBar {
    pub timestamp: i64,
    pub open_micros: i64,
    pub high_micros: i64,
    pub low_micros: i64,
    pub close_micros: i64,
    pub volume: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestConfig {
    pub instrument_id: String,
    pub symbol: String,
    pub fast_window: usize,
    pub slow_window: usize,
    /// Symmetric all-in execution penalty applied to buys and sells.
    pub execution_cost_bps: u32,
    pub commission_micros: i64,
    pub initial_cash_micros: i64,
}

impl BacktestConfig {
    pub fn moving_average_cross(
        instrument_id: impl Into<String>,
        symbol: impl Into<String>,
    ) -> Self {
        Self {
            instrument_id: instrument_id.into(),
            symbol: symbol.into(),
            fast_window: 20,
            slow_window: 100,
            execution_cost_bps: 3,
            commission_micros: 1_000_000,
            initial_cash_micros: DEFAULT_INITIAL_CASH_MICROS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeSide {
    Buy,
    Sell,
}

impl TradeSide {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestTrade {
    pub side: TradeSide,
    pub signal_timestamp: i64,
    pub execution_timestamp: i64,
    pub quantity: u64,
    pub reference_price_micros: i64,
    pub execution_price_micros: i64,
    pub commission_micros: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestDecision {
    pub observed_at: i64,
    pub executes_at: i64,
    pub target_long: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquityPoint {
    pub timestamp: i64,
    pub equity_micros: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestArtifact {
    pub schema_version: u16,
    pub strategy: String,
    pub instrument_id: String,
    pub symbol: String,
    pub source: String,
    pub quality: String,
    pub input_version: String,
    pub config_digest: String,
    pub data_digest: String,
    pub run_digest: String,
    pub bars: usize,
    pub first_timestamp: i64,
    pub last_timestamp: i64,
    pub initial_cash_micros: i64,
    pub final_equity_micros: i64,
    pub total_return_bps: i32,
    pub max_drawdown_bps: u32,
    pub turnover_bps: u32,
    pub decisions: Vec<BacktestDecision>,
    pub trades: Vec<BacktestTrade>,
    pub equity: Vec<EquityPoint>,
    pub open_quantity: u64,
    pub methodology: String,
    pub disclosures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacktestError {
    InvalidConfig(String),
    InvalidBars(String),
    Arithmetic(String),
}

impl fmt::Display for BacktestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(formatter, "invalid backtest config: {message}"),
            Self::InvalidBars(message) => write!(formatter, "invalid backtest bars: {message}"),
            Self::Arithmetic(message) => write!(formatter, "backtest arithmetic failed: {message}"),
        }
    }
}

impl std::error::Error for BacktestError {}

pub fn run_backtest(
    config: &BacktestConfig,
    bars: &[BacktestBar],
    source: impl Into<String>,
    quality: impl Into<String>,
    input_version: impl Into<String>,
) -> Result<BacktestArtifact, BacktestError> {
    validate_config(config)?;
    validate_bars(bars, config.slow_window)?;
    let source = source.into();
    let quality = quality.into();
    let input_version = input_version.into();
    if source.trim().is_empty() || quality.trim().is_empty() || input_version.trim().is_empty() {
        return Err(BacktestError::InvalidBars(
            "source, quality, and input version are required".to_owned(),
        ));
    }

    let config_digest = config_digest(config);
    let data_digest = data_digest(bars);
    let mut cash = i128::from(config.initial_cash_micros);
    let mut quantity = 0_u64;
    let mut pending = None::<(i64, bool)>;
    let mut trades = Vec::new();
    let mut decisions = Vec::new();
    let mut equity = Vec::with_capacity(bars.len());
    let mut turnover = 0_i128;
    let mut peak = i128::from(config.initial_cash_micros);
    let mut max_drawdown_bps = 0_u32;

    for (index, bar) in bars.iter().enumerate() {
        if let Some((signal_timestamp, target_long)) = pending.take() {
            if target_long && quantity == 0 {
                let execution_price = apply_cost(bar.open_micros, config.execution_cost_bps, true)?;
                let available = cash - i128::from(config.commission_micros);
                if available > 0 {
                    let purchasable = available / i128::from(execution_price);
                    quantity = u64::try_from(purchasable).map_err(|_| {
                        BacktestError::Arithmetic("position quantity overflow".to_owned())
                    })?;
                    if quantity > 0 {
                        let notional = i128::from(quantity) * i128::from(execution_price);
                        cash = cash
                            .checked_sub(notional + i128::from(config.commission_micros))
                            .ok_or_else(|| {
                                BacktestError::Arithmetic("buy cash overflow".to_owned())
                            })?;
                        turnover = turnover.checked_add(notional).ok_or_else(|| {
                            BacktestError::Arithmetic("turnover overflow".to_owned())
                        })?;
                        trades.push(BacktestTrade {
                            side: TradeSide::Buy,
                            signal_timestamp,
                            execution_timestamp: bar.timestamp,
                            quantity,
                            reference_price_micros: bar.open_micros,
                            execution_price_micros: execution_price,
                            commission_micros: config.commission_micros,
                        });
                    }
                }
            } else if !target_long && quantity > 0 {
                let execution_price =
                    apply_cost(bar.open_micros, config.execution_cost_bps, false)?;
                let notional = i128::from(quantity) * i128::from(execution_price);
                cash = cash
                    .checked_add(notional - i128::from(config.commission_micros))
                    .ok_or_else(|| BacktestError::Arithmetic("sell cash overflow".to_owned()))?;
                turnover = turnover
                    .checked_add(notional)
                    .ok_or_else(|| BacktestError::Arithmetic("turnover overflow".to_owned()))?;
                trades.push(BacktestTrade {
                    side: TradeSide::Sell,
                    signal_timestamp,
                    execution_timestamp: bar.timestamp,
                    quantity,
                    reference_price_micros: bar.open_micros,
                    execution_price_micros: execution_price,
                    commission_micros: config.commission_micros,
                });
                quantity = 0;
            }
        }

        let marked = cash
            .checked_add(i128::from(quantity) * i128::from(bar.close_micros))
            .ok_or_else(|| BacktestError::Arithmetic("equity overflow".to_owned()))?;
        let marked_i64 = i64::try_from(marked)
            .map_err(|_| BacktestError::Arithmetic("equity exceeds typed range".to_owned()))?;
        peak = peak.max(marked);
        if peak > 0 {
            let drawdown = ((peak - marked) * BPS_SCALE / peak).max(0);
            max_drawdown_bps = max_drawdown_bps.max(u32::try_from(drawdown).unwrap_or(u32::MAX));
        }
        equity.push(EquityPoint {
            timestamp: bar.timestamp,
            equity_micros: marked_i64,
        });

        if index + 1 < bars.len() && index + 1 >= config.slow_window {
            let fast_start = index + 1 - config.fast_window;
            let slow_start = index + 1 - config.slow_window;
            let fast_sum = bars[fast_start..=index]
                .iter()
                .map(|item| i128::from(item.close_micros))
                .sum::<i128>();
            let slow_sum = bars[slow_start..=index]
                .iter()
                .map(|item| i128::from(item.close_micros))
                .sum::<i128>();
            let target_long =
                fast_sum * config.slow_window as i128 > slow_sum * config.fast_window as i128;
            let current_long = quantity > 0;
            if target_long != current_long {
                let executes_at = bars[index + 1].timestamp;
                decisions.push(BacktestDecision {
                    observed_at: bar.timestamp,
                    executes_at,
                    target_long,
                });
                pending = Some((bar.timestamp, target_long));
            }
        }
    }

    let final_equity = equity
        .last()
        .map(|point| point.equity_micros)
        .ok_or_else(|| BacktestError::InvalidBars("no equity observations".to_owned()))?;
    let total_return_bps = ratio_bps(
        i128::from(final_equity) - i128::from(config.initial_cash_micros),
        i128::from(config.initial_cash_micros),
    )?;
    let turnover_bps =
        u32::try_from((turnover * BPS_SCALE / i128::from(config.initial_cash_micros)).max(0))
            .unwrap_or(u32::MAX);
    let run_digest = run_digest(
        &config_digest,
        &data_digest,
        &input_version,
        final_equity,
        &trades,
    );

    Ok(BacktestArtifact {
        schema_version: 1,
        strategy: format!("SMA {}/{} NEXT-OPEN LONG-ONLY", config.fast_window, config.slow_window),
        instrument_id: config.instrument_id.clone(),
        symbol: config.symbol.clone(),
        source,
        quality,
        input_version,
        config_digest,
        data_digest,
        run_digest,
        bars: bars.len(),
        first_timestamp: bars[0].timestamp,
        last_timestamp: bars[bars.len() - 1].timestamp,
        initial_cash_micros: config.initial_cash_micros,
        final_equity_micros: final_equity,
        total_return_bps,
        max_drawdown_bps,
        turnover_bps,
        decisions,
        trades,
        equity,
        open_quantity: quantity,
        methodology: "SIGNAL AT BAR CLOSE · EXECUTION AT NEXT BAR OPEN · INTEGER SHARES · LONG ONLY · MARK TO MARKET AT CLOSE".to_owned(),
        disclosures: vec![
            format!("ALL-IN EXECUTION PENALTY {} BPS EACH SIDE", config.execution_cost_bps),
            format!("FIXED COMMISSION {:.2} EACH FILL", config.commission_micros as f64 / 1_000_000.0),
            "NO BORROW · LEVERAGE · PARTIAL FILLS · IMPACT · DIVIDENDS · TAXES".to_owned(),
            "RESEARCH REPLAY ONLY · NOT A PERFORMANCE PROMISE OR LIVE ORDER PATH".to_owned(),
        ],
    })
}

fn validate_config(config: &BacktestConfig) -> Result<(), BacktestError> {
    if config.instrument_id.trim().is_empty() || config.symbol.trim().is_empty() {
        return Err(BacktestError::InvalidConfig(
            "instrument identity and symbol are required".to_owned(),
        ));
    }
    if config.fast_window < 2
        || config.fast_window >= config.slow_window
        || config.slow_window > MAX_MOVING_AVERAGE_WINDOW
    {
        return Err(BacktestError::InvalidConfig(format!(
            "windows require 2 <= fast < slow <= {MAX_MOVING_AVERAGE_WINDOW}"
        )));
    }
    if config.execution_cost_bps > 1_000 {
        return Err(BacktestError::InvalidConfig(
            "execution cost must be at most 1000 bps".to_owned(),
        ));
    }
    if config.commission_micros < 0 || config.initial_cash_micros <= 0 {
        return Err(BacktestError::InvalidConfig(
            "commission cannot be negative and initial cash must be positive".to_owned(),
        ));
    }
    Ok(())
}

fn validate_bars(bars: &[BacktestBar], slow_window: usize) -> Result<(), BacktestError> {
    if bars.len() <= slow_window || bars.len() > MAX_BACKTEST_BARS {
        return Err(BacktestError::InvalidBars(format!(
            "requires {}-{} observations",
            slow_window + 1,
            MAX_BACKTEST_BARS
        )));
    }
    for (index, bar) in bars.iter().enumerate() {
        if bar.open_micros <= 0
            || bar.high_micros <= 0
            || bar.low_micros <= 0
            || bar.close_micros <= 0
            || bar.low_micros > bar.open_micros
            || bar.low_micros > bar.close_micros
            || bar.high_micros < bar.open_micros
            || bar.high_micros < bar.close_micros
        {
            return Err(BacktestError::InvalidBars(format!(
                "observation {index} has invalid OHLC values"
            )));
        }
        if index > 0 && bars[index - 1].timestamp >= bar.timestamp {
            return Err(BacktestError::InvalidBars(
                "timestamps must be strictly increasing".to_owned(),
            ));
        }
    }
    Ok(())
}

fn apply_cost(price: i64, cost_bps: u32, buy: bool) -> Result<i64, BacktestError> {
    let multiplier = if buy {
        BPS_SCALE + i128::from(cost_bps)
    } else {
        BPS_SCALE - i128::from(cost_bps)
    };
    let adjusted = (i128::from(price) * multiplier + BPS_SCALE / 2) / BPS_SCALE;
    i64::try_from(adjusted)
        .map_err(|_| BacktestError::Arithmetic("execution price overflow".to_owned()))
}

fn ratio_bps(numerator: i128, denominator: i128) -> Result<i32, BacktestError> {
    let value = numerator
        .checked_mul(BPS_SCALE)
        .ok_or_else(|| BacktestError::Arithmetic("return overflow".to_owned()))?
        / denominator;
    i32::try_from(value).map_err(|_| BacktestError::Arithmetic("return exceeds range".to_owned()))
}

fn config_digest(config: &BacktestConfig) -> String {
    let mut hash = Fnv64::new();
    hash.text(&config.instrument_id);
    hash.text(&config.symbol);
    hash.usize(config.fast_window);
    hash.usize(config.slow_window);
    hash.u64(u64::from(config.execution_cost_bps));
    hash.i64(config.commission_micros);
    hash.i64(config.initial_cash_micros);
    hash.finish("CFG")
}

fn data_digest(bars: &[BacktestBar]) -> String {
    let mut hash = Fnv64::new();
    hash.usize(bars.len());
    for bar in bars {
        hash.i64(bar.timestamp);
        hash.i64(bar.open_micros);
        hash.i64(bar.high_micros);
        hash.i64(bar.low_micros);
        hash.i64(bar.close_micros);
        hash.u64(bar.volume);
    }
    hash.finish("DATA")
}

fn run_digest(
    config_digest: &str,
    data_digest: &str,
    input_version: &str,
    final_equity: i64,
    trades: &[BacktestTrade],
) -> String {
    let mut hash = Fnv64::new();
    hash.text(config_digest);
    hash.text(data_digest);
    hash.text(input_version);
    hash.i64(final_equity);
    for trade in trades {
        hash.u64(match trade.side {
            TradeSide::Buy => 1,
            TradeSide::Sell => 2,
        });
        hash.i64(trade.signal_timestamp);
        hash.i64(trade.execution_timestamp);
        hash.u64(trade.quantity);
        hash.i64(trade.execution_price_micros);
    }
    hash.finish("RUN")
}

struct Fnv64(u64);

impl Fnv64 {
    const fn new() -> Self {
        Self(14_695_981_039_346_656_037)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(1_099_511_628_211);
        }
    }
    fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
        self.bytes(&[0xff]);
    }
    fn i64(&mut self, value: i64) {
        self.bytes(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }
    fn usize(&mut self, value: usize) {
        self.u64(value as u64);
    }
    fn finish(self, kind: &str) -> String {
        format!("{kind}-FNV1A64-{:016X}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(closes: &[i64]) -> Vec<BacktestBar> {
        closes
            .iter()
            .enumerate()
            .map(|(index, close)| BacktestBar {
                timestamp: 1_700_000_000 + index as i64 * 86_400,
                open_micros: *close,
                high_micros: close + 100_000,
                low_micros: close - 100_000,
                close_micros: *close,
                volume: 1_000_000,
            })
            .collect()
    }

    fn config() -> BacktestConfig {
        let mut config = BacktestConfig::moving_average_cross("test:aapl", "AAPL");
        config.fast_window = 2;
        config.slow_window = 3;
        config.execution_cost_bps = 0;
        config.commission_micros = 0;
        config
    }

    #[test]
    fn signals_execute_only_at_the_next_open() {
        let input = bars(&[
            10_000_000, 10_000_000, 11_000_000, 12_000_000, 9_000_000, 8_000_000,
        ]);
        let run = run_backtest(&config(), &input, "FIXTURE", "REPLAY", "V1").unwrap();
        assert!(!run.decisions.is_empty());
        for decision in &run.decisions {
            assert!(decision.observed_at < decision.executes_at);
        }
        for trade in &run.trades {
            assert!(trade.signal_timestamp < trade.execution_timestamp);
        }
    }

    #[test]
    fn future_mutation_cannot_change_prior_decisions() {
        let input = bars(&[
            10_000_000, 10_000_000, 11_000_000, 12_000_000, 13_000_000, 14_000_000, 15_000_000,
        ]);
        let original = run_backtest(&config(), &input, "FIXTURE", "REPLAY", "V1").unwrap();
        let mut mutated = input.clone();
        mutated[6].close_micros = 5_000_000;
        mutated[6].low_micros = 4_900_000;
        mutated[6].open_micros = 5_000_000;
        let changed = run_backtest(&config(), &mutated, "FIXTURE", "REPLAY", "V2").unwrap();
        let cutoff = mutated[6].timestamp;
        assert_eq!(
            original
                .decisions
                .iter()
                .filter(|item| item.executes_at < cutoff)
                .collect::<Vec<_>>(),
            changed
                .decisions
                .iter()
                .filter(|item| item.executes_at < cutoff)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn execution_costs_reduce_reconciled_equity() {
        let input = bars(&[
            10_000_000, 10_000_000, 11_000_000, 12_000_000, 13_000_000, 14_000_000,
        ]);
        let free = run_backtest(&config(), &input, "FIXTURE", "REPLAY", "V1").unwrap();
        let mut costly = config();
        costly.execution_cost_bps = 25;
        costly.commission_micros = 5_000_000;
        let costed = run_backtest(&costly, &input, "FIXTURE", "REPLAY", "V1").unwrap();
        assert!(costed.final_equity_micros < free.final_equity_micros);
        assert_ne!(costed.run_digest, free.run_digest);
    }

    #[test]
    fn identical_inputs_reproduce_every_artifact_field() {
        let input = bars(&[
            10_000_000, 10_000_000, 11_000_000, 12_000_000, 13_000_000, 14_000_000,
        ]);
        let first = run_backtest(&config(), &input, "FIXTURE", "REPLAY", "V1").unwrap();
        let second = run_backtest(&config(), &input, "FIXTURE", "REPLAY", "V1").unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn malformed_data_and_unsafe_windows_fail_closed() {
        let input = bars(&[10_000_000, 11_000_000, 12_000_000, 13_000_000]);
        let mut invalid = config();
        invalid.fast_window = 3;
        invalid.slow_window = 3;
        assert!(matches!(
            run_backtest(&invalid, &input, "X", "Y", "Z"),
            Err(BacktestError::InvalidConfig(_))
        ));
        let mut unsorted = input;
        unsorted[2].timestamp = unsorted[1].timestamp;
        assert!(matches!(
            run_backtest(&config(), &unsorted, "X", "Y", "Z"),
            Err(BacktestError::InvalidBars(_))
        ));
    }
}
