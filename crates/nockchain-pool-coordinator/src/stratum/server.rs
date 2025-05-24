use std::sync::Arc;
use std::net::SocketAddr;
use std::collections::HashMap;
use axum::{
    extract::{ws::WebSocketUpgrade, State, ConnectInfo},
    response::Response,
    routing::get,
    Router,
    Json,
};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;
use tracing::{info, warn, debug, error};
use serde_json::json;

use crate::coordinator::PoolCoordinator;
use crate::database::{JobTemplate, PoolStats};
use crate::error::{PoolError, Result};
use super::protocol::{StratumMessage, StratumRequest, StratumResponse, StratumError};
use super::connection::{MinerConnection, handle_websocket, ConnectionHandler};

#[derive(Clone)]
pub struct StratumServer {
    coordinator: Arc<PoolCoordinator>,
    job_broadcaster: broadcast::Sender<JobTemplate>,
    active_connections: Arc<RwLock<HashMap<String, Arc<MinerConnection>>>>,
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

    pub fn router(self) -> Router {
        Router::new()
            .route("/", get(websocket_handler))
            .route("/stats", get(stats_handler))
            .route("/api/stats/:address", get(miner_stats_handler))
            .with_state(Arc::new(self))
    }
    
    pub async fn broadcast_new_job(&self, job: JobTemplate) {
        // Send to broadcaster
        let _ = self.job_broadcaster.send(job.clone());
        
        // Also send directly to all connected miners
        let connections = self.active_connections.read().await;
        let notification = StratumResponse::Notification {
            method: "mining.notify".to_string(),
            params: json!({
                "job_id": job.id,
                "block_commitment": hex::encode(&job.block_commitment),
                "target": hex::encode(&job.target),
                "share_target": hex::encode(&job.share_target),
                "clean_jobs": true,
            }),
        };
        
        let message = serde_json::to_string(&notification.to_message()).unwrap();
        
        for (miner_id, connection) in connections.iter() {
            if connection.authorized {
                if let Err(e) = connection.send_message(message.clone()).await {
                    warn!("Failed to send job to miner {}: {}", miner_id, e);
                }
            }
        }
    }
    
