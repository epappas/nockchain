use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc, Duration};
use tracing::{info, warn, error, debug};
use serde_json::json;

use crate::database::{RedisStore, MinerRecord, ShareRecord, JobTemplate, PoolStats, MinerReputation};
use crate::shares::{ShareSubmission, ShareValidator, ShareValidation};
use crate::payout::PayoutManager;
use crate::error::{PoolError, Result};

pub struct PoolCoordinator {
    redis: Arc<RwLock<RedisStore>>,
    share_validator: Arc<ShareValidator>,
    payout_manager: Arc<PayoutManager>,
    config: PoolConfig,
}

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub pool_name: String,
    pub fee_percent: f64,
    pub min_payout: u64,
    pub payout_interval: u64,
    pub share_window_hours: u64,
    pub validation_threshold: f64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            pool_name: "Nockchain Mining Pool".to_string(),
            fee_percent: 2.0,
            min_payout: 1_000_000,
            payout_interval: 3600,
            share_window_hours: 24,
            validation_threshold: 0.95,
        }
    }
}

impl PoolCoordinator {
    pub async fn new(redis_url: &str, config: PoolConfig) -> Result<Self> {
        let redis = Arc::new(RwLock::new(RedisStore::new(redis_url).await?));
        let share_validator = Arc::new(ShareValidator::new(redis.clone(), config.validation_threshold));
        let payout_manager = Arc::new(PayoutManager::new(redis.clone(), config.fee_percent));
        
        Ok(Self {
            redis,
            share_validator,
            payout_manager,
            config,
        })
    }
    
    pub async fn register_miner(&self, address: &str, worker_name: &str) -> Result<()> {
        let mut redis = self.redis.write().await;
        
        // Check if miner exists
        let miner = match redis.get_miner(address).await? {
            Some(mut miner) => {
                miner.worker_name = worker_name.to_string();
                miner.is_active = true;
                miner
            }
            None => MinerRecord::new(address.to_string(), worker_name.to_string()),
        };
        
        redis.save_miner(&miner).await?;
        info!("Registered miner {} as {}", address, worker_name);
        
        Ok(())
    }
    
    pub async fn unregister_miner(&self, worker_name: &str) -> Result<()> {
        // In production, would mark miner as inactive
        debug!("Miner {} disconnected", worker_name);
        Ok(())
    }
    
    pub async fn submit_share(&self, submission: ShareSubmission) -> Result<ShareValidation> {
        // Validate share
        let validation = self.share_validator.validate_share(submission.clone()).await?;
        
        if validation.is_valid {
            // Save share record
            let share_record = ShareRecord {
                id: uuid::Uuid::new_v4().to_string(),
                miner_address: submission.miner_id.clone(),
                job_id: submission.job_id,
                nonce: match &submission.share_type {
                    crate::shares::ShareType::ComputationProof { nonce, .. } => *nonce,
                    crate::shares::ShareType::ValidBlock { nonce, .. } => *nonce,
                },
                difficulty: validation.difficulty,
                timestamp: Utc::now(),
                is_valid: true,
                is_block: validation.is_block,
                reward_units: validation.reward_units,
            };
            
            let mut redis = self.redis.write().await;
            redis.save_share(&share_record).await?;
            
            // Update miner stats
            if let Some(mut miner) = redis.get_miner(&submission.miner_id).await? {
                miner.shares_submitted += 1;
                miner.shares_valid += 1;
                miner.last_share_time = Utc::now();
                miner.total_difficulty += validation.difficulty as u128;
                redis.save_miner(&miner).await?;
            }
            
            // Update reputation
            if let Some(mut reputation) = redis.get_reputation(&submission.miner_id).await? {
                reputation.valid_shares += 1;
                if validation.is_block {
                    reputation.blocks_found += 1;
                    reputation.last_block_time = Some(Utc::now());
                }
                reputation.update_reputation();
                redis.save_reputation(&reputation).await?;
            } else {
                let mut reputation = MinerReputation::new(submission.miner_id.clone());
                reputation.valid_shares = 1;
                if validation.is_block {
                    reputation.blocks_found = 1;
                    reputation.last_block_time = Some(Utc::now());
                }
                redis.save_reputation(&reputation).await?;
            }
            
            if validation.is_block {
                info!("BLOCK FOUND by miner {}", submission.miner_id);
                // Trigger payout calculation
                self.trigger_block_payout(&share_record).await?;
            }
        }
        
        Ok(validation)
    }
    
