use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use kernels::miner::KERNEL;
use nockapp::kernel::checkpoint::JamPaths;
use nockapp::kernel::form::Kernel;
use nockapp::nockapp::driver::{IODriverFn, NockAppHandle, PokeResult};
use nockapp::nockapp::wire::Wire;
use nockapp::nockapp::NockAppError;
use nockapp::noun::slab::NounSlab;
use nockapp::noun::{AtomExt, NounExt};
use nockvm::noun::{Atom, D, T};
use nockvm_macros::tas;
use tempfile::tempdir;
use tracing::{instrument, warn, info, error, debug};
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use anyhow::anyhow;
use bytes::Bytes;
use crate::pool_client::{PoolClient, PoolJob, ShareSubmission, ShareType};


pub enum MiningWire {
    Mined,
    Candidate,
    SetPubKey,
    Enable,
    PoolCandidate,
    ShareFound,
}

impl MiningWire {
    pub fn verb(&self) -> &'static str {
        match self {
            MiningWire::Mined => "mined",
            MiningWire::SetPubKey => "setpubkey",
            MiningWire::Candidate => "candidate",
            MiningWire::Enable => "enable",
            MiningWire::PoolCandidate => "pool-candidate",
            MiningWire::ShareFound => "share-found",
        }
    }
}

impl Wire for MiningWire {
    const VERSION: u64 = 1;
    const SOURCE: &'static str = "miner";

    fn to_wire(&self) -> nockapp::wire::WireRepr {
        let tags = vec![self.verb().into()];
        nockapp::wire::WireRepr::new(MiningWire::SOURCE, MiningWire::VERSION, tags)
    }
}

#[derive(Debug, Clone)]
pub struct MiningKeyConfig {
    pub share: u64,
    pub m: u64,
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMiningConfig {
    pub pool_url: String,
    pub worker_name: String,
    pub worker_password: Option<String>,
    pub share_difficulty_multiplier: f64,
}

#[derive(Debug, Clone)]
pub enum MiningMode {
    Solo(Vec<MiningKeyConfig>),
    Pool(PoolMiningConfig),
}

impl FromStr for MiningKeyConfig {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Expected format: "share,m:key1,key2,key3"
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return Err("Invalid format. Expected 'share,m:key1,key2,key3'".to_string());
        }

        let share_m: Vec<&str> = parts[0].split(',').collect();
        if share_m.len() != 2 {
            return Err("Invalid share,m format".to_string());
        }

        let share = share_m[0].parse::<u64>().map_err(|e| e.to_string())?;
        let m = share_m[1].parse::<u64>().map_err(|e| e.to_string())?;
        let keys: Vec<String> = parts[1].split(',').map(String::from).collect();

        Ok(MiningKeyConfig { share, m, keys })
    }
}

pub fn create_mining_driver(
    mining_mode: Option<MiningMode>,
    mine: bool,
    init_complete_tx: Option<tokio::sync::oneshot::Sender<()>>,
) -> IODriverFn {
    match mining_mode {
        Some(MiningMode::Pool(pool_config)) => {
            if mine {
                create_pool_mining_driver(pool_config, init_complete_tx)
            } else {
                // Pool mode requires mining to be enabled
                warn!("Pool mode specified but mining is disabled. Using solo mode.");
                create_solo_mining_driver(None, false, init_complete_tx)
            }
        }
        Some(MiningMode::Solo(mining_config)) => {
            create_solo_mining_driver(Some(mining_config), mine, init_complete_tx)
        }
        None => create_solo_mining_driver(None, mine, init_complete_tx),
    }
}

