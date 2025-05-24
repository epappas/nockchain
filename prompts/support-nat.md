# NAT Support Implementation Plan for Nockchain

## Overview

This document outlines a comprehensive plan to add Network Address Translation (NAT) traversal support to Nockchain's libp2p networking layer. The implementation will enable nodes behind NATs, firewalls, and restrictive network environments to participate fully in the Nockchain network.

## Background

Currently, Nockchain nodes behind NATs cannot accept incoming connections, limiting their ability to:
- Act as full participants in the P2P network
- Receive direct connections from other peers
- Contribute to network resilience and decentralization

This implementation adds multiple NAT traversal strategies to ensure robust connectivity.

## Goals

### Primary Goals
1. **Universal Connectivity**: Enable nodes behind any type of NAT to participate in the network
2. **Automatic Configuration**: Minimize manual network configuration requirements
3. **Fallback Strategies**: Provide multiple NAT traversal methods for reliability
4. **Performance Optimization**: Prefer direct connections when possible
5. **Security Maintenance**: Ensure NAT traversal doesn't compromise network security

### Secondary Goals
1. **Relay Infrastructure**: Support for public nodes to act as relays
2. **Network Health**: Improve overall network connectivity and resilience
3. **User Experience**: Transparent operation requiring minimal user intervention
4. **Diagnostics**: Provide clear feedback about connectivity status

## Technical Approach

### NAT Traversal Strategies

#### 1. Circuit Relay v2
- **Purpose**: Allow nodes to communicate through public relay nodes
- **Use Case**: Primary fallback for nodes that cannot establish direct connections
- **Implementation**: libp2p's relay protocol with reservation system

#### 2. Direct Connection Upgrade through Relay (DCUtR)
- **Purpose**: Establish direct connections between NAT-ed peers via relay coordination
- **Use Case**: Upgrade relayed connections to direct connections for better performance
- **Implementation**: Hole punching coordinated through relay nodes

#### 3. AutoNAT
- **Purpose**: Automatic detection of NAT status and external addresses
- **Use Case**: Determine if a node is publicly reachable and discover external addresses
- **Implementation**: Probe-based NAT detection using other network participants

#### 4. UPnP Port Mapping
- **Purpose**: Automatically configure port forwarding on compatible routers
- **Use Case**: Make nodes publicly reachable without manual router configuration
- **Implementation**: Universal Plug and Play protocol for automatic port mapping

## Implementation Plan

### Phase 1: Dependencies and Core Infrastructure

#### 1.1 Update Dependencies

**File**: `crates/nockchain-libp2p-io/Cargo.toml`

```toml
[dependencies]
libp2p = { workspace = true, features = [
    # Existing features
    "ping",
    "kad", 
    "identify",
    "quic",
    "tls",
    "dns",
    "tokio",
    "macros",
    "request-response", 
    "memory-connection-limits",
    "cbor",
    "peer-store",
    
    # New NAT traversal features
    "relay",           # Circuit relay v2
    "dcutr",          # Direct Connection Upgrade through Relay
    "autonat",        # Automatic NAT detection
    "upnp",           # UPnP port mapping
] }
```

#### 1.2 Create NAT Configuration Module

**File**: `crates/nockchain-libp2p-io/src/nat_config.rs`

```rust
use libp2p::{Multiaddr, PeerId};
use std::time::Duration;

/// Configuration for NAT traversal features
#[derive(Debug, Clone)]
pub struct NatConfig {
    /// Enable relay client functionality
    pub enable_relay_client: bool,
    /// Enable relay server functionality (for public nodes)
    pub enable_relay_server: bool,
    /// Enable direct connection upgrade through relay
    pub enable_dcutr: bool,
    /// Enable automatic NAT detection
    pub enable_autonat: bool,
    /// Enable UPnP port mapping
    pub enable_upnp: bool,
    
    /// Known relay nodes to connect to
    pub relay_nodes: Vec<(PeerId, Multiaddr)>,
    /// Maximum number of relay connections
    pub max_relay_connections: usize,
    /// Reservation request timeout
    pub reservation_timeout: Duration,
    /// Circuit request timeout  
    pub circuit_request_timeout: Duration,
    
    /// AutoNAT probe interval
    pub autonat_probe_interval: Duration,
    /// Number of AutoNAT servers to use
    pub autonat_server_count: usize,
    
    /// UPnP lease duration
    pub upnp_lease_duration: Duration,
    /// External port to request via UPnP (None = auto-assign)
    pub upnp_external_port: Option<u16>,
    
    /// Enable relay server with limited resources
    pub relay_server_limits: RelayServerLimits,
}

/// Limits for relay server functionality
#[derive(Debug, Clone)]
pub struct RelayServerLimits {
    /// Maximum number of reservations
    pub max_reservations: usize,
    /// Maximum number of active circuits
    pub max_circuits: usize,
    /// Maximum circuits per peer
    pub max_circuits_per_peer: usize,
    /// Data transfer limit per circuit (bytes)
    pub max_circuit_bytes: u64,
    /// Circuit duration limit
    pub max_circuit_duration: Duration,
}

impl Default for NatConfig {
    fn default() -> Self {
        Self {
            enable_relay_client: true,
            enable_relay_server: false, // Only enable on public nodes
            enable_dcutr: true,
            enable_autonat: true,
            enable_upnp: true,
            
            relay_nodes: Vec::new(),
            max_relay_connections: 3,
            reservation_timeout: Duration::from_secs(30),
            circuit_request_timeout: Duration::from_secs(10),
            
            autonat_probe_interval: Duration::from_secs(60),
            autonat_server_count: 3,
            
            upnp_lease_duration: Duration::from_secs(3600), // 1 hour
            upnp_external_port: None,
            
            relay_server_limits: RelayServerLimits::default(),
        }
    }
}

impl Default for RelayServerLimits {
    fn default() -> Self {
        Self {
            max_reservations: 128,
            max_circuits: 16,
            max_circuits_per_peer: 4,
            max_circuit_bytes: 1024 * 1024 * 100, // 100MB per circuit
            max_circuit_duration: Duration::from_secs(3600), // 1 hour
        }
    }
}

/// Parse relay node string in format "peer_id@multiaddr"
pub fn parse_relay_node(node_str: &str) -> Result<(PeerId, Multiaddr), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = node_str.split('@').collect();
    if parts.len() != 2 {
        return Err("Invalid relay node format. Expected: peer_id@multiaddr".into());
    }
    
    let peer_id = parts[0].parse::<PeerId>()?;
    let multiaddr = parts[1].parse::<Multiaddr>()?;
    
    Ok((peer_id, multiaddr))
}

/// Get default relay nodes for the network
pub fn get_default_relay_nodes() -> Vec<(PeerId, Multiaddr)> {
    // TODO: Replace with actual Nockchain relay infrastructure
    const DEFAULT_RELAY_NODES: &[&str] = &[
        // Example format - replace with real relay nodes
        // "12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X@/ip4/relay1.nockchain.io/tcp/4001",
        // "12D3KooWQYV9dGMFoRzNSzJmHMUe9VK7i7D6f3rJ8Rp9sF2Gg7H@/ip4/relay2.nockchain.io/tcp/4001",
    ];
    
    DEFAULT_RELAY_NODES
        .iter()
        .filter_map(|node_str| parse_relay_node(node_str).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_relay_node() {
        let relay_str = "12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X@/ip4/127.0.0.1/tcp/4001";
        let result = parse_relay_node(relay_str);
        assert!(result.is_ok());
        
        let (peer_id, multiaddr) = result.unwrap();
        assert_eq!(peer_id.to_string(), "12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X");
        assert_eq!(multiaddr.to_string(), "/ip4/127.0.0.1/tcp/4001");
    }
    
    #[test]
    fn test_parse_relay_node_invalid() {
        let invalid_strs = vec![
            "invalid_format",
            "@/ip4/127.0.0.1/tcp/4001",
            "12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X@",
            "12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X@invalid_multiaddr",
        ];
        
        for invalid_str in invalid_strs {
            assert!(parse_relay_node(invalid_str).is_err());
        }
    }
}
```

