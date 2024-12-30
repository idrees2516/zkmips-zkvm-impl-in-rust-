use std::{
    collections::HashMap,
    sync::Arc,
};
use tokio::sync::RwLock;
use blake3::Hash;
use patricia_trie::{TrieMut, Trie};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use async_trait::async_trait;
use futures::stream::{self, StreamExt};
use rayon::prelude::*;
use dashmap::DashMap;
use metrics::{counter, gauge, histogram};
use zksync_crypto::{
    franklin_crypto::{
        bellman::pairing::bn256::{Bn256, Fr},
        circuit::{boolean::Boolean, num::AllocatedNum},
    },
    params::{JUBJUB_PARAMS, RESCUE_PARAMS},
    circuit::{
        utils::allocate_inputs_for_witness,
        rescue::{rescue_hash, RescueHashParams},
    },
};
use zksync_types::{
    AccountId, Address, BlockNumber, H256, Nonce, TokenId, PubKeyHash,
    account::{Account, PubKeyHash},
    tx::{PackedEthSignature, TxSignature},
    ZkSyncOp, ZkSyncTx,
};

mod trie;
mod snapshot;
mod storage;
pub mod merkle;
pub mod proof;
pub mod cache;
pub mod account;
pub mod transition;
pub mod circuit;
pub mod witness;
pub mod verifier;

use self::merkle::MerkleTree;
use self::proof::StateProof;
use self::cache::StateCache;
use self::snapshot::Snapshot;
use self::storage::Storage;
use self::account::Account;
use self::transition::StateTransition;
use self::circuit::ZkCircuit;
use self::witness::ZkWitness;
use self::verifier::ZkVerifier;

pub use trie::{MerklePatriciaTrie, TrieError};
pub use snapshot::{Snapshot, SnapshotManager};
pub use storage::{Storage, StorageManager};