fn create_solo_mining_driver(
    mining_config: Option<Vec<MiningKeyConfig>>,
    mine: bool,
    init_complete_tx: Option<tokio::sync::oneshot::Sender<()>>,
) -> IODriverFn {
    Box::new(move |mut handle| {
        Box::pin(async move {
            let Some(configs) = mining_config else {
                enable_mining(&handle, false).await?;

                if let Some(tx) = init_complete_tx {
                    tx.send(()).map_err(|_| {
                        warn!("Could not send driver initialization for mining driver.");
                        NockAppError::OtherError
                    })?;
                }

                return Ok(());
            };
            if configs.len() == 1
                && configs[0].share == 1
                && configs[0].m == 1
                && configs[0].keys.len() == 1
            {
                set_mining_key(&handle, configs[0].keys[0].clone()).await?;
            } else {
                set_mining_key_advanced(&handle, configs).await?;
            }
            enable_mining(&handle, mine).await?;

            if let Some(tx) = init_complete_tx {
                tx.send(()).map_err(|_| {
                    warn!("Could not send driver initialization for mining driver.");
                    NockAppError::OtherError
                })?;
            }

            if !mine {
                return Ok(());
            }
            let mut next_attempt: Option<NounSlab> = None;
            let mut current_attempt: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();

            loop {
                tokio::select! {
                    effect_res = handle.next_effect() => {
                        let Ok(effect) = effect_res else {
                          warn!("Error receiving effect in mining driver: {effect_res:?}");
                        continue;
                        };
                        let Ok(effect_cell) = (unsafe { effect.root().as_cell() }) else {
                            drop(effect);
                            continue;
                        };

                        if effect_cell.head().eq_bytes("mine") {
                            let candidate_slab = {
                                let mut slab = NounSlab::new();
                                slab.copy_into(effect_cell.tail());
                                slab
                            };
                            if !current_attempt.is_empty() {
                                next_attempt = Some(candidate_slab);
                            } else {
                                let (cur_handle, attempt_handle) = handle.dup();
                                handle = cur_handle;
                                current_attempt.spawn(mining_attempt(candidate_slab, attempt_handle));
                            }
                        }
                    },
                    mining_attempt_res = current_attempt.join_next(), if !current_attempt.is_empty()  => {
                        if let Some(Err(e)) = mining_attempt_res {
                            warn!("Error during mining attempt: {e:?}");
                        }
                        let Some(candidate_slab) = next_attempt else {
                            continue;
                        };
                        next_attempt = None;
                        let (cur_handle, attempt_handle) = handle.dup();
                        handle = cur_handle;
                        current_attempt.spawn(mining_attempt(candidate_slab, attempt_handle));

                    }
                }
            }
        })
    })
}

pub async fn mining_attempt(candidate: NounSlab, handle: NockAppHandle) -> () {
    let snapshot_dir =
        tokio::task::spawn_blocking(|| tempdir().expect("Failed to create temporary directory"))
            .await
            .expect("Failed to create temporary directory");
    let hot_state = zkvm_jetpack::hot::produce_prover_hot_state();
    let snapshot_path_buf = snapshot_dir.path().to_path_buf();
    let jam_paths = JamPaths::new(snapshot_dir.path());
    // Spawns a new std::thread for this mining attempt
    let kernel =
        Kernel::load_with_hot_state_huge(snapshot_path_buf, jam_paths, KERNEL, &hot_state, false)
            .await
            .expect("Could not load mining kernel");
    let effects_slab = kernel
        .poke(MiningWire::Candidate.to_wire(), candidate)
        .await
        .expect("Could not poke mining kernel with candidate");
    for effect in effects_slab.to_vec() {
        let Ok(effect_cell) = (unsafe { effect.root().as_cell() }) else {
            drop(effect);
            continue;
        };
        if effect_cell.head().eq_bytes("command") {
            handle
                .poke(MiningWire::Mined.to_wire(), effect)
                .await
                .expect("Could not poke nockchain with mined PoW");
        }
    }
}

#[instrument(skip(handle, pubkey))]
async fn set_mining_key(
    handle: &NockAppHandle,
    pubkey: String,
) -> Result<PokeResult, NockAppError> {
    let mut set_mining_key_slab = NounSlab::new();
    let set_mining_key = Atom::from_value(&mut set_mining_key_slab, "set-mining-key")
        .expect("Failed to create set-mining-key atom");
    let pubkey_cord =
        Atom::from_value(&mut set_mining_key_slab, pubkey).expect("Failed to create pubkey atom");
    let set_mining_key_poke = T(
        &mut set_mining_key_slab,
        &[D(tas!(b"command")), set_mining_key.as_noun(), pubkey_cord.as_noun()],
    );
    set_mining_key_slab.set_root(set_mining_key_poke);

    handle
        .poke(MiningWire::SetPubKey.to_wire(), set_mining_key_slab)
        .await
}