### Phase 2: Network Behaviour Enhancement

#### 2.1 Update NetworkBehaviour

**File**: `crates/nockchain-libp2p-io/src/p2p.rs`

```rust
use libp2p::{
    relay, dcutr, autonat, upnp,
    swarm::behaviour::toggle::Toggle,
};
use crate::nat_config::NatConfig;

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "NockchainEvent")]
pub struct NockchainBehaviour {
    // Existing behaviors
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    pub allow_block_list: allow_block_list::Behaviour<allow_block_list::BlockedPeers>,
    pub allow_peers: Toggle<allow_block_list::Behaviour<allow_block_list::AllowedPeers>>,
    connection_limits: connection_limits::Behaviour,
    memory_connection_limits: Toggle<memory_connection_limits::Behaviour>,
    pub peer_store: libp2p::peer_store::Behaviour<libp2p::peer_store::memory_store::MemoryStore>,
    pub request_response: cbor::Behaviour<NockchainRequest, NockchainResponse>,

    // NAT traversal behaviors
    relay_client: Toggle<relay::client::Behaviour>,
    relay_server: Toggle<relay::Behaviour>,
    dcutr: Toggle<dcutr::Behaviour>,
    autonat: Toggle<autonat::Behaviour>,
    upnp: Toggle<upnp::tokio::Behaviour>,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum NockchainEvent {
    // Existing events
    Identify(identify::Event),
    Ping(ping::Event),
    Kad(kad::Event),
    RequestResponse(request_response::Event<NockchainRequest, NockchainResponse>),
    PeerStore(libp2p::peer_store::memory_store::Event),

    // NAT traversal events
    RelayClient(relay::client::Event),
    RelayServer(relay::Event),
    Dcutr(dcutr::Event),
    Autonat(autonat::Event),
    Upnp(upnp::Event),
}

// Implement From traits for new events
impl From<relay::client::Event> for NockchainEvent {
    fn from(event: relay::client::Event) -> Self {
        Self::RelayClient(event)
    }
}

impl From<relay::Event> for NockchainEvent {
    fn from(event: relay::Event) -> Self {
        Self::RelayServer(event)
    }
}

impl From<dcutr::Event> for NockchainEvent {
    fn from(event: dcutr::Event) -> Self {
        Self::Dcutr(event)
    }
}

impl From<autonat::Event> for NockchainEvent {
    fn from(event: autonat::Event) -> Self {
        Self::Autonat(event)
    }
}

impl From<upnp::Event> for NockchainEvent {
    fn from(event: upnp::Event) -> Self {
        Self::Upnp(event)
    }
}

impl NockchainBehaviour {
    /// Create a new NockchainBehaviour with NAT traversal support
    fn new_with_nat(
        keypair: &libp2p::identity::Keypair,
        allowed: Option<allow_block_list::Behaviour<allow_block_list::AllowedPeers>>,
        limits: connection_limits::ConnectionLimits,
        memory_limits: Option<memory_connection_limits::Behaviour>,
        nat_config: &NatConfig,
        relay_client: Option<relay::client::Behaviour>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let peer_id = libp2p::identity::PeerId::from_public_key(&keypair.public());

        // Existing behaviour setup
        let identify_config = identify::Config::new(
            IDENTIFY_PROTOCOL_VERSION.to_string(), 
            keypair.public()
        )
        .with_interval(IDENTIFY_INTERVAL)
        .with_hide_listen_addrs(true);
        let identify_behaviour = identify::Behaviour::new(identify_config);

        let memory_store = kad::store::MemoryStore::new(peer_id);
        let kad_config = kad::Config::new(libp2p::StreamProtocol::new(KAD_PROTOCOL_VERSION));
        let kad_behaviour = kad::Behaviour::with_config(peer_id, memory_store, kad_config);

        let request_response_config = request_response::Config::default()
            .with_max_concurrent_streams(REQUEST_RESPONSE_MAX_CONCURRENT_STREAMS)
            .with_request_timeout(REQUEST_RESPONSE_TIMEOUT);
        let request_response_behaviour = cbor::Behaviour::new(
            [(
                libp2p::StreamProtocol::new(REQ_RES_PROTOCOL_VERSION),
                request_response::ProtocolSupport::Full,
            )],
            request_response_config,
        );

        // NAT traversal behaviours
        let relay_client = if nat_config.enable_relay_client {
            Toggle::from(relay_client)
        } else {
            Toggle::from(None)
        };

        let relay_server = if nat_config.enable_relay_server {
            let relay_config = relay::Config {
                max_reservations: nat_config.relay_server_limits.max_reservations,
                max_reservations_per_peer: 4,
                max_circuits: nat_config.relay_server_limits.max_circuits,
                max_circuits_per_peer: nat_config.relay_server_limits.max_circuits_per_peer,
                reservation_duration: nat_config.reservation_timeout,
                circuit_src_timeout: nat_config.circuit_request_timeout,
                max_circuit_duration: nat_config.relay_server_limits.max_circuit_duration,
                max_circuit_bytes: nat_config.relay_server_limits.max_circuit_bytes,
                ..Default::default()
            };
            Toggle::from(Some(relay::Behaviour::new(peer_id, relay_config)))
        } else {
            Toggle::from(None)
        };

        let dcutr = if nat_config.enable_dcutr {
            Toggle::from(Some(dcutr::Behaviour::new(peer_id)))
        } else {
            Toggle::from(None)
        };

        let autonat = if nat_config.enable_autonat {
            let autonat_config = autonat::Config {
                retry_interval: nat_config.autonat_probe_interval,
                refresh_interval: nat_config.autonat_probe_interval * 3,
                boot_delay: Duration::from_secs(5),
                throttle_server_period: Duration::from_secs(1),
                only_global_ips: true,
                ..Default::default()
            };
            Toggle::from(Some(autonat::Behaviour::new(peer_id, autonat_config)))
        } else {
            Toggle::from(None)
        };

        let upnp = if nat_config.enable_upnp {
            let upnp_config = upnp::Config {
                lease_duration: nat_config.upnp_lease_duration,
                ..Default::default()
            };
            Toggle::from(Some(upnp::tokio::Behaviour::new(upnp_config)))
        } else {
            Toggle::from(None)
        };

        // Existing setup
        let connection_limits_behaviour = connection_limits::Behaviour::new(limits);
        let memory_connection_limits = Toggle::from(memory_limits);
        let allow_peers = Toggle::from(allowed);
        
        let peer_store_config = libp2p::peer_store::memory_store::Config::default()
            .set_record_capacity(PEER_STORE_RECORD_CAPACITY.try_into().unwrap());
        let peer_store_memory = libp2p::peer_store::memory_store::MemoryStore::new(peer_store_config);
        let peer_store_behaviour = libp2p::peer_store::Behaviour::new(peer_store_memory);

        Ok(NockchainBehaviour {
            ping: ping::Behaviour::default(),
            identify: identify_behaviour,
            kad: kad_behaviour,
            allow_block_list: allow_block_list::Behaviour::default(),
            allow_peers,
            request_response: request_response_behaviour,
            connection_limits: connection_limits_behaviour,
            memory_connection_limits,
            peer_store: peer_store_behaviour,
            
            // NAT traversal
            relay_client,
            relay_server,
            dcutr,
            autonat,
            upnp,
        })
    }
}
```

