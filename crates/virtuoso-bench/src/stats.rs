//! Small statistics over samples.

pub fn median(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

pub fn mean(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
}

/// Sample standard deviation.
pub fn stddev(xs: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let m = mean(xs)?;
    Some((xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64).sqrt())
}

/// Coefficient of variation: how much runs differ, relative to their mean.
pub fn cv(xs: &[f64]) -> Option<f64> {
    let m = mean(xs)?;
    let s = stddev(xs)?;
    (m != 0.0).then(|| s / m)
}

pub fn min(xs: &[f64]) -> Option<f64> {
    xs.iter().copied().reduce(f64::min)
}

pub fn max(xs: &[f64]) -> Option<f64> {
    xs.iter().copied().reduce(f64::max)
}
