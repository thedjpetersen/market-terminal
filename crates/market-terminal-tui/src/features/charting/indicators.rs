//! Indicator math over close series. Pure functions are index-aligned with the
//! input: `out[i]` is the indicator value at observation `i`, and `None` means
//! the lookback window is still filling.
//!
//! Adapted from `makeev/alphai-tui` at commit
//! `9143d2e1176d0a67a9f26960427cf370187fc2e6`.
//! Copyright (c) 2026 Mikhail Makeev, used under the MIT License. See
//! `THIRD_PARTY_NOTICES.md` at the repository root.

pub(crate) const MOVING_AVERAGE_FAST: usize = 20;
pub(crate) const MOVING_AVERAGE_SLOW: usize = 100;
pub(crate) const RSI_PERIOD: usize = 14;

/// Simple moving average. `None` until a full `period` window is available.
pub(crate) fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut output = vec![None; values.len()];
    if period == 0 || period > values.len() {
        return output;
    }

    let mut sum: f64 = values[..period].iter().sum();
    output[period - 1] = Some(sum / period as f64);
    for index in period..values.len() {
        sum += values[index] - values[index - period];
        output[index] = Some(sum / period as f64);
    }
    output
}

/// Exponential moving average seeded with the first simple average, so it
/// starts on the same observation as an SMA with the same period.
pub(crate) fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut output = vec![None; values.len()];
    if period == 0 || period > values.len() {
        return output;
    }

    let mut previous = values[..period].iter().sum::<f64>() / period as f64;
    output[period - 1] = Some(previous);
    let weight = 2.0 / (period as f64 + 1.0);
    for index in period..values.len() {
        previous += weight * (values[index] - previous);
        output[index] = Some(previous);
    }
    output
}

/// Relative strength index using Wilder smoothing. `None` for the first
/// `period` observations.
pub(crate) fn rsi(closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut output = vec![None; closes.len()];
    if period == 0 || closes.len() <= period {
        return output;
    }

    let (mut average_gain, mut average_loss) = (0.0, 0.0);
    for index in 1..=period {
        let delta = closes[index] - closes[index - 1];
        average_gain += delta.max(0.0);
        average_loss += (-delta).max(0.0);
    }
    average_gain /= period as f64;
    average_loss /= period as f64;
    output[period] = Some(rsi_from(average_gain, average_loss));

    for index in period + 1..closes.len() {
        let delta = closes[index] - closes[index - 1];
        average_gain = (average_gain * (period - 1) as f64 + delta.max(0.0)) / period as f64;
        average_loss = (average_loss * (period - 1) as f64 + (-delta).max(0.0)) / period as f64;
        output[index] = Some(rsi_from(average_gain, average_loss));
    }
    output
}

fn rsi_from(average_gain: f64, average_loss: f64) -> f64 {
    if average_loss == 0.0 {
        if average_gain == 0.0 {
            50.0
        } else {
            100.0
        }
    } else {
        100.0 - 100.0 / (1.0 + average_gain / average_loss)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_average_is_index_aligned() {
        assert_eq!(
            sma(&[1.0, 2.0, 3.0, 4.0, 5.0], 3),
            vec![None, None, Some(2.0), Some(3.0), Some(4.0)]
        );
        assert_eq!(sma(&[1.0, 2.0], 0), vec![None, None]);
        assert_eq!(sma(&[1.0, 2.0], 3), vec![None, None]);
    }

    #[test]
    fn exponential_average_seeds_on_the_simple_average() {
        let closes = [1.0, 2.0, 3.0, 10.0];
        let output = ema(&closes, 2);
        assert_eq!(output[0], None);
        assert_eq!(output[1], sma(&closes, 2)[1]);
        for (index, expected) in [(1, 1.5), (2, 2.5), (3, 7.5)] {
            let actual = output[index].expect("EMA should be warm");
            assert!((actual - expected).abs() < 1e-9);
        }
        assert!(output[3] > sma(&closes, 2)[3]);
    }

    #[test]
    fn exponential_average_handles_flat_and_degenerate_series() {
        let output = ema(&[42.0; 8], 3);
        assert!(output[2..].iter().all(|value| *value == Some(42.0)));
        assert_eq!(ema(&[1.0, 2.0], 0), vec![None, None]);
        assert_eq!(ema(&[1.0, 2.0], 3), vec![None, None]);
        assert_eq!(ema(&[], 3), Vec::<Option<f64>>::new());
    }

    #[test]
    fn rsi_handles_warmup_and_direction() {
        let rising = (0..20)
            .map(|index| 100.0 + f64::from(index))
            .collect::<Vec<_>>();
        let falling = (0..20)
            .map(|index| 100.0 - f64::from(index))
            .collect::<Vec<_>>();
        let rising_rsi = rsi(&rising, RSI_PERIOD);
        assert!(rising_rsi[..RSI_PERIOD].iter().all(Option::is_none));
        assert_eq!(rising_rsi[19], Some(100.0));
        assert!(rsi(&falling, RSI_PERIOD)[19].expect("RSI should be warm") < 1e-9);
        assert_eq!(rsi(&[42.0; 20], RSI_PERIOD)[19], Some(50.0));
    }

    #[test]
    fn rsi_matches_wilders_reference_series() {
        let closes = [
            44.3389, 44.0902, 44.1497, 43.6124, 44.3278, 44.8264, 45.0955, 45.4245, 45.8433,
            46.0826, 45.8931, 46.0328, 45.6140, 46.2820, 46.2820, 46.0028, 46.0328, 46.4116,
            46.2222,
        ];
        let output = rsi(&closes, RSI_PERIOD);
        assert!((output[14].expect("RSI should be warm") - 70.46).abs() < 0.3);
        assert!((output[15].expect("RSI should be warm") - 66.25).abs() < 0.3);
    }
}