#### 2.2 Update Swarm Creation

**File**: `crates/nockchain-libp2p-io/src/p2p.rs` (continued)

```rust
/// Create a swarm with NAT traversal support
pub fn start_swarm_with_nat(
    keypair: Keypair,
    bind: Vec<Multiaddr>,
    allowed: Option<allow_block_list::Behaviour<allow_block_list::AllowedPeers>>,
    limits: connection_limits::ConnectionLimits,
    memory_limits: Option<memory_connection_limits::Behaviour>,
    nat_config: NatConfig,
) -> Result<Swarm<NockchainBehaviour>, Box<dyn std::error::Error>> {
    let (resolver_config, resolver_opts) =
        if let Ok(sys) = hickory_resolver::system_conf::read_system_conf() {
            debug!("resolver configs and opts: {:?}", sys);
            sys
        } else {
            (ResolverConfig::cloudflare(), ResolverOpts::default())
        };

    let mut swarm_builder = libp2p::SwarmBuilder::with_existing_identity(keypair.clone())
        .with_tokio()
        .with_quic_config(|mut cfg| {
            cfg.max_idle_timeout = MAX_IDLE_TIMEOUT_MILLISECS;
            cfg.keep_alive_interval = KEEP_ALIVE_INTERVAL;
            cfg.handshake_timeout = HANDSHAKE_TIMEOUT;
            cfg
        })
        .with_dns_config(resolver_config, resolver_opts);

    // Add relay transport if relay client is enabled
    let mut swarm = if nat_config.enable_relay_client {
        swarm_builder
            .with_relay_client(|key, relay_behaviour| {
                (libp2p::relay::client::Transport::new(key, relay_behaviour), relay_behaviour)
            })?
            .with_behaviour(|key, relay_behaviour| {
                NockchainBehaviour::new_with_nat(
                    key, 
                    allowed, 
                    limits, 
                    memory_limits, 
                    &nat_config, 
                    Some(relay_behaviour)
                )
            })?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(SWARM_IDLE_TIMEOUT))
            .with_connection_timeout(CONNECTION_TIMEOUT)
            .build()
    } else {
        swarm_builder
            .with_behaviour(|key| {
                NockchainBehaviour::new_with_nat(
                    key, 
                    allowed, 
                    limits, 
                    memory_limits, 
                    &nat_config, 
                    None
                )
            })?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(SWARM_IDLE_TIMEOUT))
            .with_connection_timeout(CONNECTION_TIMEOUT)
            .build()
    };

    // Listen on regular addresses
    for bind_addr in bind {
        swarm.listen_on(bind_addr)?;
    }
    
    // Set up relay connections if relay client is enabled
    if nat_config.enable_relay_client {
        setup_relay_connections(&mut swarm, &nat_config)?;
    }

    Ok(swarm)
}

/// Set up connections to relay nodes and listen on relay addresses
fn setup_relay_connections(
    swarm: &mut Swarm<NockchainBehaviour>, 
    nat_config: &NatConfig
) -> Result<(), Box<dyn std::error::Error>> {
    let relay_nodes = if nat_config.relay_nodes.is_empty() {
        get_default_relay_nodes()
    } else {
        nat_config.relay_nodes.clone()
    };

    for (relay_peer_id, relay_addr) in relay_nodes.iter().take(nat_config.max_relay_connections) {
        // Connect to relay node
        let dial_addr = relay_addr.clone()
            .with(libp2p::multiaddr::Protocol::P2p(*relay_peer_id));
        
        info!("Connecting to relay node: {} at {}", relay_peer_id, dial_addr);
        swarm.dial(dial_addr)?;
        
        // Listen on relay address for incoming circuit connections
        let relay_listen_addr = relay_addr.clone()
            .with(libp2p::multiaddr::Protocol::P2p(*relay_peer_id))
            .with(libp2p::multiaddr::Protocol::P2pCircuit);
        
        info!("Listening on relay address: {}", relay_listen_addr);
        swarm.listen_on(relay_listen_addr)?;
    }
    
    Ok(())
}

/// Backward compatibility function
pub fn start_swarm(
    keypair: Keypair,
    bind: Vec<Multiaddr>,
    allowed: Option<allow_block_list::Behaviour<allow_block_list::AllowedPeers>>,
    limits: connection_limits::ConnectionLimits,
    memory_limits: Option<memory_connection_limits::Behaviour>,
) -> Result<Swarm<NockchainBehaviour>, Box<dyn std::error::Error>> {
    start_swarm_with_nat(
        keypair,
        bind,
        allowed,
        limits,
        memory_limits,
        NatConfig::default(),
    )
}
```

