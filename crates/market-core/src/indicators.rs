//! Technical indicators computing module.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]

/// Result of MACD computation.
#[derive(Debug, Clone, Default)]
pub struct MacdResult {
    pub macd_line: Vec<f64>,
    pub signal_line: Vec<f64>,
    pub histogram: Vec<f64>,
}

/// Compute Simple Moving Average.
#[must_use]
pub fn compute_sma(prices: &[f64], period: usize) -> Vec<f64> {
    if prices.len() < period || period == 0 {
        return vec![0.0; prices.len()];
    }

    let mut result = Vec::with_capacity(prices.len());

    // Fill initial uncomputable values with NaN or 0 (using 0.0 for simplicity in charts)
    for _ in 0..period - 1 {
        result.push(f64::NAN);
    }

    for i in (period - 1)..prices.len() {
        let window = &prices[i + 1 - period..=i];
        let sum: f64 = window.iter().sum();
        result.push(sum / period as f64);
    }

    result
}

/// Compute Exponential Moving Average.
#[must_use]
pub fn compute_ema(prices: &[f64], period: usize) -> Vec<f64> {
    if prices.len() < period || period == 0 {
        return vec![0.0; prices.len()];
    }

    let mut result = Vec::with_capacity(prices.len());
    let multiplier = 2.0 / (period as f64 + 1.0);

    // Initial SMA for first EMA point
    let initial_sum: f64 = prices[..period].iter().sum();
    let initial_sma = initial_sum / period as f64;

    for _ in 0..period - 1 {
        result.push(f64::NAN);
    }
    result.push(initial_sma);

    for i in period..prices.len() {
        let prev_ema = result[i - 1];
        let ema = (prices[i] - prev_ema) * multiplier + prev_ema;
        result.push(ema);
    }

    result
}

/// Compute Relative Strength Index.
#[must_use]
pub fn compute_rsi(prices: &[f64], period: usize) -> Vec<f64> {
    if prices.len() <= period || period == 0 {
        return vec![50.0; prices.len()]; // Default neutral
    }

    let mut result = Vec::with_capacity(prices.len());
    for _ in 0..period {
        result.push(f64::NAN);
    }

    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;

    // Initial RS based on simple averages
    for i in 1..=period {
        let change = prices[i] - prices[i - 1];
        if change > 0.0 {
            avg_gain += change;
        } else {
            avg_loss += change.abs();
        }
    }

    avg_gain /= period as f64;
    avg_loss /= period as f64;

    let initial_rs = if avg_loss == 0.0 {
        f64::MAX
    } else {
        avg_gain / avg_loss
    };
    let initial_rsi = if avg_loss == 0.0 {
        100.0
    } else {
        100.0 - (100.0 / (1.0 + initial_rs))
    };
    result.push(initial_rsi);

    for i in period + 1..prices.len() {
        let change = prices[i] - prices[i - 1];
        let gain = if change > 0.0 { change } else { 0.0 };
        let loss = if change < 0.0 { change.abs() } else { 0.0 };

        avg_gain = (avg_gain * (period as f64 - 1.0) + gain) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + loss) / period as f64;

        let rs = if avg_loss == 0.0 {
            f64::MAX
        } else {
            avg_gain / avg_loss
        };
        let rsi = if avg_loss == 0.0 {
            100.0
        } else {
            100.0 - (100.0 / (1.0 + rs))
        };
        result.push(rsi);
    }

    result
}

/// Compute Moving Average Convergence Divergence.
#[must_use]
pub fn compute_macd(prices: &[f64]) -> MacdResult {
    let fast_period = 12;
    let slow_period = 26;
    let signal_period = 9;

    let fast_ema = compute_ema(prices, fast_period);
    let slow_ema = compute_ema(prices, slow_period);

    let mut macd_line = Vec::with_capacity(prices.len());
    for i in 0..prices.len() {
        if fast_ema[i].is_nan() || slow_ema[i].is_nan() {
            macd_line.push(f64::NAN);
        } else {
            macd_line.push(fast_ema[i] - slow_ema[i]);
        }
    }

    // To compute signal line (EMA of MACD line), we need to extract the valid part
    let valid_start = slow_period - 1; // Since slow_period > fast_period

    let mut signal_line = vec![f64::NAN; prices.len()];
    let mut histogram = vec![f64::NAN; prices.len()];

    if prices.len() > valid_start + signal_period {
        let valid_macd = &macd_line[valid_start..];
        let valid_signal = compute_ema(valid_macd, signal_period);

        for i in valid_start..prices.len() {
            signal_line[i] = valid_signal[i - valid_start];
            if !macd_line[i].is_nan() && !signal_line[i].is_nan() {
                histogram[i] = macd_line[i] - signal_line[i];
            }
        }
    }

    MacdResult {
        macd_line,
        signal_line,
        histogram,
    }
}
