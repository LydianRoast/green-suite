// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
use super::*;
#[derive(Default)]
pub struct SeriesOptions {
    pub clocks_confirmed: bool,
    pub time_weighted: bool,
    pub max_gap_seconds: f64,
}
pub struct SeriesResult {
    pub plot: Vec<(f64, f64, f64)>,
    pub plot_uses_time: bool,
    pub duration_seconds: Option<f64>,
    pub dispersion_unit: Unit,
    pub count: usize,
    pub minimum: f64,
    pub maximum: f64,
    pub mean: f64,
    pub sample_stddev: Option<f64>,
    pub missing_times: usize,
    pub backward_steps: usize,
    pub duplicate_times: usize,
    pub maximum_gap_seconds: Option<f64>,
    pub clock_established: bool,
    pub time_weighted_mean: Option<f64>,
    pub result: Reading,
}
pub fn series(
    readings: &[&Reading],
    target: Unit,
    basis: Basis,
    options: &SeriesOptions,
) -> Result<SeriesResult> {
    ensure!(
        !readings.is_empty() && readings.len() <= MAX_INPUTS,
        "Select between 1 and 100,000 readings"
    );
    ensure!(
        options.max_gap_seconds.is_finite() && options.max_gap_seconds >= 0.,
        "Gap limit must be finite and nonnegative"
    );
    let values = readings
        .iter()
        .map(|r| value_in(r, target, basis))
        .collect::<Result<Vec<_>>>()?;
    let scale = values
        .iter()
        .fold(0f64, |v, x| v.max(x.abs()))
        .max(f64::MIN_POSITIVE);
    let mut mean = 0.;
    let mut m2 = 0.;
    for (index, value) in values.iter().enumerate() {
        let x = *value / scale;
        let delta = x - mean;
        mean += delta / (index + 1) as f64;
        m2 += delta * (x - mean);
    }
    let sample_stddev =
        (values.len() > 1).then(|| (m2.max(0.) / (values.len() - 1) as f64).sqrt() * scale);
    let mean = mean * scale;
    ensure!(
        mean.is_finite() && sample_stddev.is_none_or(f64::is_finite),
        "Statistics exceed the numeric range"
    );
    let stamps: Vec<_> = readings.iter().map(|r| stamp(r).ok()).collect();
    let missing_times = stamps.iter().filter(|s| s.is_none()).count();
    let clock_established = stamps.iter().all(|s| {
        s.as_ref().is_some_and(|s| {
            comparable(s, stamps[0].as_ref().unwrap_or(s), options.clocks_confirmed)
        })
    });
    let mut backward_steps = 0;
    let mut duplicate_times = 0;
    let mut max_gap = 0f64;
    if clock_established {
        for pair in stamps.windows(2) {
            let delta = (pair[1].as_ref().unwrap().time - pair[0].as_ref().unwrap().time)
                .num_milliseconds() as f64
                / 1000.;
            if delta < 0. {
                backward_steps += 1;
            } else if delta == 0. {
                duplicate_times += 1;
            } else {
                max_gap = max_gap.max(delta);
            }
        }
    }
    let mut weighted = None;
    if options.time_weighted {
        ensure!(
            readings.len() > 1 && clock_established && backward_steps == 0 && duplicate_times == 0,
            "Time-weighted analysis requires comparable, complete, strictly increasing timestamps"
        );
        ensure!(
            max_gap <= options.max_gap_seconds,
            "A sampling gap exceeds the chosen hold-interval limit"
        );
        let span = (stamps.last().unwrap().as_ref().unwrap().time
            - stamps[0].as_ref().unwrap().time)
            .num_milliseconds() as f64
            / 1000.;
        let mut total = 0.;
        for (index, pair) in stamps.windows(2).enumerate() {
            let delta = (pair[1].as_ref().unwrap().time - pair[0].as_ref().unwrap().time)
                .num_milliseconds() as f64
                / 1000.;
            total += (values[index] / scale) * (delta / span);
        }
        let value = total * scale;
        ensure!(value.is_finite(), "Time-weighted mean overflow");
        weighted = Some(value);
    }
    let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let first = stamps.first().and_then(|s| s.as_ref()).map(|s| s.time);
    let span = first
        .zip(stamps.last().and_then(|s| s.as_ref()).map(|s| s.time))
        .map(|(a, b)| (b - a).num_milliseconds() as f64 / 1000.)
        .unwrap_or(0.);
    let plot_uses_time = clock_established && backward_steps == 0 && span > 0.;
    let mut buckets: Vec<Option<(f64, f64)>> = vec![None; 600];
    for (index, &value) in values.iter().enumerate() {
        let x = if plot_uses_time {
            (stamps[index].as_ref().unwrap().time - first.unwrap()).num_milliseconds() as f64
                / 1000.
                / span
        } else {
            index as f64 / (values.len().saturating_sub(1).max(1)) as f64
        };
        let bucket = ((x * 599.).round() as usize).min(599);
        buckets[bucket] = Some(
            buckets[bucket]
                .map(|(min, max)| (min.min(value), max.max(value)))
                .unwrap_or((value, value)),
        );
    }
    let plot = buckets
        .into_iter()
        .enumerate()
        .filter_map(|(i, b)| b.map(|(min, max)| (i as f64 / 599., min, max)))
        .collect();
    let notes = BTreeMap::from([
        ("calculation_basis".into(), basis.label().into()),
        ("unit_rule_version".into(), basis.rule_id().into()),
        (
            "dispersion_unit".into(),
            match target {
                Unit::Celsius => Unit::DeltaC,
                Unit::Fahrenheit => Unit::DeltaF,
                Unit::Kelvin => Unit::DeltaK,
                _ => target,
            }
            .symbol()
            .into(),
        ),
        ("sample_count".into(), values.len().to_string()),
        ("minimum".into(), minimum.to_string()),
        ("maximum".into(), maximum.to_string()),
        (
            "sample_stddev".into(),
            sample_stddev
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Not defined for one sample".into()),
        ),
        ("missing_timestamps".into(), missing_times.to_string()),
        ("backward_steps".into(), backward_steps.to_string()),
        ("duplicate_timestamps".into(), duplicate_times.to_string()),
        ("clock_established".into(), clock_established.to_string()),
        (
            "clocks_confirmed_by_user".into(),
            options.clocks_confirmed.to_string(),
        ),
        (
            "weighting".into(),
            if options.time_weighted {
                "Hold previous sample until next; final sample has no duration"
            } else {
                "Equal weight per stored reading"
            }
            .into(),
        ),
        (
            "maximum_gap_seconds".into(),
            if clock_established {
                max_gap.to_string()
            } else {
                "Unknown".into()
            },
        ),
        (
            "hold_interval_limit_seconds".into(),
            options.max_gap_seconds.to_string(),
        ),
    ]);
    let result = derived(
        &readings[0].group,
        if options.time_weighted {
            "Time-weighted mean"
        } else {
            "Sample mean"
        },
        weighted.unwrap_or(mean),
        target,
        "stored-series-summary-v1",
        readings,
        notes,
    )?;
    Ok(SeriesResult {
        plot,
        plot_uses_time,
        duration_seconds: plot_uses_time.then_some(span),
        dispersion_unit: match target {
            Unit::Celsius => Unit::DeltaC,
            Unit::Fahrenheit => Unit::DeltaF,
            Unit::Kelvin => Unit::DeltaK,
            _ => target,
        },
        count: values.len(),
        minimum,
        maximum,
        mean,
        sample_stddev,
        missing_times,
        backward_steps,
        duplicate_times,
        maximum_gap_seconds: clock_established.then_some(max_gap),
        clock_established,
        time_weighted_mean: weighted,
        result,
    })
}