### Phase 3: Event Handling

#### 3.1 Update Main Event Loop

**File**: `crates/nockchain-libp2p-io/src/nc.rs`

Add NAT event handling to the main event loop:

```rust
// Add to the main event loop in make_libp2p_driver function

match event {
    // ... existing event handlers ...

    SwarmEvent::Behaviour(NockchainEvent::RelayClient(event)) => {
        handle_relay_client_event(event, &mut swarm).await?;
    }
    
    SwarmEvent::Behaviour(NockchainEvent::RelayServer(event)) => {
        handle_relay_server_event(event).await?;
    }
    
    SwarmEvent::Behaviour(NockchainEvent::Dcutr(event)) => {
        handle_dcutr_event(event, &mut swarm).await?;
    }
    
    SwarmEvent::Behaviour(NockchainEvent::Autonat(event)) => {
        handle_autonat_event(event, &mut swarm).await?;
    }
    
    SwarmEvent::Behaviour(NockchainEvent::Upnp(event)) => {
        handle_upnp_event(event, &mut swarm).await?;
    }

    // ... rest of existing handlers ...
}

/// Handle relay client events
async fn handle_relay_client_event(
    event: relay::client::Event,
    swarm: &mut Swarm<NockchainBehaviour>,
) -> Result<(), NockAppError> {
    match event {
        relay::client::Event::ReservationReqAccepted { 
            relay_peer_id, 
            renewal, 
            limit 
        } => {
            if renewal {
                info!("Relay reservation renewed with {} (limit: {:?})", relay_peer_id, limit);
            } else {
                info!("Relay reservation established with {} (limit: {:?})", relay_peer_id, limit);
                
                // Add relay to Kademlia DHT for discovery
                if let Some(relay_addr) = swarm.external_addresses().next() {
                    swarm.behaviour_mut().kad.add_address(&relay_peer_id, relay_addr.clone());
                }
            }
        }
        
        relay::client::Event::ReservationReqFailed { 
            relay_peer_id, 
            renewal, 
            error 
        } => {
            if renewal {
                warn!("Relay reservation renewal failed with {}: {:?}", relay_peer_id, error);
            } else {
                warn!("Relay reservation request failed with {}: {:?}", relay_peer_id, error);
            }
            // TODO: Try next relay or implement backoff strategy
        }
        
        relay::client::Event::ReservationTimedOut { relay_peer_id } => {
            warn!("Relay reservation timed out with {}", relay_peer_id);
            // TODO: Attempt to reconnect
        }
        
        relay::client::Event::InboundCircuitEstablished { 
            src_peer_id, 
            limit 
        } => {
            info!("Inbound circuit established from {} (limit: {:?})", src_peer_id, limit);
        }
        
        relay::client::Event::InboundCircuitReqFailed { 
            src_peer_id, 
            error 
        } => {
            debug!("Inbound circuit request failed from {}: {:?}", src_peer_id, error);
        }
        
        relay::client::Event::OutboundCircuitEstablished { 
            relay_peer_id, 
            limit 
        } => {
            info!("Outbound circuit established via {} (limit: {:?})", relay_peer_id, limit);
        }
        
        relay::client::Event::OutboundCircuitReqFailed { 
            relay_peer_id, 
            error 
        } => {
            warn!("Outbound circuit request failed via {}: {:?}", relay_peer_id, error);
        }
    }
    
    Ok(())
}

/// Handle relay server events
async fn handle_relay_server_event(
    event: relay::Event,
) -> Result<(), NockAppError> {
    match event {
        relay::Event::ReservationReqAccepted { 
            src_peer_id, 
            renewed 
        } => {
            if renewed {
                debug!("Relay reservation renewed for {}", src_peer_id);
            } else {
                info!("Relay reservation accepted for {}", src_peer_id);
            }
        }
        
        relay::Event::ReservationReqFailed { 
            src_peer_id, 
            error 
        } => {
            debug!("Relay reservation failed for {}: {:?}", src_peer_id, error);
        }
        
        relay::Event::ReservationTimedOut { src_peer_id } => {
            debug!("Relay reservation timed out for {}", src_peer_id);
        }
        
        relay::Event::CircuitReqReceived { 
            src_peer_id, 
            dst_peer_id 
        } => {
            debug!("Circuit request received: {} -> {}", src_peer_id, dst_peer_id);
        }
        
        relay::Event::CircuitReqAccepted { 
            src_peer_id, 
            dst_peer_id 
        } => {
            debug!("Circuit request accepted: {} -> {}", src_peer_id, dst_peer_id);
        }
        
        relay::Event::CircuitReqFailed { 
            src_peer_id, 
            dst_peer_id, 
            error 
        } => {
            debug!("Circuit request failed: {} -> {} ({:?})", src_peer_id, dst_peer_id, error);
        }
        
        relay::Event::CircuitClosed { 
            src_peer_id, 
            dst_peer_id, 
            error 
        } => {
            debug!("Circuit closed: {} -> {} ({:?})", src_peer_id, dst_peer_id, error);
        }
    }
    
    Ok(())
}

/// Handle DCUtR (Direct Connection Upgrade through Relay) events
async fn handle_dcutr_event(
    event: dcutr::Event,
    swarm: &mut Swarm<NockchainBehaviour>,
) -> Result<(), NockAppError> {
    match event {
        dcutr::Event::InitiatedDirectConnectionUpgrade { 
            remote_peer_id, 
            local_relayed_addr 
        } => {
            info!("Attempting direct connection upgrade to {} via {}", 
                  remote_peer_id, local_relayed_addr);
        }
        
        dcutr::Event::RemoteInitiatedDirectConnectionUpgrade { 
            remote_peer_id, 
            remote_relayed_addr 
        } => {
            info!("Remote peer {} attempting direct connection upgrade via {}", 
                  remote_peer_id, remote_relayed_addr);
        }
        
        dcutr::Event::DirectConnectionUpgradeSucceeded { remote_peer_id } => {
            info!("Direct connection upgrade succeeded with {}", remote_peer_id);
            
            // Update Kademlia with direct address if available
            if let Some(direct_addr) = swarm.network_info()
                .connection_counters()
                .num_connections_established() {
                // Add logic to update routing table with direct connection
            }
        }
        
        dcutr::Event::DirectConnectionUpgradeFailed { 
            remote_peer_id, 
            error 
        } => {
            warn!("Direct connection upgrade failed with {}: {:?}", remote_peer_id, error);
            // Continue using relayed connection
        }
    }
    
    Ok(())
}

/// Handle AutoNAT events
async fn handle_autonat_event(
    event: autonat::Event,
    swarm: &mut Swarm<NockchainBehaviour>,
) -> Result<(), NockAppError> {
    match event {
        autonat::Event::InboundProbe(probe_event) => {
            match probe_event {
                autonat::InboundProbeEvent::Request { peer, addresses, response_sender } => {
                    debug!("AutoNAT probe request from {} for addresses: {:?}", peer, addresses);
                    // libp2p handles the response automatically
                }
                autonat::InboundProbeEvent::Response { peer, .. } => {
                    debug!("AutoNAT probe response sent to {}", peer);
                }
                autonat::InboundProbeEvent::Error { peer, error } => {
                    debug!("AutoNAT inbound probe error with {}: {:?}", peer, error);
                }
            }
        }
        
        autonat::Event::OutboundProbe(probe_event) => {
            match probe_event {
                autonat::OutboundProbeEvent::Request { probe_id, peer } => {
                    debug!("AutoNAT probe {} sent to {}", probe_id, peer);
                }
                
                autonat::OutboundProbeEvent::Response { 
                    probe_id, 
                    peer, 
                    address 
                } => {
                    info!("AutoNAT probe {} confirmed external address {} via {}", 
                          probe_id, address, peer);
                    
                    // Add confirmed external address
                    swarm.add_external_address(address);
                }
                
                autonat::OutboundProbeEvent::Error { 
                    probe_id, 
                    peer, 
                    error 
                } => {
                    debug!("AutoNAT probe {} to {} failed: {:?}", probe_id, peer, error);
                }
            }
        }
        
        autonat::Event::StatusChanged { old, new } => {
            info!("NAT status changed from {:?} to {:?}", old, new);
            
            match new {
                autonat::NatStatus::Public(addr) => {
                    info!("Detected as public node with address: {}", addr);
                    
                    // Add external address for advertising
                    swarm.add_external_address(addr);
                    
                    // Consider enabling relay server if not already enabled
                    // This would require dynamic behavior modification
                }
                
                autonat::NatStatus::Private => {
                    info!("Detected as private node behind NAT - using relay for connectivity");
                    
                    // Ensure relay client is working properly
                    // Might want to trigger additional relay connections
                }
                
                autonat::NatStatus::Unknown => {
                    warn!("NAT status unknown - potential network issues");
                    
                    // Consider diagnostic actions or fallback strategies
                }
            }
        }
    }
    
    Ok(())
}

/// Handle UPnP events
async fn handle_upnp_event(
    event: upnp::Event,
    swarm: &mut Swarm<NockchainBehaviour>,
) -> Result<(), NockAppError> {
    match event {
        upnp::Event::NewExternalAddr(addr) => {
            info!("UPnP successfully mapped external address: {}", addr);
            
            // Add external address for peer discovery
            swarm.add_external_address(addr);
        }
        
        upnp::Event::ExpiredExternalAddr(addr) => {
            info!("UPnP mapping expired for address: {}", addr);
            
            // Remove expired external address
            swarm.remove_external_address(&addr);
        }
        
        upnp::Event::GatewayNotFound => {
            warn!("UPnP gateway not found - router may not support UPnP or it's disabled");
            info!("Consider manual port forwarding or using relay connections");
        }
        
        upnp::Event::NonRoutableGateway => {
            warn!("UPnP gateway is not routable - may be behind carrier-grade NAT");
            info!("Relay connections will be used for connectivity");
        }
    }
    
    Ok(())
}
```

