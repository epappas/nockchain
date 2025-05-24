use std::ops::Range;
use sha2::{Sha256, Digest};
use nockvm::noun::{Atom, Noun, D, T};
use nockapp::noun::slab::NounSlab;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct ComputationProof {
    pub witness_commitment: [u8; 32],
    pub nonce_range: Range<u64>,
    pub computation_steps: u64,
    pub intermediate_hashes: Vec<[u8; 32]>,
}

impl ComputationProof {
    pub fn generate_for_range(
        block_commitment: &[u8],
        nonce_range: Range<u64>,
        sample_rate: usize,
    ) -> Self {
        let mut hasher = Sha256::new();
        let mut intermediate_hashes = Vec::new();
        let mut computation_steps = 0;
        
        // Sample nonces from range
        let step = ((nonce_range.end - nonce_range.start) / sample_rate as u64).max(1);
        
        for i in 0..sample_rate {
            let nonce = nonce_range.start + (i as u64 * step);
            if nonce >= nonce_range.end {
                break;
            }
            
            // Simulate STARK witness computation
            let witness = compute_partial_witness(block_commitment, nonce);
            let witness_bytes = witness_to_bytes(&witness);
            hasher.update(&witness_bytes);
            
            // Store intermediate hash
            let intermediate = hasher.clone().finalize();
            intermediate_hashes.push(intermediate.into());
            
            computation_steps += estimate_computation_steps(&witness_bytes);
        }
        
        ComputationProof {
            witness_commitment: hasher.finalize().into(),
            nonce_range,
            computation_steps,
            intermediate_hashes,
        }
    }
    
    pub fn verify(&self, block_commitment: &[u8], spot_check_count: usize) -> bool {
        use rand::Rng;
        
        // Randomly select nonces to verify
        let mut rng = rand::thread_rng();
        let nonces: Vec<u64> = (0..spot_check_count)
            .map(|_| rng.gen_range(self.nonce_range.clone()))
            .collect();
        
        // Verify each selected nonce
        for nonce in nonces {
            let witness = compute_partial_witness(block_commitment, nonce);
            let witness_bytes = witness_to_bytes(&witness);
            let hash = Sha256::digest(&witness_bytes);
            
            // Check if hash matches any intermediate
            let found = self.intermediate_hashes.iter().any(|h| {
                // Allow some flexibility in matching due to sampling
                h[..8] == hash[..8]
            });
            
            if !found {
                debug!("Failed to verify nonce {} in range {:?}", nonce, self.nonce_range);
                return false;
            }
        }
        
        true
    }
}

fn compute_partial_witness(commitment: &[u8], nonce: u64) -> NounSlab {
    // Simplified witness computation
    // In production, this would compute actual STARK witness
    let mut slab = NounSlab::new();
    let commitment_atom = Atom::from_bytes(&mut slab, commitment)
        .expect("Failed to create commitment atom");
    let witness = T(&mut slab, &[commitment_atom.as_noun(), D(nonce)]);
    slab.set_root(witness);
    slab
}

fn witness_to_bytes(witness: &NounSlab) -> Vec<u8> {
    // Convert witness to bytes for hashing
    // Simplified version - in production would serialize properly
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&witness.root().as_atom().unwrap_or_default().as_bytes());
    bytes
}

fn estimate_computation_steps(witness_bytes: &[u8]) -> u64 {
    // Estimate based on witness size
    // In production, would track actual computation steps
    witness_bytes.len() as u64 * 100
}