# Nockchain Mining Pool Implementation Guide

## Table of Contents

1. [System Architecture Overview](#system-architecture-overview)
2. [Phase 1: Core Infrastructure](#phase-1-core-infrastructure)
3. [Phase 2: Protocol Implementation](#phase-2-protocol-implementation)
4. [Phase 3: Share System](#phase-3-share-system)
5. [Phase 4: Integration & Testing](#phase-4-integration-testing)
6. [Phase 5: Production Deployment](#phase-5-production-deployment)

## System Architecture Overview

### Component Diagram

```text
┌────────────────────────────────────────────────────────────┐
│                    Pool Infrastructure                      │
├────────────────────┬───────────────────┬──────────────────┤
│   Pool Coordinator │  Share Validator  │  Payout Manager  │
│   (Modified Node)  │  (New Component)  │  (New Component) │
└──────────┬─────────┴─────────┬─────────┴──────────────────┘
           │                   │
    ┌──────┴─────┐      ┌──────┴─────┐
    │  Stratum   │      │  Database  │
    │  Server    │      │  (Redis)   │
    └──────┬─────┘      └────────────┘
           │
    ┌──────┴─────────────────┐
    │   Pool Miners          │
    │ (Modified nockchain)   │
    └────────────────────────┘
```

## Phase 1: Core Infrastructure

### Step 1.1: Fork and Setup Repository

```bash
# Fork nockchain repository
git clone https://github.com/yourusername/nockchain-pool
cd nockchain-pool

# Create pool-specific branches
git checkout -b pool/main
git checkout -b pool/coordinator
git checkout -b pool/miner-client

# Setup additional dependencies
cat >> Cargo.toml << EOF

[workspace.dependencies]
# Pool-specific dependencies
redis = "0.23"
tokio-tungstenite = "0.20"
serde_json = "1.0"
prometheus = "0.13"
EOF
```

### Step 1.2: Create Pool Coordinator Crate

```bash
# Create new crate for pool coordinator
cargo new --lib crates/nockchain-pool-coordinator
cd crates/nockchain-pool-coordinator
```

Create `Cargo.toml`:

```toml
[package]
name = "nockchain-pool-coordinator"
version = "0.1.0"
edition = "2021"

[dependencies]
nockchain = { path = "../nockchain" }
nockapp = { path = "../nockapp" }
nockvm = { path = "../nockvm" }
tokio = { version = "1.35", features = ["full"] }
redis = { version = "0.23", features = ["tokio-comp"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
axum = "0.7"
tower = "0.4"
tracing = "0.1"
prometheus = "0.13"
```

### Step 1.3: Database Schema Design

Create `src/database/schema.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerRecord {
    pub address: String,
    pub worker_name: String,
    pub shares_submitted: u64,
    pub shares_valid: u64,
    pub last_share_time: u64,
    pub total_difficulty: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareRecord {
    pub id: String,
    pub miner_address: String,
    pub job_id: String,
    pub nonce: u64,
    pub difficulty: u64,
    pub timestamp: u64,
    pub is_valid: bool,
    pub is_block: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobTemplate {
    pub id: String,
    pub block_commitment: Vec<u8>,
    pub target: Vec<u8>,
    pub timestamp: u64,
    pub nonce_ranges: HashMap<String, (u64, u64)>, // miner -> (start, end)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutQueue {
    pub pending_payouts: Vec<PendingPayout>,
    pub last_payout_time: u64,
    pub total_paid: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPayout {
    pub miner_address: String,
    pub amount: u64,
    pub shares_window: (u64, u64), // (start_time, end_time)
}
```

## Phase 2: Protocol Implementation

### Step 2.1: Stratum-NC Server Implementation

Create `src/stratum/server.rs`:

```rust
use axum::{
    extract::{ws::WebSocket, WebSocketUpgrade, State},
    response::Response,
    routing::get,
    Router,
};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub struct StratumServer {
    coordinator: Arc<PoolCoordinator>,
    job_broadcaster: broadcast::Sender<JobNotification>,
    active_connections: Arc<RwLock<HashMap<String, MinerConnection>>>,
}

impl StratumServer {
    pub async fn new(coordinator: Arc<PoolCoordinator>) -> Self {
        let (job_broadcaster, _) = broadcast::channel(1024);
        
        Self {
            coordinator,
            job_broadcaster,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(websocket_handler))
            .route("/stats", get(stats_handler))
            .with_state(Arc::new(self.clone()))
    }

    async fn handle_miner_connection(&self, ws: WebSocket, addr: SocketAddr) {
        let (sender, mut receiver) = ws.split();
        let miner_id = Uuid::new_v4().to_string();
        
        // Setup miner connection
        let connection = MinerConnection {
            id: miner_id.clone(),
            address: addr,
            sender: Arc::new(Mutex::new(sender)),
            authorized: false,
            worker_name: None,
        };
        
        // Handle incoming messages
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    self.handle_stratum_message(&miner_id, &text).await;
                }
                Err(e) => {
                    tracing::error!("WebSocket error: {}", e);
                    break;
                }
            }
        }
        
        // Cleanup on disconnect
        self.active_connections.write().await.remove(&miner_id);
    }

    async fn handle_stratum_message(&self, miner_id: &str, message: &str) {
        let msg: StratumMessage = match serde_json::from_str(message) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("Invalid message format: {}", e);
                return;
            }
        };

        match msg.method.as_str() {
            "mining.authorize" => self.handle_authorize(miner_id, msg).await,
            "mining.submit" => self.handle_submit(miner_id, msg).await,
            "mining.subscribe" => self.handle_subscribe(miner_id, msg).await,
            _ => tracing::warn!("Unknown method: {}", msg.method),
        }
    }
}
```

### Step 2.2: Modify Mining Driver for Pool Support

Edit `crates/nockchain/src/mining.rs`:

```rust
// Add pool support to existing mining driver

pub enum MiningMode {
    Solo(MiningKeyConfig),
    Pool(PoolMiningConfig),
}

pub struct PoolMiningConfig {
    pub pool_url: String,
    pub worker_name: String,
    pub worker_password: Option<String>,
    pub share_difficulty_multiplier: f64,
}

impl PoolMiningConfig {
    pub fn to_driver(&self) -> IODriverFn {
        let config = self.clone();
        Box::new(move |mut handle| {
            Box::pin(async move {
                // Initialize pool client
                let pool_client = PoolClient::new(&config).await?;
                
                // Main pool mining loop
                loop {
                    tokio::select! {
                        // Receive work from pool
                        job = pool_client.recv_job() => {
                            match job {
                                Ok(job) => {
                                    spawn_pool_mining_task(job, handle.dup()).await;
                                }
                                Err(e) => {
                                    tracing::error!("Failed to receive job: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        
                        // Handle mining results
                        effect = handle.next_effect() => {
                            if let Ok(effect) = effect {
                                handle_pool_mining_effect(effect, &pool_client).await;
                            }
                        }
                    }
                }
            })
        })
    }
}

async fn spawn_pool_mining_task(job: PoolJob, handle: NockAppHandle) {
    let snapshot_dir = tempdir().expect("Failed to create temporary directory");
    let hot_state = zkvm_jetpack::hot::produce_prover_hot_state();
    let jam_paths = JamPaths::new(snapshot_dir.path());
    
    // Load mining kernel with pool-specific configuration
    let mut kernel = Kernel::load_with_hot_state_huge(
        snapshot_dir.path().to_path_buf(),
        jam_paths,
        KERNEL,
        &hot_state,
        false,
    )
    .await
    .expect("Could not load mining kernel");
    
    // Configure for share generation
    let share_config = ShareMiningConfig {
        block_commitment: job.block_commitment,
        target: job.target,
        share_target: job.share_target,
        nonce_start: job.nonce_start,
        nonce_range: job.nonce_range,
    };
    
    // Start mining with share support
    let effects = kernel
        .poke(MiningWire::PoolCandidate.to_wire(), share_config.to_noun())
        .await
        .expect("Could not poke mining kernel");
    
    // Process effects (shares and blocks)
    for effect in effects.to_vec() {
        handle.forward_effect(effect).await;
    }
}
```

## Phase 3: Share System

### Step 3.1: Computation Proof Implementation

Create `src/shares/computation_proof.rs`:

```rust
use nockvm::noun::{Atom, Noun};
use sha2::{Sha256, Digest};

#[derive(Debug, Clone)]
pub struct ComputationProof {
    pub witness_commitment: [u8; 32],
    pub nonce_range: Range<u64>,
    pub computation_steps: u64,
    pub intermediate_hashes: Vec<[u8; 32]>,
}

impl ComputationProof {
    pub fn generate_for_range(
        block_commitment: &Noun,
        nonce_range: Range<u64>,
        sample_rate: usize,
    ) -> Self {
        let mut hasher = Sha256::new();
        let mut intermediate_hashes = Vec::new();
        let mut computation_steps = 0;
        
        // Sample nonces from range
        let step = (nonce_range.end - nonce_range.start) / sample_rate as u64;
        
        for i in 0..sample_rate {
            let nonce = nonce_range.start + (i as u64 * step);
            
            // Simulate STARK witness computation
            let witness = compute_partial_witness(block_commitment, nonce);
            hasher.update(&witness.to_bytes());
            
            // Store intermediate hash
            let intermediate = hasher.clone().finalize();
            intermediate_hashes.push(intermediate.into());
            
            computation_steps += estimate_computation_steps(&witness);
        }
        
        ComputationProof {
            witness_commitment: hasher.finalize().into(),
            nonce_range,
            computation_steps,
            intermediate_hashes,
        }
    }
    
    pub fn verify(&self, block_commitment: &Noun, spot_check_count: usize) -> bool {
        // Randomly select nonces to verify
        let mut rng = rand::thread_rng();
        let nonces: Vec<u64> = (0..spot_check_count)
            .map(|_| rng.gen_range(self.nonce_range.clone()))
            .collect();
        
        // Verify each selected nonce
        for nonce in nonces {
            let witness = compute_partial_witness(block_commitment, nonce);
            let hash = Sha256::digest(&witness.to_bytes());
            
            // Check if hash matches any intermediate
            if !self.intermediate_hashes.iter().any(|h| h == &hash.as_slice()) {
                return false;
            }
        }
        
        true
    }
}

fn compute_partial_witness(commitment: &Noun, nonce: u64) -> Noun {
    // Simplified witness computation
    // In production, this would compute actual STARK witness
    let mut slab = NounSlab::new();
    T(&mut slab, &[commitment.clone(), D(nonce)])
}
```

### Step 3.2: Share Validation System

Create `src/shares/validator.rs`:

```rust
pub struct ShareValidator {
    redis: Arc<redis::Client>,
    kernel_cache: Arc<RwLock<HashMap<String, Kernel>>>,
    validation_threshold: f64,
}

impl ShareValidator {
    pub async fn validate_share(
        &self,
        share: ShareSubmission,
    ) -> Result<ShareValidation, ValidationError> {
        // Check if share is duplicate
        if self.is_duplicate(&share).await? {
            return Err(ValidationError::DuplicateShare);
        }
        
        match share.share_type {
            ShareType::ComputationProof(proof) => {
                self.validate_computation_proof(&share.job_id, proof).await
            }
            ShareType::ValidBlock(block) => {
                self.validate_block(&share.job_id, block).await
            }
        }
    }
    
    async fn validate_computation_proof(
        &self,
        job_id: &str,
        proof: ComputationProof,
    ) -> Result<ShareValidation, ValidationError> {
        // Get job template
        let job = self.get_job_template(job_id).await?;
        
        // Verify proof
        if !proof.verify(&job.block_commitment, SPOT_CHECK_COUNT) {
            return Err(ValidationError::InvalidProof);
        }
        
        // Calculate share difficulty
        let difficulty = self.calculate_share_difficulty(&proof);
        
        Ok(ShareValidation {
            is_valid: true,
            difficulty,
            is_block: false,
            reward_units: difficulty * proof.computation_steps,
        })
    }
    
    async fn validate_block(
        &self,
        job_id: &str,
        block: ValidBlock,
    ) -> Result<ShareValidation, ValidationError> {
        // Get job template
        let job = self.get_job_template(job_id).await?;
        
        // Verify STARK proof
        let kernel = self.get_verification_kernel().await?;
        let is_valid = kernel.verify_stark_proof(&block.proof, &job.block_commitment);
        
        if !is_valid {
            return Err(ValidationError::InvalidBlock);
        }
        
        // Check if meets target
        let proof_hash = hash_stark_proof(&block.proof);
        if proof_hash > job.target {
            return Err(ValidationError::InsufficientDifficulty);
        }
        
        Ok(ShareValidation {
            is_valid: true,
            difficulty: calculate_block_difficulty(&job.target),
            is_block: true,
            reward_units: BLOCK_REWARD_UNITS,
        })
    }
}
```

## Phase 4: Integration & Testing

### Step 4.1: Modify Hoon Mining Kernel

Create patch for `hoon/apps/dumbnet/miner.hoon`:

```hoon
::  Add pool mining support to miner kernel
::
|%
++  mine-with-shares
  |=  $:  commitment=@
          target=@
          share-target=@
          nonce-start=@
          nonce-range=@
      ==
  ^-  [%mine-result (unit mine-result)]
  =/  nonce  nonce-start
  =/  end-nonce  (add nonce-start nonce-range)
  =/  share-proofs  *(list share-proof)
  |-
  ?:  (gte nonce end-nonce)
    [%exhausted share-proofs]
  ::  Generate STARK proof for current nonce
  =/  proof  (generate-stark-proof commitment nonce)
  =/  proof-hash  (hash-proof proof)
  ::  Check if valid block
  ?:  (lth proof-hash target)
    [%block nonce proof share-proofs]
  ::  Check if valid share
  ?:  (lth proof-hash share-target)
    =/  witness  (compute-witness commitment nonce)
    =/  share  [%computation nonce witness (compute-steps witness)]
    $(nonce +(nonce), share-proofs [share share-proofs])
  ::  Continue searching
  $(nonce +(nonce))
::
++  compute-witness
  |=  [commitment=@ nonce=@]
  ^-  witness
  ::  Compute partial STARK witness
  ::  This is a simplified version
  =/  input  (cat 3 commitment nonce)
  =/  trace  (compute-trace input)
  [%witness (hash-trace trace) trace]
::
++  compute-steps
  |=  witness=^witness
  ^-  @ud
  ::  Estimate computation complexity
  (lent trace.witness)
--
```

### Step 4.2: Integration Tests

Create `tests/pool_integration.rs`:

```rust
#[cfg(test)]
mod pool_integration_tests {
    use super::*;
    use tokio::test;

    #[test]
    async fn test_pool_mining_flow() {
        // Start pool coordinator
        let coordinator = PoolCoordinator::new(test_config()).await;
        let pool_server = coordinator.start_server().await;
        
        // Start test miners
        let mut miners = vec![];
        for i in 0..5 {
            let miner = TestMiner::new(&format!("miner_{}", i));
            miner.connect(&pool_server.url()).await;
            miners.push(miner);
        }
        
        // Submit test block
        let block = create_test_block();
        coordinator.new_block(block).await;
        
        // Wait for shares
        tokio::time::sleep(Duration::from_secs(10)).await;
        
        // Verify shares received
        let stats = coordinator.get_stats().await;
        assert!(stats.total_shares > 0);
        assert_eq!(stats.connected_miners, 5);
        
        // Simulate block found
        let winning_miner = &miners[0];
        winning_miner.submit_block(test_block_proof()).await;
        
        // Verify rewards calculated
        let rewards = coordinator.calculate_rewards().await;
        assert_eq!(rewards.len(), 5);
        assert!(rewards.values().sum::<u64>() <= BLOCK_REWARD);
    }

    #[test]
    async fn test_share_validation() {
        let validator = ShareValidator::new(test_config());
        
        // Test valid computation proof
        let valid_proof = ComputationProof::generate_for_range(
            &test_commitment(),
            0..1000,
            10,
        );
        
        let result = validator.validate_share(ShareSubmission {
            job_id: "test_job".to_string(),
            share_type: ShareType::ComputationProof(valid_proof),
            miner_id: "test_miner".to_string(),
        }).await;
        
        assert!(result.is_ok());
        
        // Test invalid proof
        let mut invalid_proof = valid_proof.clone();
        invalid_proof.witness_commitment = [0; 32];
        
        let result = validator.validate_share(ShareSubmission {
            job_id: "test_job".to_string(),
            share_type: ShareType::ComputationProof(invalid_proof),
            miner_id: "test_miner".to_string(),
        }).await;
        
        assert!(matches!(result, Err(ValidationError::InvalidProof)));
    }
}
```

## Phase 5: Production Deployment

### Step 5.1: Docker Configuration

Create `docker/pool-coordinator/Dockerfile`:

```dockerfile
FROM rust:1.75 as builder

WORKDIR /app
COPY . .

# Build pool coordinator
RUN cargo build --release -p nockchain-pool-coordinator

# Runtime image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/nockchain-pool-coordinator /usr/local/bin/

# Configuration
ENV RUST_LOG=info
ENV POOL_REDIS_URL=redis://redis:6379
ENV POOL_FEE_PERCENT=2.0
ENV POOL_MIN_PAYOUT=1000000

EXPOSE 3333 8080

CMD ["nockchain-pool-coordinator"]
```

Create `docker-compose.yml`:

```yaml
version: '3.8'

services:
  pool-coordinator:
    build:
      context: .
      dockerfile: docker/pool-coordinator/Dockerfile
    ports:
      - "3333:3333"  # Stratum port
      - "8080:8080"  # HTTP API
    environment:
      - POOL_REDIS_URL=redis://redis:6379
      - POOL_NODE_URL=http://nockchain-node:9652
    depends_on:
      - redis
      - nockchain-node
    volumes:
      - ./pool-data:/data

  nockchain-node:
    build:
      context: .
      dockerfile: docker/node/Dockerfile
    command: ["--pool-mode", "--no-mine"]
    volumes:
      - ./node-data:/data

  redis:
    image: redis:7-alpine
    command: redis-server --appendonly yes
    volumes:
      - ./redis-data:/data

  prometheus:
    image: prom/prometheus
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus-data:/prometheus
    ports:
      - "9090:9090"

  grafana:
    image: grafana/grafana
    ports:
      - "3000:3000"
    volumes:
      - grafana-data:/var/lib/grafana
      - ./grafana/dashboards:/etc/grafana/provisioning/dashboards

volumes:
  prometheus-data:
  grafana-data:
```

### Step 5.2: Configuration Management

Create `config/pool.toml`:

```toml
[pool]
name = "Nockchain Mining Pool"
fee_percent = 2.0
min_payout = 1000000  # Minimum payout threshold
payout_interval = 3600  # Payout every hour

[stratum]
host = "0.0.0.0"
port = 3333
max_connections = 10000
share_difficulty_multiplier = 0.001  # Share diff = block diff * multiplier

[redis]
url = "redis://localhost:6379"
pool_size = 50

[pplns]
window_size = 100000  # Last N shares
window_time = 86400   # 24 hours

[security]
enable_registration_pow = true
registration_difficulty = 1000000
max_workers_per_account = 100
share_rate_limit = 100  # shares per second per worker

[monitoring]
prometheus_port = 9091
enable_metrics = true
```

### Step 5.3: Deployment Script

Create `scripts/deploy_pool.sh`:

```bash
#!/bin/bash
set -e

# Configuration
POOL_NAME="nockchain-pool"
DOMAIN="pool.nockchain.com"
EMAIL="admin@nockchain.com"

echo "Deploying Nockchain Mining Pool..."

# Build containers
docker-compose build

# Setup SSL with Let's Encrypt
docker run --rm \
  -v ./certbot:/etc/letsencrypt \
  -v ./certbot-www:/var/www/certbot \
  certbot/certbot certonly \
  --webroot \
  --webroot-path=/var/www/certbot \
  --email $EMAIL \
  --agree-tos \
  --no-eff-email \
  -d $DOMAIN

# Start services
docker-compose up -d

# Wait for services to be ready
echo "Waiting for services to start..."
sleep 30

# Run database migrations
docker-compose exec pool-coordinator /usr/local/bin/migrate

# Setup monitoring
docker-compose exec prometheus promtool check config /etc/prometheus/prometheus.yml

echo "Pool deployment complete!"
echo "Stratum endpoint: stratum+tcp://$DOMAIN:3333"
echo "Dashboard: http://$DOMAIN:8080"
echo "Monitoring: http://$DOMAIN:3000"
```

### Step 5.4: Miner Configuration

Create `docs/miner-setup.md`:

```markdown
# Nockchain Pool Mining Setup

## Quick Start

1. Download the pool-enabled miner:
```bash
wget https://releases.nockchain.com/pool-miner-v1.0.0
chmod +x pool-miner-v1.0.0
```

2. Configure your miner:

```bash
./pool-miner-v1.0.0 \
  --pool-url stratum+tcp://pool.nockchain.com:3333 \
  --worker-name your_worker_name \
  --wallet-address your_nockchain_address
```

3. Monitor your statistics:

- Web dashboard: <https://pool.nockchain.com>
- API endpoint: <https://pool.nockchain.com/api/stats/YOUR_ADDRESS>

## Advanced Configuration

For multiple GPUs:

```bash
./pool-miner-v1.0.0 \
  --pool-url stratum+tcp://pool.nockchain.com:3333 \
  --worker-name rig1 \
  --wallet-address your_address \
  --gpu-devices 0,1,2,3 \
  --kernel-threads 4
```

## Troubleshooting

- Check connectivity: `telnet pool.nockchain.com 3333`
- View logs: `./pool-miner-v1.0.0 --log-level debug`
- Report issues: <https://github.com/nockchain/pool/issues>

```

## Summary

This implementation guide provides a complete roadmap for building a Nockchain mining pool:

1. **Infrastructure**: Modified node with pool coordinator, Redis for state management
2. **Protocol**: Stratum-NC for work distribution, computation proofs for shares
3. **Integration**: Modified mining driver and kernel for pool support
4. **Security**: Anti-cheating measures, registration PoW, rate limiting
5. **Deployment**: Dockerized setup with monitoring and auto-scaling

The key technical innovations are:
- Computation proof shares that work with STARK's all-or-nothing nature
- Efficient work distribution using nonce ranges
- Spot-check validation to prevent cheating
- PPLNS reward system adapted for computation shares

This design maintains decentralization while providing stable mining income for participants.