### Phase 4: CLI Integration

#### 4.1 Update Command Line Arguments

**File**: `crates/nockchain/src/lib.rs`

```rust
#[derive(Parser, Debug, Clone)]
#[command(name = "nockchain")]
pub struct NockchainCli {
    // ... existing fields ...

    // NAT traversal options
    #[arg(long, help = "Enable relay client for NAT traversal", default_value = "true")]
    pub enable_relay_client: bool,
    
    #[arg(long, help = "Enable relay server (for public nodes only)", default_value = "false")]
    pub enable_relay_server: bool,
    
    #[arg(long, help = "Enable direct connection upgrade through relay", default_value = "true")]
    pub enable_dcutr: bool,
    
    #[arg(long, help = "Enable automatic NAT detection", default_value = "true")]
    pub enable_autonat: bool,
    
    #[arg(long, help = "Enable UPnP automatic port mapping", default_value = "true")]
    pub enable_upnp: bool,
    
    #[arg(
        long, 
        help = "Known relay nodes (format: peer_id@multiaddr)", 
        action = ArgAction::Append
    )]
    pub relay_nodes: Vec<String>,
    
    #[arg(long, help = "Maximum number of relay connections", default_value = "3")]
    pub max_relay_connections: usize,
    
    #[arg(long, help = "AutoNAT probe interval in seconds", default_value = "60")]
    pub autonat_probe_interval: u64,
    
    #[arg(long, help = "UPnP lease duration in seconds", default_value = "3600")]
    pub upnp_lease_duration: u64,
    
    #[arg(long, help = "Specific external port to request via UPnP")]
    pub upnp_external_port: Option<u16>,
}

impl NockchainCli {
    /// Create NAT configuration from CLI arguments
    pub fn create_nat_config(&self) -> Result<NatConfig, String> {
        let mut relay_nodes = Vec::new();
        
        // Parse relay node strings
        for relay_str in &self.relay_nodes {
            match parse_relay_node(relay_str) {
                Ok((peer_id, multiaddr)) => {
                    relay_nodes.push((peer_id, multiaddr));
                }
                Err(e) => {
                    return Err(format!("Invalid relay node '{}': {}", relay_str, e));
                }
            }
        }
        
        // Add default relay nodes if none specified
        if relay_nodes.is_empty() {
            relay_nodes = get_default_relay_nodes();
        }
        
        Ok(NatConfig {
            enable_relay_client: self.enable_relay_client,
            enable_relay_server: self.enable_relay_server,
            enable_dcutr: self.enable_dcutr,
            enable_autonat: self.enable_autonat,
            enable_upnp: self.enable_upnp,
            
            relay_nodes,
            max_relay_connections: self.max_relay_connections,
            reservation_timeout: Duration::from_secs(30),
            circuit_request_timeout: Duration::from_secs(10),
            
            autonat_probe_interval: Duration::from_secs(self.autonat_probe_interval),
            autonat_server_count: 3,
            
            upnp_lease_duration: Duration::from_secs(self.upnp_lease_duration),
            upnp_external_port: self.upnp_external_port,
            
            relay_server_limits: RelayServerLimits::default(),
        })
    }
}
```

