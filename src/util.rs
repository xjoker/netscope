use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::report::SpeedStats;

pub fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        Some(values[mid])
    } else {
        Some((values[mid - 1] + values[mid]) / 2.0)
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn short_error(err: &anyhow::Error) -> String {
    format!("{err}")
        .lines()
        .next()
        .unwrap_or("unknown error")
        .to_string()
}

pub fn compare_optional_f64(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Compute population standard deviation.
pub fn stddev(samples: &[f64]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let variance = samples.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / samples.len() as f64;
    variance.sqrt()
}

/// Percentile (linear interpolation); samples must be sorted.
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = p / 100.0 * (sorted.len() - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    let frac = idx - lo as f64;
    sorted[lo] + frac * (sorted[hi] - sorted[lo])
}

/// IQR trimming: remove values < Q1 - 1.5*IQR or > Q3 + 1.5*IQR.
/// If the trimmed slice is empty, the original samples are kept.
pub fn iqr_trim(samples: &mut Vec<f64>) {
    if samples.len() < 4 {
        return;
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let q1 = percentile_sorted(samples, 25.0);
    let q3 = percentile_sorted(samples, 75.0);
    let iqr = q3 - q1;
    let lo = q1 - 1.5 * iqr;
    let hi = q3 + 1.5 * iqr;
    let trimmed: Vec<f64> = samples.iter().copied().filter(|&v| v >= lo && v <= hi).collect();
    if !trimmed.is_empty() {
        *samples = trimmed;
    }
}

/// Compute SpeedStats from a list of Mbps samples; applies IQR trimming internally.
pub fn speed_stats_from_samples(mut samples: Vec<f64>) -> SpeedStats {
    if samples.is_empty() {
        return SpeedStats::default();
    }
    iqr_trim(&mut samples);
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let min_mbps = samples.first().copied().unwrap_or(0.0);
    let max_mbps = samples.last().copied().unwrap_or(0.0);
    let avg_mbps = samples.iter().sum::<f64>() / samples.len() as f64;
    let stddev_mbps = stddev(&samples);
    let p25_mbps = percentile_sorted(&samples, 25.0);
    let p75_mbps = percentile_sorted(&samples, 75.0);
    let p90_mbps = percentile_sorted(&samples, 90.0);

    SpeedStats {
        min_mbps,
        avg_mbps,
        max_mbps,
        stddev_mbps,
        p25_mbps,
        p75_mbps,
        p90_mbps,
        min_mbs: min_mbps / 8.0,
        avg_mbs: avg_mbps / 8.0,
        max_mbs: max_mbps / 8.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── median ────────────────────────────────────────────────────────────────

    #[test]
    fn median_odd() {
        let mut v = vec![3.0, 1.0, 2.0];
        assert_eq!(median(&mut v), Some(2.0));
    }

    #[test]
    fn median_even() {
        let mut v = vec![1.0, 4.0, 2.0, 3.0];
        assert_eq!(median(&mut v), Some(2.5));
    }

    #[test]
    fn median_single() {
        let mut v = vec![42.0];
        assert_eq!(median(&mut v), Some(42.0));
    }

    #[test]
    fn median_empty() {
        let mut v: Vec<f64> = vec![];
        assert_eq!(median(&mut v), None);
    }

    // ── stddev ────────────────────────────────────────────────────────────────

    #[test]
    fn stddev_constant() {
        // All same values → stddev = 0
        let v = vec![5.0, 5.0, 5.0, 5.0];
        assert_eq!(stddev(&v), 0.0);
    }

    #[test]
    fn stddev_known() {
        // Population stddev of [2, 4, 4, 4, 5, 5, 7, 9] = 2.0
        let v = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let result = stddev(&v);
        assert!((result - 2.0).abs() < 1e-9, "expected 2.0, got {result}");
    }

    #[test]
    fn stddev_single_returns_zero() {
        assert_eq!(stddev(&[42.0]), 0.0);
    }

    // ── percentile_sorted ─────────────────────────────────────────────────────

    #[test]
    fn percentile_p0_is_min() {
        let v = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile_sorted(&v, 0.0), 1.0);
    }

    #[test]
    fn percentile_p100_is_max() {
        let v = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile_sorted(&v, 100.0), 5.0);
    }

    #[test]
    fn percentile_p50_interpolated() {
        let v = vec![1.0, 2.0, 3.0, 4.0];
        // idx = 0.5 * 3 = 1.5 → lo=1, hi=2, frac=0.5 → 2.0 + 0.5*(3.0-2.0) = 2.5
        let result = percentile_sorted(&v, 50.0);
        assert!((result - 2.5).abs() < 1e-9, "expected 2.5, got {result}");
    }

    #[test]
    fn percentile_single_element() {
        let v = vec![7.0];
        assert_eq!(percentile_sorted(&v, 95.0), 7.0);
    }

    // ── iqr_trim ──────────────────────────────────────────────────────────────

    #[test]
    fn iqr_trim_removes_outlier() {
        // 1000 is a clear outlier; should be trimmed
        let mut v = vec![10.0, 11.0, 12.0, 13.0, 14.0, 1000.0];
        iqr_trim(&mut v);
        assert!(
            !v.contains(&1000.0),
            "outlier 1000.0 should be trimmed, got {v:?}"
        );
    }

    #[test]
    fn iqr_trim_preserves_tight_cluster() {
        let mut v = vec![10.0, 11.0, 12.0, 13.0];
        let original_len = v.len();
        iqr_trim(&mut v);
        // Tight cluster: nothing trimmed
        assert_eq!(v.len(), original_len);
    }

    #[test]
    fn iqr_trim_less_than_4_noop() {
        let mut v = vec![1.0, 100.0, 1000.0];
        let original = v.clone();
        iqr_trim(&mut v);
        assert_eq!(v, original, "should not trim when fewer than 4 samples");
    }

    // ── speed_stats_from_samples ───────────────────────────────────────────────

    #[test]
    fn speed_stats_empty_returns_default() {
        let stats = speed_stats_from_samples(vec![]);
        assert_eq!(stats.avg_mbps, 0.0);
        assert_eq!(stats.max_mbps, 0.0);
    }

    #[test]
    fn speed_stats_single_sample() {
        let stats = speed_stats_from_samples(vec![100.0]);
        assert_eq!(stats.min_mbps, 100.0);
        assert_eq!(stats.max_mbps, 100.0);
        assert_eq!(stats.avg_mbps, 100.0);
    }

    #[test]
    fn speed_stats_mbs_equals_mbps_over_8() {
        let stats = speed_stats_from_samples(vec![80.0, 80.0, 80.0, 80.0, 80.0]);
        let expected = 80.0 / 8.0;
        assert!(
            (stats.avg_mbs - expected).abs() < 1e-9,
            "avg_mbs should be avg_mbps / 8"
        );
    }

    #[test]
    fn speed_stats_outlier_trimmed() {
        // 999.9 is a clear outlier in an otherwise tight cluster at ~10 Mbps
        let samples = vec![10.0, 10.5, 11.0, 10.2, 10.8, 999.9];
        let stats = speed_stats_from_samples(samples);
        assert!(
            stats.max_mbps < 100.0,
            "outlier should be trimmed, max was {}",
            stats.max_mbps
        );
    }

    // ── compare_optional_f64 ─────────────────────────────────────────────────

    #[test]
    fn compare_optional_both_some() {
        use std::cmp::Ordering;
        assert_eq!(compare_optional_f64(Some(1.0), Some(2.0)), Ordering::Less);
        assert_eq!(compare_optional_f64(Some(2.0), Some(1.0)), Ordering::Greater);
        assert_eq!(compare_optional_f64(Some(1.0), Some(1.0)), Ordering::Equal);
    }

    #[test]
    fn compare_optional_none_is_greatest() {
        use std::cmp::Ordering;
        // Some < None (None sorts last)
        assert_eq!(compare_optional_f64(Some(1.0), None), Ordering::Less);
        assert_eq!(compare_optional_f64(None, Some(1.0)), Ordering::Greater);
        assert_eq!(compare_optional_f64(None, None), Ordering::Equal);
    }

    // ── short_error ───────────────────────────────────────────────────────────

    #[test]
    fn short_error_takes_first_line() {
        let err = anyhow::anyhow!("line one\nline two\nline three");
        let result = short_error(&err);
        assert_eq!(result, "line one");
    }
}
