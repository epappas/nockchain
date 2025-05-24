# Nockchain Mining Pool Protocol Design

## Executive Summary

This document outlines a comprehensive mining pool protocol design for Nockchain that addresses the unique challenges of STARK-based proof-of-work. The protocol supports both centralized and decentralized pool architectures while maintaining the security properties of the underlying blockchain.

## Key Challenges

### 1. STARK Proof Indivisibility

Unlike traditional hash-based PoW where partial solutions exist (shares), STARK proofs are binary - either valid or invalid. This requires a novel approach to work distribution and reward allocation.

### 2. High Computational Cost

STARK proof generation is computationally intensive, making it crucial to minimize redundant work and maximize efficiency across pool participants.

### 3. Kernel State Management

Each mining attempt requires a Nock kernel instance with specific state, complicating work distribution.

## Protocol Architecture

### Pool Coordinator Node

```text
┌─────────────────────┐
│   Pool Coordinator  │
│  (Extended Node)    │
├─────────────────────┤
│ - Work Distribution │
│ - Share Validation  │
│ - Reward Tracking   │
│ - Miner Registry    │
└──────────┬──────────┘
           │
    ┌──────┴──────┐
    │   Miners    │
    │ (Pool API)  │
    └─────────────┘
```

## Core Protocol Components

### 1. Work Distribution Protocol

#### 1.1 Stratum-NC (Stratum for Nockchain)

A modified Stratum protocol adapted for STARK-based mining:

```json
// Work notification from pool to miner
{
  "id": null,
  "method": "mining.notify",
  "params": [
    "job_id",           // Unique job identifier
    "block_commitment", // Block commitment to prove
    "target",          // Difficulty target
    "nonce_start",     // Starting nonce for this miner
    "nonce_range",     // Number of nonces to try
    "clean_jobs"       // Whether to abandon previous work
  ]
}

// Submit work from miner to pool
{
  "id": 4,
  "method": "mining.submit",
  "params": [
    "worker_name",
    "job_id",
    "nonce",
    "proof_commitment" // Commitment to full proof
  ]
}
```

#### 1.2 Work Segmentation

Since STARK proofs can't be partially solved, we implement pseudo-shares:

```rust
pub struct WorkSegment {
    pub block_commitment: Noun,
    pub nonce_range: Range<u64>,
    pub sub_difficulty: u64,  // Easier target for shares
    pub segment_id: u64,
}

pub struct ShareProof {
    pub segment_id: u64,
    pub nonce_tried: u64,
    pub computation_proof: ComputationProof, // Proves work was done
}
```

### 2. Share System

#### 2.1 Computation Proof Shares

Instead of partial solutions, miners prove they performed computation:

```rust
pub enum ShareType {
    // Proof that miner computed STARK witness for nonce range
    ComputationShare {
        witness_merkle_root: [u8; 32],
        nonce_range: Range<u64>,
        computation_steps: u64,
    },
    
    // Proof of intermediate polynomial evaluation
    IntermediateShare {
        poly_commitment: [u8; 32],
        evaluation_point: Felt252,
        claimed_value: Felt252,
    },
    
    // Full valid block (jackpot)
    ValidBlock {
        nonce: u64,
        full_proof: StarkProof,
    },
}
```

#### 2.2 Share Validation

Pool validates shares using spot-checks:

```rust
impl PoolCoordinator {
    async fn validate_share(&self, share: ShareType) -> Result<bool, PoolError> {
        match share {
            ShareType::ComputationShare { witness_merkle_root, nonce_range, .. } => {
                // Randomly sample nonces and verify witness computation
                let sample_nonces = self.sample_nonces(&nonce_range, SAMPLE_SIZE);
                for nonce in sample_nonces {
                    let expected_witness = self.compute_witness_for_nonce(nonce);
                    if !self.verify_witness_in_merkle(expected_witness, witness_merkle_root) {
                        return Ok(false);
                    }
                }
                Ok(true)
            },
            // ... other share types
        }
    }
}
```

### 3. Reward Distribution

#### 3.1 PPLNS (Pay Per Last N Shares)