async fn set_mining_key_advanced(
    handle: &NockAppHandle,
    configs: Vec<MiningKeyConfig>,
) -> Result<PokeResult, NockAppError> {
    let mut set_mining_key_slab = NounSlab::new();
    let set_mining_key_adv = Atom::from_value(&mut set_mining_key_slab, "set-mining-key-advanced")
        .expect("Failed to create set-mining-key-advanced atom");

    // Create the list of configs
    let mut configs_list = D(0);
    for config in configs {
        // Create the list of keys
        let mut keys_noun = D(0);
        for key in config.keys {
            let key_atom =
                Atom::from_value(&mut set_mining_key_slab, key).expect("Failed to create key atom");
            keys_noun = T(&mut set_mining_key_slab, &[key_atom.as_noun(), keys_noun]);
        }

        // Create the config tuple [share m keys]
        let config_tuple = T(
            &mut set_mining_key_slab,
            &[D(config.share), D(config.m), keys_noun],
        );

        configs_list = T(&mut set_mining_key_slab, &[config_tuple, configs_list]);
    }

    let set_mining_key_poke = T(
        &mut set_mining_key_slab,
        &[D(tas!(b"command")), set_mining_key_adv.as_noun(), configs_list],
    );
    set_mining_key_slab.set_root(set_mining_key_poke);

    handle
        .poke(MiningWire::SetPubKey.to_wire(), set_mining_key_slab)
        .await
}

//TODO add %set-mining-key-multisig poke
#[instrument(skip(handle))]
async fn enable_mining(handle: &NockAppHandle, enable: bool) -> Result<PokeResult, NockAppError> {
    let mut enable_mining_slab = NounSlab::new();
    let enable_mining = Atom::from_value(&mut enable_mining_slab, "enable-mining")
        .expect("Failed to create enable-mining atom");
    let enable_mining_poke = T(
        &mut enable_mining_slab,
        &[D(tas!(b"command")), enable_mining.as_noun(), D(if enable { 0 } else { 1 })],
    );
    enable_mining_slab.set_root(enable_mining_poke);
    handle
        .poke(MiningWire::Enable.to_wire(), enable_mining_slab)
        .await
}

// Pool mining implementation
fn create_pool_mining_driver(
    pool_config: PoolMiningConfig,
    init_complete_tx: Option<tokio::sync::oneshot::Sender<()>>,
) -> IODriverFn {
    Box::new(move |mut handle| {
        Box::pin(async move {
            info!("Starting pool mining driver for {}", pool_config.pool_url);
            
            // Create pool client
            let pool_client = match PoolClient::new(&pool_config).await {
                Ok(client) => Arc::new(client),
                Err(e) => {
                    error!("Failed to create pool client: {}", e);
                    if let Some(tx) = init_complete_tx {
                        let _ = tx.send(());
                    }
                    return Err(NockAppError::OtherError);
                }
            };
            
            // Wait for authorization
            let mut retries = 0;
            while !pool_client.is_authorized().await && retries < 30 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                retries += 1;
            }
            
            if !pool_client.is_authorized().await {
                error!("Failed to authorize with pool");
                if let Some(tx) = init_complete_tx {
                    let _ = tx.send(());
                }
                return Err(NockAppError::OtherError);
            }
            
            info!("Successfully authorized with pool");
            
            if let Some(tx) = init_complete_tx {
                let _ = tx.send(());
            }
            
            // Main pool mining loop
            loop {
                tokio::select! {
                    // Receive work from pool
                    job_res = pool_client.recv_job() => {
                        match job_res {
                            Ok(job) => {
                                info!("Received job {} from pool", job.id);
                                let pool_client_clone = pool_client.clone();
                                let (handle_copy, handle_new) = handle.dup();
                                handle = handle_new;
                                spawn_pool_mining_task(job, handle_copy, pool_client_clone).await;
                            }
                            Err(e) => {
                                error!("Failed to receive job: {}", e);
                                drop(e); // Explicitly drop the error before await
                                tokio::time::sleep(Duration::from_secs(5)).await;
                            }
                        }
                    }
                    
                    // Note: Effects from pool mining are handled directly in spawn_pool_mining_task
                    // This is here for any other effects that might come through
                    effect_res = handle.next_effect() => {
                        let Ok(_effect) = effect_res else {
                            warn!("Error receiving effect in pool mining driver: {effect_res:?}");
                            continue;
                        };
                        // Other effects would be handled here if needed
                    }
                }
            }
        })
    })
}