    pub async fn new_job(&self, job: JobTemplate) -> Result<()> {
        let mut redis = self.redis.write().await;
        redis.save_job(&job).await?;
        info!("New job {} at height {}", job.id, job.height);
        Ok(())
    }
    
    pub async fn get_current_job(&self) -> Result<Option<JobTemplate>> {
        let mut redis = self.redis.write().await;
        redis.get_current_job().await
    }
    
    pub async fn get_pool_stats(&self) -> Result<PoolStats> {
        let mut redis = self.redis.write().await;
        
        // Get active miners
        let active_miners = redis.get_active_miners().await?.len() as u64;
        
        // Calculate other stats
        let now = Utc::now();
        let window_start = now - Duration::hours(self.config.share_window_hours as i64);
        let shares = redis.get_shares_in_window(window_start, now).await?;
        
        let total_difficulty: u64 = shares.iter().map(|s| s.difficulty).sum();
        let shares_per_second = shares.len() as f64 / (self.config.share_window_hours as f64 * 3600.0);
        let average_share_difficulty = if shares.is_empty() { 
            0 
        } else { 
            total_difficulty / shares.len() as u64 
        };
        
        let blocks_found_24h = shares.iter().filter(|s| s.is_block).count() as u64;
        
        // Estimate hashrate (simplified)
        let total_hashrate = total_difficulty as f64 / (self.config.share_window_hours as f64 * 3600.0);
        
        let stats = PoolStats {
            total_hashrate,
            active_miners,
            shares_per_second,
            average_share_difficulty,
            blocks_found_24h,
            total_paid_24h: 0, // Would be calculated from payout records
            pool_fee_percent: self.config.fee_percent,
        };
        
        redis.update_pool_stats(&stats).await?;
        Ok(stats)
    }
    
    pub async fn get_miner_stats(&self, address: &str) -> Result<serde_json::Value> {
        let mut redis = self.redis.write().await;
        
        let miner = redis.get_miner(address).await?
            .ok_or_else(|| PoolError::MinerNotFound(address.to_string()))?;
        
        let reputation = redis.get_reputation(address).await?
            .unwrap_or_else(|| MinerReputation::new(address.to_string()));
        
        Ok(json!({
            "address": miner.address,
            "worker_name": miner.worker_name,
            "shares_submitted": miner.shares_submitted,
            "shares_valid": miner.shares_valid,
            "total_difficulty": miner.total_difficulty.to_string(),
            "last_share_time": miner.last_share_time,
            "blocks_found": reputation.blocks_found,
            "reputation_score": reputation.reputation_score,
            "is_active": miner.is_active,
        }))
    }
    
    async fn trigger_block_payout(&self, block_share: &ShareRecord) -> Result<()> {
        // Calculate rewards for all miners in the share window
        let now = Utc::now();
        let window_start = now - Duration::hours(self.config.share_window_hours as i64);
        
        info!("Calculating payouts for block found at {}", block_share.timestamp);
        
        // This would trigger the payout manager to calculate and queue payouts
        // In production, would be more sophisticated
        Ok(())
    }
    
    pub async fn run_maintenance(&self) -> Result<()> {
        // Clean up old shares
        let cutoff = Utc::now() - Duration::hours(48);
        let mut redis = self.redis.write().await;
        let removed = redis.cleanup_old_shares(cutoff).await?;
        
        if removed > 0 {
            debug!("Cleaned up {} old shares", removed);
        }
        
        Ok(())
    }
}