Adapt PPLNS for computation shares:

```rust
pub struct ShareWindow {
    shares: VecDeque<ShareRecord>,
    total_difficulty: u128,
    window_size: usize,
}

pub struct ShareRecord {
    miner: PublicKey,
    share_type: ShareType,
    difficulty_contribution: u64,
    timestamp: u64,
}

impl ShareWindow {
    fn calculate_rewards(&self, block_reward: u64) -> HashMap<PublicKey, u64> {
        let mut rewards = HashMap::new();
        
        for share in &self.shares {
            let miner_contribution = share.difficulty_contribution as f64 
                / self.total_difficulty as f64;
            let miner_reward = (block_reward as f64 * miner_contribution) as u64;
            
            *rewards.entry(share.miner).or_insert(0) += miner_reward;
        }
        
        rewards
    }
}
```

#### 3.2 Smart Contract Based Distribution

For decentralized pools, use on-chain distribution:

```hoon
::  Pool distribution contract
|_  [shares=(map address @ud) pool-fee=@ud]
++  distribute-rewards
  |=  [block-reward=@ud winning-miner=address]
  ^-  (list [address @ud])
  =/  total-shares  (roll ~(val by shares) add)
  =/  pool-cut  (div (mul block-reward pool-fee) 10.000)
  =/  miner-rewards  (sub block-reward pool-cut)
  %+  turn  ~(tap by shares)
  |=  [miner=address share=@ud]
  :-  miner
  (div (mul miner-rewards share) total-shares)
--
```

### 4. Pool Mining Integration

#### 4.1 Modified Mining Driver

Extend the existing mining driver for pool support:

```rust
// In mining.rs
pub struct PoolMiningConfig {
    pub pool_url: String,
    pub worker_name: String,
    pub worker_password: Option<String>,
}

pub fn create_pool_mining_driver(
    pool_config: PoolMiningConfig,
    init_complete_tx: Option<tokio::sync::oneshot::Sender<()>>,
) -> IODriverFn {
    Box::new(move |mut handle| {
        Box::pin(async move {
            // Connect to pool
            let pool_client = PoolClient::connect(&pool_config.pool_url).await?;
            
            // Authentication
            pool_client.authorize(&pool_config.worker_name, 
                                pool_config.worker_password.as_deref()).await?;
            
            // Main mining loop
            loop {
                tokio::select! {
                    // Receive work from pool
                    work = pool_client.get_work() => {
                        let work = work?;
                        spawn_pool_mining_attempt(work, handle.dup(), pool_client.clone());
                    },
                    
                    // Handle state updates
                    effect_res = handle.next_effect() => {
                        // Process mining kernel effects
                    }
                }
            }
        })
    })
}
```

#### 4.2 Pool-Aware Mining Kernel

Modify the mining kernel to support share generation:

```hoon
::  In miner.hoon
++  mine-with-shares
  |=  [commitment=@ target=@ share-target=@ nonce-start=@ nonce-range=@]
  =/  nonce  nonce-start
  |-
  ?:  (gte nonce (add nonce-start nonce-range))
    [%exhausted ~]
  =/  proof  (generate-proof commitment nonce)
  =/  hash   (hash-proof proof)
  ?:  (lth hash share-target)
    ?:  (lth hash target)
      [%block nonce proof]  :: Found valid block!
    [%share nonce (compute-witness commitment nonce)]  :: Found share
  $(nonce +(nonce))
```

### 5. Decentralized Pool Protocol

#### 5.1 P2P Work Coordination

Use libp2p for decentralized work distribution:

```rust
pub struct DecentralizedPool {
    peer_id: PeerId,
    dht: Kademlia<MemoryStore>,
    work_segments: Arc<RwLock<HashMap<u64, WorkSegment>>>,
}

impl DecentralizedPool {
    async fn claim_work_segment(&mut self) -> Result<WorkSegment, Error> {
        // Use DHT to coordinate work segments
        let my_segment = self.calculate_segment_for_peer(self.peer_id);
        
        // Announce claimed segment
        self.dht.put_record(
            Record::new(
                format!("segment:{}", my_segment.segment_id), 
                self.peer_id.to_bytes()
            )
        ).await?;
        
        Ok(my_segment)
    }
}
```

