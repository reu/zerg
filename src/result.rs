use std::{
    fmt::Display,
    iter::Sum,
    ops::{Add, AddAssign},
    time::Duration,
};

use tdigest::TDigest;

#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub(crate) success: usize,
    pub(crate) http_error: usize,
    pub(crate) tcp_error: usize,
    pub(crate) elapsed: Duration,
    pub(crate) min_time: Duration,
    pub(crate) max_time: Duration,
    pub(crate) timings: Vec<Duration>,
}

impl BenchmarkResult {
    pub fn total_request_count(&self) -> usize {
        self.success + self.http_error
    }

    pub fn requests_per_second(&self) -> f64 {
        (self.total_request_count() as f64 / self.elapsed.as_millis() as f64) * 1000.0
    }

    pub fn success_count(&self) -> usize {
        self.success
    }

    pub fn http_error_count(&self) -> usize {
        self.http_error
    }

    pub fn tcp_error_count(&self) -> usize {
        self.tcp_error
    }

    pub fn total_time(&self) -> Duration {
        self.elapsed
    }

    pub fn timings(&self) -> impl Iterator<Item = Duration> + '_ {
        self.timings.iter().copied()
    }

    pub fn percentiles(&self) -> Percentiles {
        let tdigest = TDigest::new_with_size(100);
        Percentiles(
            tdigest.merge_unsorted(self.timings.iter().map(|dur| dur.as_secs_f64()).collect()),
        )
    }
}

pub struct Percentiles(TDigest);

impl Percentiles {
    pub fn percentile(&self, q: f64) -> Duration {
        Duration::from_secs_f64(self.0.estimate_quantile(q))
    }
}

impl Default for BenchmarkResult {
    fn default() -> Self {
        Self {
            success: Default::default(),
            http_error: Default::default(),
            tcp_error: Default::default(),
            elapsed: Duration::ZERO,
            min_time: Duration::MAX,
            max_time: Duration::ZERO,
            timings: Vec::with_capacity(100000),
        }
    }
}

impl Display for BenchmarkResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seconds = self.elapsed.as_millis() as f64 / 1000.0;
        let total_requests = self.total_request_count();
        writeln!(f, "Requests:  {} in {:.2}s", total_requests, seconds)?;
        writeln!(f, "Reqs/sec:  {:.2}", self.requests_per_second())?;

        if self.success > 0 && self.http_error > 0 {
            writeln!(f, "Success:   {}", self.success)?;
        }
        if self.http_error > 0 {
            writeln!(f, "Errors:    {}", self.http_error)?;
        }
        if self.tcp_error > 0 {
            writeln!(f, "TCP error: {}", self.tcp_error)?;
        }

        let percentiles = self.percentiles();
        let p99 = percentiles.percentile(0.99).as_millis() as f64;
        let p90 = percentiles.percentile(0.90).as_millis() as f64;
        let p75 = percentiles.percentile(0.75).as_millis() as f64;
        let p50 = percentiles.percentile(0.50).as_millis() as f64;

        writeln!(f, "P99:       {p99:.2}ms")?;
        writeln!(f, "P90:       {p90:.2}ms")?;
        writeln!(f, "P75:       {p75:.2}ms")?;
        writeln!(f, "P50:       {p50:.2}ms")?;
        writeln!(f, "Min:       {:.2}ms", self.min_time.as_millis() as f64)?;
        writeln!(f, "Max:       {:.2}ms", self.max_time.as_millis() as f64)?;
        Ok(())
    }
}

impl Sum<BenchmarkResult> for BenchmarkResult {
    fn sum<I: Iterator<Item = BenchmarkResult>>(iter: I) -> Self {
        iter.fold(BenchmarkResult::default(), |total, result| {
            BenchmarkResult {
                success: total.success + result.success,
                http_error: total.http_error + result.http_error,
                tcp_error: total.tcp_error + result.tcp_error,
                elapsed: total.elapsed + result.elapsed,
                min_time: total.min_time.min(result.min_time),
                max_time: total.max_time.max(result.max_time),
                timings: [total.timings, result.timings].concat(),
            }
        })
    }
}

impl AddAssign<BenchmarkResult> for BenchmarkResult {
    fn add_assign(&mut self, mut rhs: BenchmarkResult) {
        self.success += rhs.success;
        self.http_error += rhs.http_error;
        self.tcp_error += rhs.tcp_error;
        self.elapsed += rhs.elapsed;
        if self.min_time > rhs.min_time {
            self.min_time = rhs.min_time;
        }
        if self.max_time < rhs.max_time {
            self.max_time = rhs.max_time;
        }
        self.timings.append(&mut rhs.timings);
    }
}

impl Add<BenchmarkResult> for BenchmarkResult {
    type Output = BenchmarkResult;

    fn add(mut self, rhs: BenchmarkResult) -> Self::Output {
        self += rhs;
        self
    }
}
