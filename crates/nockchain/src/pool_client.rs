use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures::{SinkExt, StreamExt};
use tracing::{error, info, debug, warn};
use crate::mining::PoolMiningConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolJob {
    pub id: String,
    pub block_commitment: Vec<u8>,
    pub target: Vec<u8>,
    pub share_target: Vec<u8>,
    pub nonce_start: u64,
    pub nonce_range: u64,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StratumMessage {
    id: Option<u64>,
    method: String,
    params: serde_json::Value,
}

#[derive(Clone)]
pub struct PoolClient {
    pub config: PoolMiningConfig,
    job_receiver: Arc<RwLock<mpsc::Receiver<PoolJob>>>,
    share_sender: mpsc::Sender<ShareSubmission>,
    authorized: Arc<RwLock<bool>>,
}

impl PoolClient {
    pub async fn new(config: &PoolMiningConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (job_tx, job_rx) = mpsc::channel(100);
        let (share_tx, share_rx) = mpsc::channel(100);
        
        let client = Self {
            config: config.clone(),
            job_receiver: Arc::new(RwLock::new(job_rx)),
            share_sender: share_tx,
            authorized: Arc::new(RwLock::new(false)),
        };
        
        // Start connection handler
        let client_clone = client.clone();
        let job_tx_clone = job_tx.clone();
        tokio::spawn(async move {
            client_clone.connection_handler(job_tx_clone, share_rx).await;
        });
        
        Ok(client)
    }
    
    async fn connection_handler(
        &self,
        job_sender: mpsc::Sender<PoolJob>,
        mut share_receiver: mpsc::Receiver<ShareSubmission>,
    ) {
        loop {
            match self.connect_and_handle(&job_sender, &mut share_receiver).await {
                Ok(_) => {
                    warn!("Pool connection closed, reconnecting in 5 seconds...");
                }
                Err(e) => {
                    error!("Pool connection error: {}, reconnecting in 5 seconds...", e);
                }
            }
            
            *self.authorized.write().await = false;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    
    async fn connect_and_handle(
        &self,
        job_sender: &mpsc::Sender<PoolJob>,
        share_receiver: &mut mpsc::Receiver<ShareSubmission>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (ws_stream, _) = connect_async(&self.config.pool_url).await?;
        let (mut write, mut read) = ws_stream.split();
        
        info!("Connected to pool at {}", self.config.pool_url);
        
        // Send authorization
        let auth_msg = StratumMessage {
            id: Some(1),
            method: "mining.authorize".to_string(),
            params: serde_json::json!([
                self.config.worker_name,
                self.config.worker_password.as_deref().unwrap_or("")
            ]),
        };
        
        write.send(Message::Text(serde_json::to_string(&auth_msg)?)).await?;
        
        // Subscribe to jobs
        let subscribe_msg = StratumMessage {
            id: Some(2),
            method: "mining.subscribe".to_string(),
            params: serde_json::json!([]),
        };
        
        write.send(Message::Text(serde_json::to_string(&subscribe_msg)?)).await?;
        
        loop {
            tokio::select! {
                // Handle incoming messages from pool
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(stratum_msg) = serde_json::from_str::<StratumMessage>(&text) {
                                self.handle_stratum_message(stratum_msg, job_sender).await?;
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("Pool closed connection");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                
                // Handle outgoing shares
                share = share_receiver.recv() => {
                    if let Some(share) = share {
                        let submit_msg = StratumMessage {
                            id: Some(rand::random()),
                            method: "mining.submit".to_string(),
                            params: serde_json::to_value(&share)?,
                        };
                        
                        write.send(Message::Text(serde_json::to_string(&submit_msg)?)).await?;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    async fn handle_stratum_message(
        &self,
        msg: StratumMessage,
        job_sender: &mpsc::Sender<PoolJob>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match msg.method.as_str() {
            "mining.notify" => {
                if let Ok(job) = serde_json::from_value::<PoolJob>(msg.params) {
                    debug!("Received new job: {}", job.id);
                    job_sender.send(job).await?;
                }
            }
            "mining.set_difficulty" => {
                debug!("Difficulty update: {:?}", msg.params);
            }
            _ => {
                if msg.id.is_some() {
                    // Response to our request
                    if let Some(result) = msg.params.get("result") {
                        if result.as_bool() == Some(true) {
                            *self.authorized.write().await = true;
                            info!("Successfully authorized with pool");
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    pub async fn recv_job(&self) -> Result<PoolJob, Box<dyn std::error::Error + Send + Sync>> {
        let mut receiver = self.job_receiver.write().await;
        receiver.recv().await.ok_or_else(|| "Job channel closed".into())
    }
    
    pub async fn submit_share(&self, share: ShareSubmission) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.share_sender.send(share).await?;
        Ok(())
    }
    
    pub async fn is_authorized(&self) -> bool {
        *self.authorized.read().await
    }
}