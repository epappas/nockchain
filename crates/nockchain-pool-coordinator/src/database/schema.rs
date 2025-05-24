use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerRecord {
    pub address: String,
    pub worker_name: String,
    pub shares_submitted: u64,
    pub shares_valid: u64,
    pub last_share_time: DateTime<Utc>,
    pub total_difficulty: u128,
    pub registration_time: DateTime<Utc>,
    pub is_active: bool,
}

impl MinerRecord {
    pub fn new(address: String, worker_name: String) -> Self {
        let now = Utc::now();
        Self {
            address,
            worker_name,
            shares_submitted: 0,
            shares_valid: 0,
            last_share_time: now,
            total_difficulty: 0,
            registration_time: now,
            is_active: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareRecord {
    pub id: String,
    pub miner_address: String,
    pub job_id: String,
    pub nonce: u64,
    pub difficulty: u64,
    pub timestamp: DateTime<Utc>,
    pub is_valid: bool,
    pub is_block: bool,
    pub reward_units: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobTemplate {
    pub id: String,
    pub block_commitment: Vec<u8>,
    pub target: Vec<u8>,
    pub share_target: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub nonce_ranges: HashMap<String, Range<u64>>,
    pub height: u64,
    pub previous_block: String,
}

impl JobTemplate {
    pub fn calculate_nonce_range(&self, miner_id: &str, total_miners: usize) -> Range<u64> {
        let range_size = u64::MAX / total_miners as u64;
        let miner_hash = sha2::Sha256::digest(miner_id.as_bytes());
        let miner_index = u64::from_le_bytes(miner_hash[0..8].try_into().unwrap()) % total_miners as u64;
        
        let start = miner_index * range_size;
        let end = if miner_index == (total_miners - 1) as u64 {
            u64::MAX
        } else {
            (miner_index + 1) * range_size
        };
        
        start..end
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutQueue {
    pub pending_payouts: Vec<PendingPayout>,
    pub last_payout_time: DateTime<Utc>,
    pub total_paid: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPayout {
    pub miner_address: String,
    pub amount: u64,
    pub shares_window: (DateTime<Utc>, DateTime<Utc>),
    pub share_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerReputation {
    pub miner_address: String,
    pub valid_shares: u64,
    pub invalid_shares: u64,
    pub blocks_found: u64,
    pub last_block_time: Option<DateTime<Utc>>,
    pub reputation_score: f64,
}

impl MinerReputation {
    pub fn new(miner_address: String) -> Self {
        Self {
            miner_address,
            valid_shares: 0,
            invalid_shares: 0,
            blocks_found: 0,
            last_block_time: None,
            reputation_score: 1.0,
        }
    }
    
    pub fn update_reputation(&mut self) {
        let valid_ratio = self.valid_shares as f64 / (self.valid_shares + self.invalid_shares).max(1) as f64;
        let expected_blocks = self.calculate_expected_blocks();
        let block_ratio = self.blocks_found as f64 / expected_blocks.max(1.0);
        
        self.reputation_score = (valid_ratio * 0.7 + block_ratio.min(2.0) * 0.3).max(0.1).min(2.0);
    }
    
    fn calculate_expected_blocks(&self) -> f64 {
        // Simplified calculation - in production would use network difficulty
        self.valid_shares as f64 * 0.00001
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub total_hashrate: f64,
    pub active_miners: u64,
    pub shares_per_second: f64,
    pub average_share_difficulty: u64,
    pub blocks_found_24h: u64,
    pub total_paid_24h: u64,
    pub pool_fee_percent: f64,
}