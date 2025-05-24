use super::schema::*;
use crate::error::{PoolError, Result};
use redis::{aio::ConnectionManager, AsyncCommands};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde_json;
use tracing::{debug, error};

#[derive(Clone)]
pub struct RedisStore {
    client: Arc<redis::Client>,
    conn: ConnectionManager,
}

impl RedisStore {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = Arc::new(redis::Client::open(redis_url)?);
        let conn = ConnectionManager::new(client.clone()).await?;
        
        Ok(Self { client, conn })
    }
    
    // Miner operations
    pub async fn get_miner(&mut self, address: &str) -> Result<Option<MinerRecord>> {
        let key = format!("miner:{}", address);
        let data: Option<String> = self.conn.get(&key).await?;
        
        match data {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }
    
    pub async fn save_miner(&mut self, miner: &MinerRecord) -> Result<()> {
        let key = format!("miner:{}", miner.address);
        let json = serde_json::to_string(miner)?;
        
        self.conn.set(&key, json).await?;
        self.conn.sadd("miners:active", &miner.address).await?;
        
        Ok(())
    }
    
    pub async fn get_active_miners(&mut self) -> Result<Vec<String>> {
        let miners: Vec<String> = self.conn.smembers("miners:active").await?;
        Ok(miners)
    }
    
    // Share operations
    pub async fn save_share(&mut self, share: &ShareRecord) -> Result<()> {
        let key = format!("share:{}", share.id);
        let json = serde_json::to_string(share)?;
        
        // Check for duplicate
        let exists: bool = self.conn.exists(&key).await?;
        if exists {
            return Err(PoolError::DuplicateShare);
        }
        
        // Save share with TTL of 24 hours
        self.conn.set_ex(&key, json, 86400).await?;
        
        // Add to miner's share list
        let miner_shares_key = format!("miner:{}:shares", share.miner_address);
        self.conn.zadd(&miner_shares_key, &share.id, share.timestamp.timestamp()).await?;
        
        // Update share window for PPLNS
        self.conn.zadd("shares:window", &share.id, share.timestamp.timestamp()).await?;
        
        Ok(())
    }
    
    pub async fn get_shares_in_window(&mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<ShareRecord>> {
        let share_ids: Vec<String> = self.conn
            .zrangebyscore("shares:window", start.timestamp(), end.timestamp())
            .await?;
        
        let mut shares = Vec::new();
        for id in share_ids {
            let key = format!("share:{}", id);
            let data: Option<String> = self.conn.get(&key).await?;
            
            if let Some(json) = data {
                shares.push(serde_json::from_str(&json)?);
            }
        }
        
        Ok(shares)
    }
    
    // Job operations
    pub async fn save_job(&mut self, job: &JobTemplate) -> Result<()> {
        let key = format!("job:{}", job.id);
        let json = serde_json::to_string(job)?;
        
        // Save with 1 hour TTL
        self.conn.set_ex(&key, json, 3600).await?;
        
        // Set as current job
        self.conn.set("job:current", &job.id).await?;
        
        Ok(())
    }
    
    pub async fn get_current_job(&mut self) -> Result<Option<JobTemplate>> {
        let job_id: Option<String> = self.conn.get("job:current").await?;
        
        match job_id {
            Some(id) => self.get_job(&id).await,
            None => Ok(None),
        }
    }
    
    pub async fn get_job(&mut self, job_id: &str) -> Result<Option<JobTemplate>> {
        let key = format!("job:{}", job_id);
        let data: Option<String> = self.conn.get(&key).await?;
        
        match data {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }
    
    // Reputation operations
    pub async fn get_reputation(&mut self, miner_address: &str) -> Result<Option<MinerReputation>> {
        let key = format!("reputation:{}", miner_address);
        let data: Option<String> = self.conn.get(&key).await?;
        
        match data {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }
    
    pub async fn save_reputation(&mut self, reputation: &MinerReputation) -> Result<()> {
        let key = format!("reputation:{}", reputation.miner_address);
        let json = serde_json::to_string(reputation)?;
        
        self.conn.set(&key, json).await?;
        Ok(())
    }
    
    // Pool stats
    pub async fn update_pool_stats(&mut self, stats: &PoolStats) -> Result<()> {
        let json = serde_json::to_string(stats)?;
        self.conn.set("pool:stats", json).await?;
        Ok(())
    }
    
    pub async fn get_pool_stats(&mut self) -> Result<Option<PoolStats>> {
        let data: Option<String> = self.conn.get("pool:stats").await?;
        
        match data {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }
    
    // Cleanup operations
    pub async fn cleanup_old_shares(&mut self, before: DateTime<Utc>) -> Result<u64> {
        let removed: u64 = self.conn
            .zremrangebyscore("shares:window", 0, before.timestamp())
            .await?;
        
        debug!("Cleaned up {} old shares", removed);
        Ok(removed)
    }
}