async fn spawn_pool_mining_task(
    job: PoolJob,
    handle: NockAppHandle,
    pool_client: Arc<PoolClient>,
) {
    tokio::spawn(async move {
        let snapshot_dir = match tempdir() {
            Ok(dir) => dir,
            Err(e) => {
                error!("Failed to create temporary directory: {}", e);
                return;
            }
        };
        
        let hot_state = zkvm_jetpack::hot::produce_prover_hot_state();
        let jam_paths = JamPaths::new(snapshot_dir.path());
        
        // Load mining kernel with pool-specific configuration
        let kernel = match Kernel::load_with_hot_state_huge(
            snapshot_dir.path().to_path_buf(),
            jam_paths,
            KERNEL,
            &hot_state,
            false,
        )
        .await {
            Ok(kernel) => kernel,
            Err(e) => {
                error!("Could not load mining kernel: {}", e);
                return;
            }
        };
        
        // Create share mining configuration
        let mut config_slab = NounSlab::new();
        let commitment_bytes = Bytes::copy_from_slice(&job.block_commitment);
        let commitment_atom = Atom::from_bytes(&mut config_slab, &commitment_bytes);
        let target_bytes = Bytes::copy_from_slice(&job.target);
        let target_atom = Atom::from_bytes(&mut config_slab, &target_bytes);
        let share_target_bytes = Bytes::copy_from_slice(&job.share_target);
        let share_target_atom = Atom::from_bytes(&mut config_slab, &share_target_bytes);
        
        let config_noun = T(
            &mut config_slab,
            &[
                commitment_atom.as_noun(),
                target_atom.as_noun(),
                share_target_atom.as_noun(),
                D(job.nonce_start),
                D(job.nonce_range),
            ],
        );
        config_slab.set_root(config_noun);
        
        // Start mining with share support
        let effects = match kernel
            .poke(MiningWire::PoolCandidate.to_wire(), config_slab)
            .await {
                Ok(effects) => effects,
                Err(e) => {
                    error!("Could not poke mining kernel: {}", e);
                    return;
                }
            };
        
        // Process effects (shares and blocks)
        for effect in effects.to_vec() {
            // Store job_id and pool_client in the effect by wrapping it
            let mut wrapped_slab = NounSlab::new();
            let job_id_bytes = Bytes::copy_from_slice(job.id.as_bytes());
            let job_id_atom = Atom::from_bytes(&mut wrapped_slab, &job_id_bytes);
            
            let wrapped_effect = unsafe {
                T(
                    &mut wrapped_slab,
                    &[job_id_atom.as_noun(), *effect.root()],
                )
            };
            wrapped_slab.set_root(wrapped_effect);
            
            // Store pool_client reference for the handler
            let pool_client_clone = pool_client.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_pool_mining_effect(wrapped_slab, &pool_client_clone).await {
                    error!("Failed to handle pool effect: {}", e);
                }
            });
        }
    });
}

async fn handle_pool_mining_effect(
    wrapped_effect: NounSlab,
    pool_client: &Arc<PoolClient>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = unsafe { wrapped_effect.root() };
    let wrapper_cell = match root.as_cell() {
        Ok(cell) => cell,
        Err(_) => return Ok(()),
    };
    
    // Extract job_id and actual effect
    let job_id_atom = wrapper_cell.head().as_atom()?;
    let job_id_bytes = job_id_atom.as_ne_bytes();
    let job_id = String::from_utf8(job_id_bytes.to_vec())
        .map_err(|_| anyhow::anyhow!("Invalid job_id UTF-8"))?;
    let effect_cell = match wrapper_cell.tail().as_cell() {
        Ok(cell) => cell,
        Err(_) => return Ok(()),
    };
    
    if effect_cell.head().eq_bytes("share") {
        // Handle share submission
        let share_cell = match effect_cell.tail().as_cell() {
            Ok(cell) => cell,
            Err(_) => return Ok(()),
        };
        
        let nonce = share_cell.head().as_atom()?.as_u64()?;
        let witness_atom = share_cell.tail().as_atom()?;
        let witness_data = witness_atom.as_ne_bytes();
        let witness_commitment = sha2::Sha256::digest(&witness_data).into();
        
        let share = ShareSubmission {
            job_id,
            miner_id: pool_client.config.worker_name.clone(),
            share_type: ShareType::ComputationProof {
                nonce,
                witness_commitment,
                computation_steps: witness_data.len() as u64,
            },
        };
        
        pool_client.submit_share(share).await?;
        debug!("Submitted share for nonce {}", nonce);
        
    } else if effect_cell.head().eq_bytes("block") {
        // Handle block submission
        let block_cell = match effect_cell.tail().as_cell() {
            Ok(cell) => cell,
            Err(_) => return Ok(()),
        };
        
        let nonce = block_cell.head().as_atom()?.as_u64()?;
        let proof_atom = block_cell.tail().as_atom()?;
        let proof = proof_atom.as_ne_bytes().to_vec();
        
        let share = ShareSubmission {
            job_id,
            miner_id: pool_client.config.worker_name.clone(),
            share_type: ShareType::ValidBlock { nonce, proof },
        };
        
        pool_client.submit_share(share).await?;
        info!("Submitted BLOCK for nonce {}!", nonce);
    }
    
    Ok(())
}