#### 4.2 Update Swarm Initialization

**File**: `crates/nockchain-libp2p-io/src/nc.rs`

```rust
// Update the make_libp2p_driver function to accept NAT configuration

#[instrument(skip(keypair, bind, allowed, limits, memory_limits, equix_builder, nat_config))]
pub fn make_libp2p_driver_with_nat(
    keypair: Keypair,
    bind: Vec<Multiaddr>,
    allowed: Option<allow_block_list::Behaviour<allow_block_list::AllowedPeers>>,
    limits: connection_limits::ConnectionLimits,
    memory_limits: Option<memory_connection_limits::Behaviour>,
    initial_peers: &[Multiaddr],
    equix_builder: equix::EquiXBuilder,
    init_complete_tx: Option<tokio::sync::oneshot::Sender<()>>,
    nat_config: NatConfig,
) -> IODriverFn {
    let initial_peers = Vec::from(initial_peers);
    Box::new(|mut handle| {
        let metrics = NockchainP2PMetrics::register(gnort::global_metrics_registry())
            .expect("Failed to register metrics!");

        Box::pin(async move {
            let mut swarm = match crate::p2p::start_swarm_with_nat(
                keypair, 
                bind, 
                allowed, 
                limits, 
                memory_limits, 
                nat_config
            ) {
                Ok(swarm) => swarm,
                Err(e) => {
                    error!("Could not create swarm with NAT support: {}", e);
                    let (_, handle_clone) = handle.dup();
                    tokio::spawn(async move {
                        if let Err(e) = handle_clone.exit.exit(1).await {
                            error!("Failed to send exit signal: {}", e);
                        }
                    });
                    return Err(NockAppError::OtherError);
                }
            };
            
            // ... rest of the function remains the same but with NAT event handling added
        })
    })
}

// Backward compatibility function
pub fn make_libp2p_driver(
    keypair: Keypair,
    bind: Vec<Multiaddr>,
    allowed: Option<allow_block_list::Behaviour<allow_block_list::AllowedPeers>>,
    limits: connection_limits::ConnectionLimits,
    memory_limits: Option<memory_connection_limits::Behaviour>,
    initial_peers: &[Multiaddr],
    equix_builder: equix::EquiXBuilder,
    init_complete_tx: Option<tokio::sync::oneshot::Sender<()>>,
) -> IODriverFn {
    make_libp2p_driver_with_nat(
        keypair,
        bind,
        allowed,
        limits,
        memory_limits,
        initial_peers,
        equix_builder,
        init_complete_tx,
        NatConfig::default(),
    )
}
```

