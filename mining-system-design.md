# Nockchain Mining System Design

## Executive Summary

Nockchain implements a sophisticated mining architecture that separates the mining computation from the main blockchain logic. The system uses STARK-based proof-of-work, with mining performed in isolated Nock kernels that communicate with the main node through a well-defined interface. This design provides security, modularity, and scalability while leveraging Nock's deterministic computation model.

## Architecture Overview

```text
┌─────────────────────┐
│   Nockchain Node    │
│  (Rust + Main Kernel)│
└──────────┬──────────┘
           │
     ┌─────┴─────┐
     │  Mining   │
     │  Driver   │
     └─────┬─────┘
           │
    ┌──────┴──────┐
    │   Mining    │
    │  Kernels    │
    │ (Parallel)  │
    └─────────────┘
```

## Core Components

### 1. Mining Driver (`/crates/nockchain/src/mining.rs`)

The mining driver acts as the bridge between the Rust runtime and Nock mining kernels:

- **Lifecycle Management**: Creates and manages mining kernel instances
- **Work Distribution**: Dispatches candidate blocks to mining kernels
- **Result Collection**: Receives successful proofs and forwards them to the main kernel
- **Concurrency Control**: Manages parallel mining attempts with different nonces

Key structures:

- `MiningWire`: Defines communication protocol between components
- `MiningKeyConfig`: Handles mining reward distribution configuration

### 2. Mining Kernel (`/hoon/apps/dumbnet/miner.hoon`)

A specialized Nock kernel dedicated to proof-of-work computation:

- **Stateless Design**: Each mining attempt is independent
- **STARK Proof Generation**: Uses zero-knowledge proofs for PoW
- **Nonce Search**: Iterates through nonces to find valid proofs
- **Early Exit**: Stops when a valid proof is found

Key operations:

- `%candidate` poke: Receives block commitment and starts mining
- `%command %pow` effect: Returns successful proof

### 3. Main Blockchain Kernel (`/hoon/apps/dumbnet/inner.hoon`)

Manages the overall blockchain state and consensus:

- **Candidate Generation**: Creates block templates for mining
- **Block Validation**: Verifies proofs and transaction validity
- **Chain Selection**: Maintains the heaviest valid chain
- **State Management**: Updates UTXO set and consensus state

## Mining Process Flow

### 1. Candidate Generation

```text
1. New block received or timeout occurs
2. Main kernel collects pending transactions
3. Calculates coinbase rewards and splits
4. Generates block header with merkle root
5. Emits %mine effect with block commitment
```

### 2. Mining Execution

```text
1. Mining driver receives %mine effect
2. Spawns new mining kernel instance
3. Sends %candidate poke with:
   - Block commitment
   - Starting nonce
   - Target difficulty
4. Mining kernel iterates nonces:
   - Computes STARK proof
   - Checks if hash meets target
   - Returns proof if successful
```

### 3. Block Finalization

```text
1. Mining driver receives proof
2. Sends %mined poke to main kernel
3. Main kernel validates proof
4. Updates blockchain state
5. Gossips block to network peers
```

## Proof-of-Work Algorithm

### STARK-Based PoW

Nockchain uses STARK proofs as its proof-of-work mechanism:

1. **Puzzle Construction**: Block commitment serves as input
2. **Proof Generation**: Computes STARK proof of Nock execution
3. **Difficulty Check**: Hash of proof must be below target
4. **Deterministic Verification**: Anyone can verify proof validity

### Difficulty Adjustment

Following Bitcoin's model with modifications:

- **Adjustment Period**: Every 2016 blocks
- **Target Time**: 14 days per epoch
- **Max Change**: 4x increase/decrease cap
- **Calculation**: Based on actual vs expected epoch duration

## Network Communication

### Block Propagation

The libp2p layer handles distributed communication:

1. **Gossip Protocol**: New blocks broadcast to all peers
2. **Direct Requests**: Specific blocks fetched on demand
3. **Chain Sync**: Elder blocks requested for initial sync
4. **Transaction Pool**: Pending transactions shared across network

### Message Types

- `GetBlock`: Request specific block by height/ID
- `Block`: Block data response
- `GetElders`: Request ancestor blocks
- `Elders`: Multiple blocks response
- `Advert`: Announce new block availability

## Consensus Mechanism

### Chain Selection Rules

1. **Heaviest Chain**: Follow chain with most accumulated work
2. **Valid Blocks Only**: All consensus rules must be satisfied
3. **Reorg Handling**: Switch to heavier chain when discovered

### Block Validation

Required checks:

- Proof-of-work meets target difficulty
- Timestamp constraints satisfied
- All transactions valid
- Proper coinbase amount
- Correct parent block reference

### Time Constraints

- **Median Time Past**: Block time > median of last 11 blocks
- **Future Limit**: Block time < current time + 2 hours
- **Ensures**: Monotonic time progression

## Mining Configuration

### Basic Mining

```bash
nockchain --mine --mining-pubkey <public-key>
```

### Advanced Configuration

Multi-signature or split coinbase:

```bash
nockchain --mine --mining-key-adv "share1,m1:key1,key2,key3" --mining-key-adv "share2,m2:key4,key5"
```

### Configuration Options

- **Single Key**: Simple mining to one address
- **Multi-sig**: m-of-n signature requirement
- **Split Coinbase**: Distribute rewards to multiple addresses
- **Dynamic Control**: Enable/disable mining at runtime

## Performance Considerations

### Parallelization

- Multiple mining kernels run concurrently
- Each tries different nonce ranges
- First valid proof wins
- Automatic work cancellation on new blocks

### Resource Management

- Temporary directories for kernel isolation
- Memory-mapped files for efficiency
- Controlled thread pool for mining
- Graceful shutdown handling

## Security Properties

### Isolation

- Mining kernels run in separate processes
- No shared state between attempts
- Failure contained to single kernel
- Main blockchain state protected

### Determinism

- STARK proofs provide deterministic verification
- No randomness in consensus rules
- Reproducible blockchain state
- Audit trail for all operations

## Future Enhancements

### Potential Improvements

1. **GPU Mining**: Accelerate STARK proof generation
2. **Mining Pools**: Decentralized pool protocol
3. **Dynamic Difficulty**: More responsive adjustment algorithm
4. **Alternative PoW**: Support for different proof systems

### Scalability Considerations

- Sharding for parallel block production
- Light client mining participation
- Optimized proof verification
- Reduced mining kernel overhead

## Conclusion

Nockchain's mining architecture achieves a balance between security, modularity, and performance. By separating mining logic into isolated kernels and using STARK-based proofs, the system provides strong security guarantees while maintaining the flexibility to evolve. The design supports various mining configurations and scales well with increased network participation.