#[derive(Error, Debug)]
pub enum StateError {
    #[error("Trie error: {0}")]
    TrieError(#[from] TrieError),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Concurrency error: {0}")]
    ConcurrencyError(String),
    #[error("Proof verification failed: {0}")]
    ProofVerificationError(String),
    #[error("Snapshot error: {0}")]
    SnapshotError(String),
    #[error("Circuit generation error: {0}")]
    CircuitError(String),
    #[error("Witness generation error: {0}")]
    WitnessError(String),
    #[error("Verification error: {0}")]
    VerificationError(String),
}

pub type StateResult<T> = Result<T, StateError>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub nonce: Nonce,
    pub balance: HashMap<TokenId, u128>,
    pub pub_key_hash: PubKeyHash,
    pub storage_root: Hash,
    pub code_hash: Hash,
    pub last_modified_block: BlockNumber,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateUpdate {
    pub block_number: BlockNumber,
    pub accounts: HashMap<Address, AccountUpdate>,
    pub timestamp: u64,
    pub metadata: HashMap<String, Vec<u8>>,
    pub zk_proof: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountUpdate {
    pub nonce: Option<Nonce>,
    pub balance: HashMap<TokenId, u128>,
    pub pub_key_hash: Option<PubKeyHash>,
    pub storage: HashMap<Hash, Vec<u8>>,
    pub code: Option<Vec<u8>>,
    pub metadata: HashMap<String, Vec<u8>>,
}

pub struct StateManager {
    trie: Arc<RwLock<MerklePatriciaTrie>>,
    storage: Arc<StorageManager>,
    snapshots: Arc<SnapshotManager>,
    cache: Arc<StateCache>,
    pending_updates: Arc<DashMap<Address, AccountUpdate>>,
    circuit: ZkCircuit,
    verifier: ZkVerifier,
}

#[derive(Default)]
struct StateCache {
    accounts: DashMap<Address, (Account, BlockNumber)>,
    storage: DashMap<(Address, Hash), (Vec<u8>, BlockNumber)>,
    code: DashMap<Hash, Vec<u8>>,
}

impl StateManager {
    pub fn new(
        storage: Arc<StorageManager>,
        snapshots: Arc<SnapshotManager>,
    ) -> Self {
        Self {
            trie: Arc::new(RwLock::new(MerklePatriciaTrie::new())),
            storage,
            snapshots,
            cache: Arc::new(StateCache::default()),
            pending_updates: Arc::new(DashMap::new()),
            circuit: ZkCircuit::new(&RESCUE_PARAMS),
            verifier: ZkVerifier::new(&JUBJUB_PARAMS),
        }
    }

    pub async fn get_account(&self, address: &Address, block_number: BlockNumber) -> StateResult<Option<Account>> {
        if let Some((account, last_block)) = self.cache.accounts.get(address) {
            if *last_block >= block_number {
                counter!("state.cache.hit", 1);
                return Ok(Some(account.clone()));
            }
        }
        
        let trie = self.trie.read().await;
        let account_bytes = match trie.get(address.as_bytes())? {
            Some(bytes) => bytes,
            None => {
                counter!("state.account.miss", 1);
                return Ok(None);
            }
        };
        
        let account: Account = bincode::deserialize(&account_bytes)
            .map_err(|_| StateError::InvalidState("Failed to deserialize account".into()))?;
            
        self.cache.accounts.insert(*address, (account.clone(), block_number));
        counter!("state.cache.update", 1);
        
        Ok(Some(account))
    }

    pub async fn get_storage(&self, address: &Address, key: &Hash, block_number: BlockNumber) -> StateResult<Option<Vec<u8>>> {
        if let Some((value, last_block)) = self.cache.storage.get(&(*address, *key)) {
            if *last_block >= block_number {
                counter!("state.storage.cache.hit", 1);
                return Ok(Some(value.clone()));
            }
        }
        
        let account = match self.get_account(address, block_number).await? {
            Some(account) => account,
            None => {
                counter!("state.storage.account.miss", 1);
                return Ok(None);
            }
        };
        
        let value = self.storage.get_storage(address, key, &account.storage_root).await?;
        
        if let Some(value) = value.as_ref() {
            self.cache.storage.insert((*address, *key), (value.clone(), block_number));
            counter!("state.storage.cache.update", 1);
        }
        
        Ok(value)
    }

    pub async fn get_code(&self, code_hash: &Hash) -> StateResult<Option<Vec<u8>>> {
        if let Some(code) = self.cache.code.get(code_hash) {
            counter!("state.code.cache.hit", 1);
            return Ok(Some(code.clone()));
        }
        
        let code = self.storage.get_code(code_hash).await?;
        
        if let Some(code) = code.as_ref() {
            self.cache.code.insert(*code_hash, code.clone());
            counter!("state.code.cache.update", 1);
        }
        
        Ok(code)
    }

    pub async fn update_state(&mut self, update: StateUpdate) -> StateResult<Hash> {
        let mut trie = self.trie.write().await;
        let storage = self.storage.clone();
        
        let accounts_to_update: Vec<_> = update.accounts.into_iter().collect();
        let results: Vec<_> = stream::iter(accounts_to_update)
            .map(|(address, account_update)| async move {
                self.update_account(&mut trie, &storage, &address, account_update, update.block_number).await
            })
            .buffer_unordered(100)
            .collect()
            .await;

        for result in results {
            result?;
        }
        
        let witness = self.generate_witness(&trie, update.block_number).await?;
        let proof = self.circuit.generate_proof(&witness)?;
        
        self.snapshots.create_snapshot(
            update.block_number,
            trie.root_hash()?,
            update.timestamp,
            Some(proof),
        ).await?;
        
        let root_hash = trie.root_hash()?;
        gauge!("state.root_hash", root_hash.as_bytes().to_vec());
        
        Ok(root_hash)
    }

    async fn update_account(
        &self,
        trie: &mut MerklePatriciaTrie,
        storage: &StorageManager,
        address: &Address,
        account_update: AccountUpdate,
        block_number: BlockNumber,
    ) -> StateResult<()> {
        let mut account = match self.get_account(address, block_number).await? {
            Some(account) => account,
            None => Account {
                nonce: Nonce(0),
                balance: HashMap::new(),
                pub_key_hash: PubKeyHash::default(),
                storage_root: Hash::default(),
                code_hash: Hash::default(),
                last_modified_block: block_number,
            },
        };
        
        if let Some(nonce) = account_update.nonce {
            account.nonce = nonce;
        }
        for (token_id, balance) in account_update.balance {
            account.balance.insert(token_id, balance);
        }
        if let Some(pub_key_hash) = account_update.pub_key_hash {
            account.pub_key_hash = pub_key_hash;
        }
        
        if !account_update.storage.is_empty() {
            account.storage_root = storage.update_storage(
                address,
                account_update.storage,
                &account.storage_root,
            ).await?;
        }
        
        if let Some(code) = account_update.code {
            let code_hash = blake3::hash(&code);
            storage.store_code(&code_hash, &code).await?;
            account.code_hash = code_hash;
        }
        
        account.last_modified_block = block_number;
        
        let account_bytes = bincode::serialize(&account)
            .map_err(|_| StateError::InvalidState("Failed to serialize account".into()))?;
        trie.insert(address.as_bytes(), &account_bytes)?;
        
        self.cache.accounts.insert(*address, (account, block_number));
        
        Ok(())
    }

    pub async fn revert_to_snapshot(&mut self, block_number: BlockNumber) -> StateResult<()> {
        let snapshot = self.snapshots.get_snapshot(block_number).await?;
        
        let mut trie = self.trie.write().await;
        *trie = MerklePatriciaTrie::from_root(snapshot.root_hash)?;
        
        self.cache.accounts.clear();
        self.cache.storage.clear();
        self.cache.code.clear();
        
        counter!("state.revert", 1);
        
        Ok(())
    }

    pub async fn get_proof(&self, address: &Address, storage_keys: &[Hash]) -> StateResult<StateProof> {
        let trie = self.trie.read().await;
        
        let account_proof = trie.get_proof(address.as_bytes())?;
        
        let mut storage_proofs = Vec::new();
        if let Some(account_bytes) = trie.get(address.as_bytes())? {
            let account: Account = bincode::deserialize(&account_bytes)
                .map_err(|_| StateError::InvalidState("Failed to deserialize account".into()))?;
                
            storage_proofs = stream::iter(storage_keys)
                .map(|key| async {
                    self.storage.get_proof(address, key, &account.storage_root).await
                })
                .buffer_unordered(50)
                .collect::<Vec<StateResult<_>>>()
                .await
                .into_iter()
                .collect::<StateResult<Vec<_>>>()?;
        }
        
        Ok(StateProof {
            account_proof,
            storage_proofs,
        })
    }

    pub async fn verify_proof(
        &self,
        address: &Address,
        storage_keys: &[Hash],
        proof: &StateProof,
        root_hash: Hash,
    ) -> StateResult<bool> {
        let trie = MerklePatriciaTrie::from_root(root_hash)?;
        if !trie.verify_proof(address.as_bytes(), &proof.account_proof)? {
            return Ok(false);
        }
        
        if let Some(account_bytes) = trie.get(address.as_bytes())? {
            let account: Account = bincode::deserialize(&account_bytes)
                .map_err(|_| StateError::InvalidState("Failed to deserialize account".into()))?;
                
            let results: Vec<_> = storage_keys.par_iter().zip(proof.storage_proofs.par_iter())
                .map(|(key, proof)| {
                    self.storage.verify_proof(address, key, proof, &account.storage_root)
                })
                .collect::<Vec<StateResult<_>>>()
                .into_iter()
                .collect::<StateResult<Vec<_>>>()?;
            
            if results.iter().any(|&r| !r) {
                return Ok(false);
            }
        }
        
        Ok(true)
    }

    pub async fn begin_transaction(&self) -> StateResult<()> {
        self.pending_updates.clear();
        Ok(())
    }

    pub async fn commit_transaction(&self) -> StateResult<Hash> {
        let mut update = StateUpdate {
            block_number: self.snapshots.get_latest_block_number().await?,
            accounts: self.pending_updates.iter().map(|r| (*r.key(), r.value().clone())).collect(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            metadata: HashMap::new(),
            zk_proof: None,
        };

        let root_hash = self.update_state(update).await?;
        self.pending_updates.clear();
        Ok(root_hash)
    }

    pub async fn rollback_transaction(&self) -> StateResult<()> {
        self.pending_updates.clear();
        Ok(())
    }

    pub async fn get_state_size(&self) -> StateResult<usize> {
        let trie = self.trie.read().await;
        Ok(trie.get_size())
    }

    pub async fn get_accounts_in_range(&self, start: &Address, end: &Address, limit: usize) -> StateResult<Vec<(Address, Account)>> {
        let trie = self.trie.read().await;
        let mut accounts = Vec::new();
        
        for item in trie.range(start.as_
