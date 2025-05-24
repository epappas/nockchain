use prometheus::{Encoder, TextEncoder, Counter, Gauge, Histogram, HistogramOpts};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SHARES_SUBMITTED: Counter = Counter::new(
        "pool_shares_submitted_total", 
        "Total number of shares submitted"
    ).unwrap();
    
    pub static ref SHARES_ACCEPTED: Counter = Counter::new(
        "pool_shares_accepted_total",
        "Total number of shares accepted"
    ).unwrap();
    
    pub static ref SHARES_REJECTED: Counter = Counter::new(
        "pool_shares_rejected_total",
        "Total number of shares rejected"
    ).unwrap();
    
    pub static ref BLOCKS_FOUND: Counter = Counter::new(
        "pool_blocks_found_total",
        "Total number of blocks found"
    ).unwrap();
    
    pub static ref ACTIVE_MINERS: Gauge = Gauge::new(
        "pool_active_miners",
        "Number of active miners"
    ).unwrap();
    
    pub static ref POOL_HASHRATE: Gauge = Gauge::new(
        "pool_hashrate_hps",
        "Pool hashrate in hashes per second"
    ).unwrap();
    
    pub static ref SHARE_VALIDATION_TIME: Histogram = Histogram::with_opts(
        HistogramOpts::new("pool_share_validation_seconds", "Time to validate shares")
    ).unwrap();
}

pub fn register_metrics() {
    prometheus::register(Box::new(SHARES_SUBMITTED.clone())).unwrap();
    prometheus::register(Box::new(SHARES_ACCEPTED.clone())).unwrap();
    prometheus::register(Box::new(SHARES_REJECTED.clone())).unwrap();
    prometheus::register(Box::new(BLOCKS_FOUND.clone())).unwrap();
    prometheus::register(Box::new(ACTIVE_MINERS.clone())).unwrap();
    prometheus::register(Box::new(POOL_HASHRATE.clone())).unwrap();
    prometheus::register(Box::new(SHARE_VALIDATION_TIME.clone())).unwrap();
}

pub fn metrics_handler() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}