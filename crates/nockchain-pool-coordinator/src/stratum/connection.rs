use std::sync::Arc;
use std::net::SocketAddr;
use tokio::sync::{mpsc, Mutex};
use axum::extract::ws::{WebSocket, Message};
use futures::{SinkExt, StreamExt};
use tracing::{debug, error, info};
use crate::error::Result;

pub struct MinerConnection {
    pub id: String,
    pub address: SocketAddr,
    pub worker_name: Option<String>,
    pub authorized: bool,
    sender: mpsc::Sender<Message>,
}

impl MinerConnection {
    pub fn new(id: String, address: SocketAddr, sender: mpsc::Sender<Message>) -> Self {
        Self {
            id,
            address,
            worker_name: None,
            authorized: false,
            sender,
        }
    }
    
    pub async fn send_message(&self, message: String) -> Result<()> {
        self.sender.send(Message::Text(message)).await
            .map_err(|_| crate::error::PoolError::WebSocket("Failed to send message".to_string()))?;
        Ok(())
    }
    
    pub async fn close(&self) -> Result<()> {
        self.sender.send(Message::Close(None)).await
            .map_err(|_| crate::error::PoolError::WebSocket("Failed to send close".to_string()))?;
        Ok(())
    }
}

pub async fn handle_websocket(
    ws: WebSocket,
    addr: SocketAddr,
    miner_id: String,
    handler: Arc<dyn ConnectionHandler + Send + Sync>,
) {
    let (ws_sender, mut ws_receiver) = ws.split();
    let (tx, mut rx) = mpsc::channel(100);
    
    let connection = Arc::new(MinerConnection::new(miner_id.clone(), addr, tx));
    
    // Spawn task to forward messages from channel to websocket
    let ws_sender = Arc::new(Mutex::new(ws_sender));
    let ws_sender_clone = ws_sender.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = ws_sender_clone.lock().await.send(msg).await {
                error!("WebSocket send error: {}", e);
                break;
            }
        }
    });
    
    // Handle connection
    handler.on_connect(connection.clone()).await;
    
    // Process incoming messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Err(e) = handler.on_message(connection.clone(), text).await {
                    error!("Error handling message: {}", e);
                }
            }
            Ok(Message::Close(_)) => {
                info!("Client {} closed connection", miner_id);
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
    
    // Handle disconnection
    handler.on_disconnect(connection.clone()).await;
}

#[async_trait::async_trait]
pub trait ConnectionHandler {
    async fn on_connect(&self, connection: Arc<MinerConnection>);
    async fn on_message(&self, connection: Arc<MinerConnection>, message: String) -> Result<()>;
    async fn on_disconnect(&self, connection: Arc<MinerConnection>);
}