    async fn handle_stratum_request(
        &self,
        connection: Arc<MinerConnection>,
        request: StratumRequest,
    ) -> Result<StratumResponse> {
        match request {
            StratumRequest::Subscribe { id, user_agent } => {
                debug!("Miner {} subscribing with user agent: {:?}", connection.id, user_agent);
                
                // Generate subscription ID
                let subscription_id = format!("{:x}", rand::random::<u64>());
                
                Ok(StratumResponse::Result {
                    id,
                    result: json!([
                        ["mining.notify", subscription_id],
                        "00000000", // Extra nonce 1
                        4           // Extra nonce 2 size
                    ]),
                })
            }
            
            StratumRequest::Authorize { id, worker_name, password: _ } => {
                // Update connection with worker name
                let mut connections = self.active_connections.write().await;
                if let Some(conn) = connections.get_mut(&connection.id) {
                    let mut_conn = Arc::get_mut(conn).unwrap();
                    mut_conn.worker_name = Some(worker_name.clone());
                    mut_conn.authorized = true;
                }
                
                // Register miner with coordinator
                self.coordinator.register_miner(&connection.address.to_string(), &worker_name).await?;
                
                info!("Miner {} authorized as {}", connection.id, worker_name);
                
                // Send current job if available
                if let Some(job) = self.coordinator.get_current_job().await? {
                    let notification = StratumResponse::Notification {
                        method: "mining.notify".to_string(),
                        params: json!({
                            "job_id": job.id,
                            "block_commitment": hex::encode(&job.block_commitment),
                            "target": hex::encode(&job.target),
                            "share_target": hex::encode(&job.share_target),
                            "clean_jobs": true,
                        }),
                    };
                    
                    let message = serde_json::to_string(&notification.to_message()).unwrap();
                    connection.send_message(message).await?;
                }
                
                Ok(StratumResponse::Result {
                    id,
                    result: json!(true),
                })
            }
            
            StratumRequest::Submit { id, job_id, share_data, .. } => {
                let worker_name = connection.worker_name.as_ref()
                    .ok_or_else(|| PoolError::MinerNotFound(connection.id.clone()))?;
                
                // Create share submission
                let share_type = match share_data {
                    super::protocol::ShareSubmissionData::ComputationProof { witness_commitment, computation_steps } => {
                        crate::shares::ShareType::ComputationProof {
                            nonce: 0, // Should be extracted from actual submission
                            witness_commitment,
                            computation_steps,
                        }
                    }
                    super::protocol::ShareSubmissionData::ValidBlock { proof } => {
                        crate::shares::ShareType::ValidBlock {
                            nonce: 0, // Should be extracted from actual submission
                            proof,
                        }
                    }
                };
                
                let submission = crate::shares::ShareSubmission {
                    job_id,
                    miner_id: worker_name.clone(),
                    share_type,
                };
                
                // Submit to coordinator
                match self.coordinator.submit_share(submission).await {
                    Ok(validation) => {
                        if validation.is_block {
                            info!("BLOCK FOUND by {}!", worker_name);
                        }
                        
                        Ok(StratumResponse::Result {
                            id,
                            result: json!(true),
                        })
                    }
                    Err(e) => {
                        warn!("Share rejected from {}: {}", worker_name, e);
                        Ok(StratumResponse::Error {
                            id,
                            error: StratumError {
                                code: -32603,
                                message: e.to_string(),
                                data: None,
                            },
                        })
                    }
                }
            }
            
            StratumRequest::GetStatus { id } => {
                let stats = self.coordinator.get_pool_stats().await?;
                Ok(StratumResponse::Result {
                    id,
                    result: serde_json::to_value(stats)?,
                })
            }
        }
    }
}

#[async_trait::async_trait]
impl ConnectionHandler for StratumServer {
    async fn on_connect(&self, connection: Arc<MinerConnection>) {
        info!("New miner connection from {}", connection.address);
        self.active_connections.write().await.insert(connection.id.clone(), connection);
    }
    
    async fn on_message(&self, connection: Arc<MinerConnection>, message: String) -> Result<()> {
        debug!("Received message from {}: {}", connection.id, message);
        
        let stratum_msg: StratumMessage = serde_json::from_str(&message)?;
        let request = stratum_msg.parse_request()
            .map_err(|e| PoolError::StratumProtocol(format!("Invalid request: {:?}", e)))?;
        
        let response = self.handle_stratum_request(connection.clone(), request).await?;
        let response_msg = serde_json::to_string(&response.to_message())?;
        
        connection.send_message(response_msg).await?;
        Ok(())
    }
    
    async fn on_disconnect(&self, connection: Arc<MinerConnection>) {
        info!("Miner {} disconnected", connection.id);
        self.active_connections.write().await.remove(&connection.id);
        
        if let Some(worker_name) = &connection.worker_name {
            if let Err(e) = self.coordinator.unregister_miner(worker_name).await {
                error!("Failed to unregister miner {}: {}", worker_name, e);
            }
        }
    }
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<StratumServer>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    let miner_id = Uuid::new_v4().to_string();
    ws.on_upgrade(move |socket| handle_websocket(socket, addr, miner_id, server))
}

async fn stats_handler(
    State(server): State<Arc<StratumServer>>,
) -> Result<Json<PoolStats>, PoolError> {
    let stats = server.coordinator.get_pool_stats().await?;
    Ok(Json(stats))
}

async fn miner_stats_handler(
    State(server): State<Arc<StratumServer>>,
    axum::extract::Path(address): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, PoolError> {
    let stats = server.coordinator.get_miner_stats(&address).await?;
    Ok(Json(stats))
}