pub mod database;
pub mod error;
pub mod shares;
pub mod stratum;
pub mod coordinator;
pub mod payout;
pub mod metrics;

pub use coordinator::PoolCoordinator;
pub use error::{PoolError, Result};