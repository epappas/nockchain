use thiserror::Error;

#[derive(Error, Debug)]
pub enum PoolError {
    #[error("Database error: {0}")]
    Database(#[from] redis::RedisError),
    
    #[error("Share validation error: {0}")]
    ShareValidation(String),
    
    #[error("Stratum protocol error: {0}")]
    StratumProtocol(String),
    
    #[error("Payout error: {0}")]
    Payout(String),
    
    #[error("Configuration error: {0}")]
    Configuration(String),
    
    #[error("Miner not found: {0}")]
    MinerNotFound(String),
    
    #[error("Duplicate share")]
    DuplicateShare,
    
    #[error("Invalid proof")]
    InvalidProof,
    
    #[error("Insufficient difficulty")]
    InsufficientDifficulty,
    
    #[error("Job not found: {0}")]
    JobNotFound(String),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("WebSocket error: {0}")]
    WebSocket(String),
    
    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PoolError>;