#### 5.2 Consensus on Shares

Implement Byzantine-fault tolerant share validation:

```rust
pub struct ShareConsensus {
    validators: Vec<PeerId>,
    threshold: usize,
}

impl ShareConsensus {
    async fn validate_share_distributed(&self, share: ShareProof) -> bool {
        let validations = self.validators.par_iter()
            .map(|validator| self.request_validation(validator, &share))
            .collect::<Vec<_>>();
        
        let positive_validations = validations.into_iter()
            .filter(|v| v.is_ok() && v.unwrap())
            .count();
            
        positive_validations >= self.threshold
    }
}
```

### 6. Security Considerations

#### 6.1 Share Withholding Attack Mitigation

Implement reputation system and random share audits:

```rust
pub struct MinerReputation {
    valid_shares: u64,
    invalid_shares: u64,
    blocks_found: u64,
    last_block_time: Option<u64>,
}

impl PoolCoordinator {
    fn should_audit_miner(&self, reputation: &MinerReputation) -> bool {
        // Increase audit probability for suspicious miners
        let expected_blocks = self.calculate_expected_blocks(reputation.valid_shares);
        let block_deficit = expected_blocks.saturating_sub(reputation.blocks_found);
        
        // Exponentially increase audit rate based on deficit
        let audit_probability = (block_deficit as f64 * 0.1).min(1.0);
        rand::random::<f64>() < audit_probability
    }
}
```

#### 6.2 Sybil Attack Prevention

Require proof-of-work for pool registration:

```rust
pub struct PoolRegistration {
    pub miner_pubkey: PublicKey,
    pub registration_pow: RegistrationProof,
    pub stake: Option<u64>,  // Optional stake requirement
}

pub struct RegistrationProof {
    nonce: u64,
    difficulty: u64,
}
```

### 7. Implementation Roadmap

#### Phase 1: Centralized Pool (2-4 weeks)

1. Implement Stratum-NC protocol
2. Modify mining driver for pool support
3. Create share validation system
4. Implement PPLNS reward distribution

#### Phase 2: Share System Optimization (4-6 weeks)

1. Optimize computation proof generation
2. Implement efficient share validation
3. Add reputation system
4. Create pool coordinator software

#### Phase 3: Decentralized Pool (6-8 weeks)

1. Implement P2P work coordination
2. Create consensus mechanism for shares
3. Deploy smart contracts for rewards
4. Add Byzantine fault tolerance

#### Phase 4: Advanced Features (8-12 weeks)

1. GPU mining pool support
2. Merged mining capabilities
3. Advanced anti-cheating mechanisms
4. Pool federation protocol

## Performance Metrics

### Target Performance

- **Share submission latency**: < 100ms
- **Work distribution latency**: < 50ms
- **Share validation time**: < 200ms
- **Pool efficiency**: > 98% (vs solo mining)

### Monitoring

```rust
pub struct PoolMetrics {
    pub total_hashrate: f64,
    pub active_miners: u64,
    pub shares_per_second: f64,
    pub average_share_difficulty: u64,
    pub orphan_rate: f64,
    pub payout_efficiency: f64,
}
```

## Economic Model

### Pool Fees

- **Centralized pools**: 1-3% of block rewards
- **Decentralized pools**: 0.5-1% (lower due to reduced overhead)
- **Vardiff fees**: Dynamic based on miner hashrate

### Minimum Payouts

- Batch payouts to reduce transaction costs
- Configurable minimum thresholds
- Lightning Network integration for micro-payouts

## Conclusion

This mining pool protocol design addresses the unique challenges of Nockchain's STARK-based PoW while providing familiar pool mining benefits. The hybrid approach supports both centralized efficiency and decentralized trustlessness, allowing the ecosystem to evolve based on miner preferences.

The key innovation is the computation proof share system, which enables fair reward distribution without partial solutions. Combined with advanced anti-cheating mechanisms and efficient work distribution, this protocol can significantly reduce mining variance while maintaining network security.
