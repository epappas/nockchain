use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::database::{RedisStore, JobTemplate};
use crate::error::{PoolError, Result};
use super::types::{ShareSubmission, ShareType, ShareValidation};
use super::computation_proof::ComputationProof;

const SPOT_CHECK_COUNT: usize = 5;
const BLOCK_REWARD_UNITS: u64 = 1_000_000;

pub struct ShareValidator {
    redis: Arc<RwLock<RedisStore>>,
    validation_threshold: f64,
}

impl ShareValidator {
    pub fn new(redis: Arc<RwLock<RedisStore>>, validation_threshold: f64) -> Self {
        Self {
            redis,
            validation_threshold,
        }
    }
    
    pub async fn validate_share(
        &self,
        share: ShareSubmission,
    ) -> Result<ShareValidation> {
        // Check if share is duplicate
        if self.is_duplicate(&share).await? {
            return Err(PoolError::DuplicateShare);
        }
        
        match &share.share_type {
            ShareType::ComputationProof { witness_commitment, computation_steps, nonce } => {
                self.validate_computation_proof(&share.job_id, *witness_commitment, *computation_steps, *nonce).await
            }
            ShareType::ValidBlock { proof, nonce } => {
                self.validate_block(&share.job_id, proof, *nonce).await
            }
        }
    }
    
    async fn is_duplicate(&self, share: &ShareSubmission) -> Result<bool> {
        // Generate unique share ID based on job_id, miner_id, and nonce
        let share_id = match &share.share_type {
            ShareType::ComputationProof { nonce, .. } => {
                format!("{}:{}:{}", share.job_id, share.miner_id, nonce)
            }
            ShareType::ValidBlock { nonce, .. } => {
                format!("{}:{}:{}", share.job_id, share.miner_id, nonce)
            }
        };
        
        // Check in Redis if this share ID already exists
        // This is simplified - in production would use proper duplicate detection
        Ok(false)
    }
    
    async fn validate_computation_proof(
        &self,
        job_id: &str,
        witness_commitment: [u8; 32],
        computation_steps: u64,
        nonce: u64,
    ) -> Result<ShareValidation> {
        // Get job template
        let job = self.get_job_template(job_id).await?;
        
        // Create a computation proof for verification
        let proof = ComputationProof {
            witness_commitment,
            nonce_range: nonce..nonce + 1,
            computation_steps,
            intermediate_hashes: vec![witness_commitment],
        };
        
        // Verify proof
        if !proof.verify(&job.block_commitment, SPOT_CHECK_COUNT) {
            return Err(PoolError::InvalidProof);
        }
        
        // Calculate share difficulty
        let difficulty = self.calculate_share_difficulty(&witness_commitment);
        
        Ok(ShareValidation {
            is_valid: true,
            difficulty,
            is_block: false,
            reward_units: difficulty * computation_steps,
        })
    }
    
    async fn validate_block(
        &self,
        job_id: &str,
        proof: &[u8],
        nonce: u64,
    ) -> Result<ShareValidation> {
        // Get job template
        let job = self.get_job_template(job_id).await?;
        
        // In production, would verify STARK proof here
        // For now, we'll do a simplified check
        let proof_hash = sha2::Sha256::digest(proof);
        
        // Check if meets target
        if !self.meets_target(&proof_hash, &job.target) {
            return Err(PoolError::InsufficientDifficulty);
        }
        
        debug!("Valid block found for job {} with nonce {}", job_id, nonce);
        
        Ok(ShareValidation {
            is_valid: true,
            difficulty: self.calculate_block_difficulty(&job.target),
            is_block: true,
            reward_units: BLOCK_REWARD_UNITS,
        })
    }
    
    async fn get_job_template(&self, job_id: &str) -> Result<JobTemplate> {
        let mut redis = self.redis.write().await;
        redis.get_job(job_id).await?
            .ok_or_else(|| PoolError::JobNotFound(job_id.to_string()))
    }
    
    fn calculate_share_difficulty(&self, commitment: &[u8; 32]) -> u64 {
        // Simplified difficulty calculation based on leading zeros
        let mut difficulty = 0u64;
        for byte in commitment {
            if *byte == 0 {
                difficulty += 256;
            } else {
                difficulty += byte.leading_zeros() as u64 * 32;
                break;
            }
        }
        difficulty.max(1)
    }
    
    fn calculate_block_difficulty(&self, target: &[u8]) -> u64 {
        // Calculate difficulty from target
        // Simplified version - in production would use proper difficulty calculation
        let mut difficulty = 0u64;
        for byte in target {
            if *byte == 0xff {
                difficulty += 256;
            } else {
                break;
            }
        }
        difficulty.max(1) * 1000 // Scale up for blocks
    }
    
    fn meets_target(&self, hash: &[u8], target: &[u8]) -> bool {
        // Check if hash is less than target
        for (h, t) in hash.iter().zip(target.iter()) {
            match h.cmp(t) {
                std::cmp::Ordering::Less => return true,
                std::cmp::Ordering::Greater => return false,
                std::cmp::Ordering::Equal => continue,
            }
        }
        true
    }
}