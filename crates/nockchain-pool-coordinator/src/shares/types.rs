use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareSubmission {
    pub job_id: String,
    pub miner_id: String,
    pub share_type: ShareType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShareType {
    ComputationProof {
        nonce: u64,
        witness_commitment: [u8; 32],
        computation_steps: u64,
    },
    ValidBlock {
        nonce: u64,
        proof: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct ShareValidation {
    pub is_valid: bool,
    pub difficulty: u64,
    pub is_block: bool,
    pub reward_units: u64,
}