#### 4.3 Update Main Application

**File**: `crates/nockchain/src/lib.rs` (update init_with_kernel function)

```rust
pub async fn init_with_kernel(
    cli: Option<NockchainCli>,
    kernel_jam: &[u8],
    hot_state: &[HotEntry],
) -> Result<NockApp, Box<dyn Error>> {
    // ... existing setup code ...

    // Create NAT configuration from CLI
    let nat_config = if let Some(cli) = &cli {
        cli.create_nat_config()?
    } else {
        NatConfig::default()
    };

    // ... existing peer and connection setup ...

    let libp2p_driver = nockchain_libp2p_io::nc::make_libp2p_driver_with_nat(
        keypair,
        bind_multiaddrs,
        allowed,
        limits,
        memory_limits,
        &peer_multiaddrs,
        equix_builder,
        Some(libp2p_init_tx),
        nat_config, // Pass NAT configuration
    );
    nockapp.add_io_driver(libp2p_driver).await;

    // ... rest of the function ...
}
```

### Phase 5: Testing and Validation

#### 5.1 Unit Tests

**File**: `crates/nockchain-libp2p-io/src/nat_tests.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::PeerId;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_nat_config_creation() {
        let nat_config = NatConfig::default();
        assert!(nat_config.enable_relay_client);
        assert!(!nat_config.enable_relay_server);
        assert!(nat_config.enable_dcutr);
        assert!(nat_config.enable_autonat);
        assert!(nat_config.enable_upnp);
    }

    #[tokio::test]
    async fn test_relay_node_parsing() {
        let relay_str = "12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X@/ip4/127.0.0.1/tcp/4001";
        let result = parse_relay_node(relay_str);
        assert!(result.is_ok());

        let (peer_id, multiaddr) = result.unwrap();
        assert_eq!(peer_id.to_string(), "12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X");
        assert_eq!(multiaddr.to_string(), "/ip4/127.0.0.1/tcp/4001");
    }

    #[tokio::test]
    async fn test_swarm_creation_with_nat() {
        let keypair = Keypair::generate_ed25519();
        let nat_config = NatConfig {
            enable_relay_client: true,
            enable_relay_server: false,
            enable_dcutr: true,
            enable_autonat: true,
            enable_upnp: false, // Disable for testing
            ..Default::default()
        };

        let swarm = start_swarm_with_nat(
            keypair,
            vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
            None,
            connection_limits::ConnectionLimits::default(),
            None,
            nat_config,
        );

        assert!(swarm.is_ok());
    }

    #[tokio::test]
    async fn test_relay_client_event_handling() {
        let relay_peer_id = PeerId::random();
        let event = relay::client::Event::ReservationReqAccepted {
            relay_peer_id,
            renewal: false,
            limit: None,
        };

        // Create a mock swarm for testing
        let keypair = Keypair::generate_ed25519();
        let mut swarm = start_swarm_with_nat(
            keypair,
            vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
            None,
            connection_limits::ConnectionLimits::default(),
            None,
            NatConfig::default(),
        ).unwrap();

        // Test event handling doesn't panic
        let result = handle_relay_client_event(event, &mut swarm).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_autonat_status_change() {
        let old_status = autonat::NatStatus::Unknown;
        let new_status = autonat::NatStatus::Public("/ip4/1.2.3.4/tcp/4001".parse().unwrap());
        
        let event = autonat::Event::StatusChanged {
            old,
            new: new_status,
        };

        let keypair = Keypair::generate_ed25519();
        let mut swarm = start_swarm_with_nat(
            keypair,
            vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
            None,
            connection_limits::ConnectionLimits::default(),
            None,
            NatConfig::default(),
        ).unwrap();

        let result = handle_autonat_event(event, &mut swarm).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cli_nat_config_creation() {
        let cli = NockchainCli {
            enable_relay_client: true,
            enable_relay_server: false,
            enable_dcutr: true,
            enable_autonat: true,
            enable_upnp: true,
            relay_nodes: vec![
                "12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X@/ip4/127.0.0.1/tcp/4001".to_string()
            ],
            max_relay_connections: 3,
            autonat_probe_interval: 60,
            upnp_lease_duration: 3600,
            upnp_external_port: None,
            // ... other required fields with default values
        };

        let nat_config = cli.create_nat_config();
        assert!(nat_config.is_ok());

        let config = nat_config.unwrap();
        assert!(config.enable_relay_client);
        assert!(!config.enable_relay_server);
        assert_eq!(config.relay_nodes.len(), 1);
        assert_eq!(config.max_relay_connections, 3);
    }
}
```

#### 5.2 Integration Tests

**File**: `crates/nockchain-libp2p-io/src/integration_tests.rs`

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    #[ignore] // Only run with --ignored flag due to network requirements
    async fn test_relay_connection_establishment() {
        // This test requires actual relay infrastructure
        // Implementation would test end-to-end relay connectivity
    }

    #[tokio::test] 
    #[ignore] // Only run with --ignored flag due to UPnP requirements
    async fn test_upnp_port_mapping() {
        // This test requires UPnP-enabled router
        // Implementation would test actual UPnP port mapping
    }

    #[tokio::test]
    async fn test_nat_detection_simulation() {
        // Simulate different NAT scenarios and test detection
        // Implementation would mock AutoNAT responses
    }
}
```

### Phase 6: Documentation and Configuration

#### 6.1 Update README

**File**: `crates/nockchain-libp2p-io/README.md`

```markdown
# Nockchain LibP2P Networking with NAT Support

