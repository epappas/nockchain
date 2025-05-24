use std::sync::Arc;
use std::net::SocketAddr;
use clap::Parser;
use tokio::signal;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use nockchain_pool_coordinator::{
    PoolCoordinator,
    coordinator::PoolConfig,
    stratum::StratumServer,
    metrics::{register_metrics, metrics_handler},
};

#[derive(Parser, Debug)]
#[clap(name = "nockchain-pool-coordinator")]
#[clap(about = "Nockchain mining pool coordinator", long_about = None)]
struct Args {
    /// Redis URL for state storage
    #[clap(long, env = "POOL_REDIS_URL", default_value = "redis://localhost:6379")]
    redis_url: String,
    
    /// Pool name
    #[clap(long, env = "POOL_NAME", default_value = "Nockchain Mining Pool")]
    pool_name: String,
    
    /// Pool fee percentage
    #[clap(long, env = "POOL_FEE_PERCENT", default_value = "2.0")]
    pool_fee: f64,
    
    /// Minimum payout amount
    #[clap(long, env = "POOL_MIN_PAYOUT", default_value = "1000000")]
    min_payout: u64,
    
    /// Stratum server bind address
    #[clap(long, default_value = "0.0.0.0:3333")]
    stratum_bind: SocketAddr,
    
    /// HTTP API bind address
    #[clap(long, default_value = "0.0.0.0:8080")]
    http_bind: SocketAddr,
    
    /// Prometheus metrics port
    #[clap(long, default_value = "9091")]
    metrics_port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    let args = Args::parse();
    
    info!("Starting {} coordinator", args.pool_name);
    info!("Redis URL: {}", args.redis_url);
    info!("Pool fee: {}%", args.pool_fee);
    
    // Initialize metrics
    register_metrics();
    
    // Create pool configuration
    let config = PoolConfig {
        pool_name: args.pool_name.clone(),
        fee_percent: args.pool_fee,
        min_payout: args.min_payout,
        payout_interval: 3600,
        share_window_hours: 24,
        validation_threshold: 0.95,
    };
    
    // Initialize pool coordinator
    let coordinator = Arc::new(
        PoolCoordinator::new(&args.redis_url, config)
            .await
            .expect("Failed to create pool coordinator")
    );
    
    // Start maintenance tasks
    let coordinator_clone = coordinator.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            if let Err(e) = coordinator_clone.run_maintenance().await {
                error!("Maintenance error: {}", e);
            }
        }
    });
    
    // Create Stratum server
    let stratum_server = StratumServer::new(coordinator.clone()).await;
    
    // Start HTTP API server
    let api_router = axum::Router::new()
        .merge(stratum_server.router())
        .route("/metrics", axum::routing::get(|| async { metrics_handler() }));
    
    let http_server = axum::Server::bind(&args.http_bind)
        .serve(api_router.into_make_service_with_connect_info::<SocketAddr>());
    
    info!("Stratum server listening on {}", args.stratum_bind);
    info!("HTTP API listening on {}", args.http_bind);
    info!("Prometheus metrics on port {}", args.metrics_port);
    
    // Run until shutdown signal
    tokio::select! {
        res = http_server => {
            if let Err(e) = res {
                error!("HTTP server error: {}", e);
            }
        }
        _ = signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }
    
    info!("Pool coordinator shutting down");
    Ok(())
}