This crate provides peer-to-peer networking for Nockchain with comprehensive NAT traversal support.

## NAT Traversal Features

### Supported Methods

1. **Circuit Relay v2**: Connect through public relay nodes
2. **DCUtR (Direct Connection Upgrade through Relay)**: Establish direct connections via relay coordination
3. **AutoNAT**: Automatic NAT status detection and external address discovery
4. **UPnP**: Automatic port forwarding on compatible routers

### Configuration

NAT traversal can be configured via command line arguments:

```bash
# Enable all NAT features (default)
nockchain --enable-relay-client --enable-dcutr --enable-autonat --enable-upnp

# Run as public relay server
nockchain --enable-relay-server --bind /ip4/0.0.0.0/tcp/4001

# Use specific relay nodes
nockchain --relay-nodes "peer_id@/ip4/relay1.example.com/tcp/4001"

# Disable specific features if needed
nockchain --enable-upnp=false
```

### For Network Operators

To set up a relay node:

```bash
# Run with relay server enabled on a public IP
nockchain --enable-relay-server \
          --bind /ip4/0.0.0.0/tcp/4001 \
          --bind /ip6/::/tcp/4001
```

### Troubleshooting

Common issues and solutions:

1. **No incoming connections**: Check firewall settings, enable UPnP, or use relay
2. **Relay connection failures**: Verify relay node addresses and connectivity
3. **UPnP not working**: Enable UPnP on router or use manual port forwarding
4. **High latency**: DCUtR should upgrade to direct connections automatically

### Security Considerations

- Relay traffic is encrypted end-to-end
- Relay servers cannot decrypt message content
- UPnP mappings have time limits and are renewed automatically
- AutoNAT probes use the existing secure libp2p protocols
```

#### 6.2 Configuration Examples

**File**: `examples/nat-configuration.md`

```markdown
# NAT Configuration Examples

## Scenario 1: Home User Behind Router

Most home users are behind NAT. Enable all features for best connectivity:

```bash
nockchain --enable-relay-client \
          --enable-dcutr \
          --enable-autonat \
          --enable-upnp
```

## Scenario 2: VPS/Cloud Instance (Public IP)

Public nodes should act as relay servers to help the network:

```bash
nockchain --enable-relay-server \
          --enable-autonat \
          --bind /ip4/0.0.0.0/tcp/4001 \
          --bind /ip6/::/tcp/4001
```

## Scenario 3: Corporate Network

Corporate networks often block UPnP and have strict firewalls:

```bash
nockchain --enable-relay-client \
          --enable-dcutr \
          --enable-autonat \
          --enable-upnp=false \
          --relay-nodes "relay1@/ip4/relay.nockchain.io/tcp/4001"
```

## Scenario 4: Mobile/Cellular Connection

Mobile connections often use carrier-grade NAT:

```bash
nockchain --enable-relay-client \
          --enable-dcutr \
          --enable-autonat \
          --enable-upnp=false \
          --max-relay-connections 5
```

## Environment Variables

Alternatively, configure via environment variables:

```bash
export NOCKCHAIN_ENABLE_RELAY_CLIENT=true
export NOCKCHAIN_ENABLE_RELAY_SERVER=false
export NOCKCHAIN_ENABLE_DCUTR=true
export NOCKCHAIN_ENABLE_AUTONAT=true
export NOCKCHAIN_ENABLE_UPNP=true
export NOCKCHAIN_RELAY_NODES="relay1@/ip4/relay.example.com/tcp/4001,relay2@/ip4/relay2.example.com/tcp/4001"
```
```

## Implementation Timeline

### Phase 1 (Week 1-2): Core Infrastructure
- [ ] Add NAT traversal dependencies
- [ ] Create NAT configuration module
- [ ] Update NetworkBehaviour with NAT protocols
- [ ] Basic swarm creation with NAT support

### Phase 2 (Week 3-4): Event Handling
- [ ] Implement relay client event handling
- [ ] Implement relay server event handling
- [ ] Implement DCUtR event handling
- [ ] Implement AutoNAT event handling
- [ ] Implement UPnP event handling

### Phase 3 (Week 5-6): CLI Integration
- [ ] Add command line arguments for NAT configuration
- [ ] Update main application initialization
- [ ] Create configuration validation
- [ ] Add environment variable support

### Phase 4 (Week 7-8): Testing and Validation
- [ ] Write comprehensive unit tests
- [ ] Create integration tests
- [ ] Test with real relay infrastructure
- [ ] Performance testing and optimization

### Phase 5 (Week 9-10): Documentation and Deployment
- [ ] Update documentation
- [ ] Create configuration examples
- [ ] Set up default relay infrastructure
- [ ] Deploy relay nodes for the network

## Success Criteria

1. **Connectivity**: Nodes behind NAT can connect to the network
2. **Performance**: Direct connections established when possible
3. **Reliability**: Fallback mechanisms work under various network conditions
4. **Usability**: Minimal configuration required for typical users
5. **Security**: NAT traversal doesn't compromise network security

## Risk Mitigation

1. **Relay Infrastructure**: Set up multiple geographically distributed relays
2. **Fallback Strategies**: Multiple NAT traversal methods for redundancy
3. **Performance Impact**: Monitor and optimize relay usage patterns
4. **Security Review**: Comprehensive security audit of NAT implementation
5. **Compatibility**: Extensive testing across different network environments

## Future Enhancements

1. **Adaptive Relay Selection**: Choose optimal relays based on latency and load
2. **Relay Discovery**: Automatic discovery of available relay nodes
3. **Bandwidth Management**: Quality of service controls for relay traffic
4. **Advanced Hole Punching**: Support for more NAT types and configurations
5. **Network Topology Optimization**: Intelligent peer selection for better connectivity