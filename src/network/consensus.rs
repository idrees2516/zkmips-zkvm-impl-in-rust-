use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use blake3::Hash;
use serde::{Deserialize, Serialize};
use crate::network::{Block, Transaction, NetworkError};

#[derive(Clone, Debug)]
pub struct ConsensusConfig {
    pub block_time: Duration,
    pub max_block_size: usize,
    pub min_validators: usize,
    pub max_validators: usize,
    pub validator_stake_threshold: u64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            block_time: Duration::from_secs(15),
            max_block_size: 1024 * 1024, // 1MB
            min_validators: 4,
            max_validators: 100,
            validator_stake_threshold: 1000,
        }
    }
}

#[derive(Debug)]
pub struct ConsensusEngine {
    config: ConsensusConfig,
    state: Arc<RwLock<ConsensusState>>,
}

#[derive(Debug)]
struct ConsensusState {
    current_round: u64,
    current_height: u64,
    validators: HashMap<String, ValidatorInfo>,
    pending_transactions: VecDeque<Transaction>,
    pending_blocks: HashMap<Hash, Block>,
    finalized_blocks: HashMap<u64, Block>,
    votes: HashMap<Hash, HashSet<String>>,
    last_finalized_time: Instant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorInfo {
    pub address: String,
    pub stake: u64,
    pub last_proposed: u64,
    pub total_proposed: u64,
    pub total_validated: u64,
    pub uptime: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConsensusStatus {
    pub height: u64,
    pub round: u64,
    pub pending_transactions: usize,
    pub pending_blocks: usize,
    pub active_validators: usize,
    pub last_finalized_time: Duration,
}

impl ConsensusEngine {
    pub fn new(config: ConsensusConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(ConsensusState::new())),
        }
    }

    pub async fn start(&self) -> Result<(), NetworkError> {
        let mut interval = tokio::time::interval(self.config.block_time);
        
        loop {
            interval.tick().await;
            
            // Process consensus round
            self.process_round().await?;
            
            // Check for finalization
            self.check_finalization().await?;
            
            // Cleanup old data
            self.cleanup().await?;
        }
    }

    pub async fn process_transaction(&self, transaction: Transaction) -> Result<(), NetworkError> {
        let mut state = self.state.write().await;
        
        // Validate transaction
        if !self.validate_transaction(&transaction) {
            return Err(NetworkError::ConsensusError("Invalid transaction".into()));
        }
        
        // Add to pending transactions
        state.pending_transactions.push_back(transaction);
        
        Ok(())
    }

    pub async fn process_block(&self, block: Block) -> Result<(), NetworkError> {
        let mut state = self.state.write().await;
        
        // Validate block
        if !self.validate_block(&block) {
            return Err(NetworkError::ConsensusError("Invalid block".into()));
        }
        
        let block_hash = block.hash();
        
        // Add to pending blocks
        state.pending_blocks.insert(block_hash, block);
        
        // Initialize vote set
        state.votes.insert(block_hash, HashSet::new());
        
        Ok(())
    }

    pub async fn submit_vote(&self, validator: String, block_hash: Hash) -> Result<(), NetworkError> {
        let mut state = self.state.write().await;
        
        // Validate validator
        if !self.is_valid_validator(&validator, &state) {
            return Err(NetworkError::ConsensusError("Invalid validator".into()));
        }
        
        // Add vote
        if let Some(votes) = state.votes.get_mut(&block_hash) {
            votes.insert(validator);
            
            // Check if block can be finalized
            if self.check_consensus(votes.len(), state.validators.len()) {
                if let Some(block) = state.pending_blocks.remove(&block_hash) {
                    state.finalized_blocks.insert(block.header.number, block);
                    state.last_finalized_time = Instant::now();
                }
            }
        }
        
        Ok(())
    }

    async fn process_round(&self) -> Result<(), NetworkError> {
        let mut state = self.state.write().await;
        
        // Increment round
        state.current_round += 1;
        
        // Select proposer
        let proposer = self.select_proposer(&state)?;
        
        // Create new block
        let block = self.create_block(&mut state, &proposer)?;
        
        // Broadcast block
        // This would be implemented by the network layer
        
        Ok(())
    }

    async fn check_finalization(&self) -> Result<(), NetworkError> {
        let state = self.state.read().await;
        
        // Check for timeout
        if state.last_finalized_time.elapsed() > Duration::from_secs(60) {
            return Err(NetworkError::ConsensusError("Finalization timeout".into()));
        }
        
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), NetworkError> {
        let mut state = self.state.write().await;
        
        // Remove old votes
        state.votes.retain(|hash, _| state.pending_blocks.contains_key(hash));
        
        // Remove old transactions
        while state.pending_transactions.len() > 10000 {
            state.pending_transactions.pop_front();
        }
        
        Ok(())
    }

    fn validate_transaction(&self, transaction: &Transaction) -> bool {
        // Implement transaction validation logic
        true
    }

    fn validate_block(&self, block: &Block) -> bool {
        // Implement block validation logic
        true
    }

    fn is_valid_validator(&self, validator: &str, state: &ConsensusState) -> bool {
        if let Some(info) = state.validators.get(validator) {
            info.stake >= self.config.validator_stake_threshold
        } else {
            false
        }
    }

    fn check_consensus(&self, votes: usize, total_validators: usize) -> bool {
        votes * 3 > total_validators * 2 // 2/3 majority
    }

    fn select_proposer(&self, state: &ConsensusState) -> Result<String, NetworkError> {
        // Select proposer based on stake and last proposed time
        let total_stake: u64 = state.validators.values()
            .map(|v| v.stake)
            .sum();
            
        let mut proposer_value = state.current_round as u128;
        proposer_value *= total_stake as u128;
        proposer_value %= state.validators.len() as u128;
        
        for (address, info) in &state.validators {
            if proposer_value < info.stake as u128 {
                return Ok(address.clone());
            }
            proposer_value -= info.stake as u128;
        }
        
        Err(NetworkError::ConsensusError("No valid proposer found".into()))
    }

    fn create_block(&self, state: &mut ConsensusState, proposer: &str) -> Result<Block, NetworkError> {
        let mut transactions = Vec::new();
        let mut size = 0;
        
        // Collect transactions up to max block size
        while let Some(tx) = state.pending_transactions.front() {
            let tx_size = bincode::serialize(tx)
                .map_err(|_| NetworkError::ConsensusError("Serialization failed".into()))?
                .len();
                
            if size + tx_size > self.config.max_block_size {
                break;
            }
            
            transactions.push(state.pending_transactions.pop_front().unwrap());
            size += tx_size;
        }
        
        // Create block
        let block = Block {
            header: BlockHeader {
                parent_hash: self.get_parent_hash(state),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                number: state.current_height + 1,
                author: proposer.to_string(),
                transactions_root: self.compute_transactions_root(&transactions),
                state_root: Hash::default(), // Would be computed by state transition
                receipts_root: Hash::default(), // Would be computed from receipts
            },
            transactions,
            state_root: Hash::default(),
            receipts_root: Hash::default(),
        };
        
        Ok(block)
    }

    fn get_parent_hash(&self, state: &ConsensusState) -> Hash {
        state.finalized_blocks
            .get(&state.current_height)
            .map(|b| b.hash())
            .unwrap_or_default()
    }

    fn compute_transactions_root(&self, transactions: &[Transaction]) -> Hash {
        let mut hasher = blake3::Hasher::new();
        for tx in transactions {
            let tx_bytes = bincode::serialize(tx).unwrap();
            hasher.update(&tx_bytes);
        }
        hasher.finalize()
    }

    pub async fn get_status(&self) -> ConsensusStatus {
        let state = self.state.read().await;
        ConsensusStatus {
            height: state.current_height,
            round: state.current_round,
            pending_transactions: state.pending_transactions.len(),
            pending_blocks: state.pending_blocks.len(),
            active_validators: state.validators.len(),
            last_finalized_time: state.last_finalized_time.elapsed(),
        }
    }
}

impl ConsensusState {
    fn new() -> Self {
        Self {
            current_round: 0,
            current_height: 0,
            validators: HashMap::new(),
            pending_transactions: VecDeque::new(),
            pending_blocks: HashMap::new(),
            finalized_blocks: HashMap::new(),
            votes: HashMap::new(),
            last_finalized_time: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_consensus_flow() {
        let config = ConsensusConfig::default();
        let consensus = ConsensusEngine::new(config);

        // Add validator
        {
            let mut state = consensus.state.write().await;
            state.validators.insert("validator1".into(), ValidatorInfo {
                address: "validator1".into(),
                stake: 1000,
                last_proposed: 0,
                total_proposed: 0,
                total_validated: 0,
                uptime: 1.0,
            });
        }

        // Process transaction
        let tx = Transaction {
            nonce: 0,
            from: "sender".into(),
            to: "receiver".into(),
            value: 100,
            data: vec![],
            signature: None,
        };
        consensus.process_transaction(tx).await.unwrap();

        // Process round
        consensus.process_round().await.unwrap();

        // Check status
        let status = consensus.get_status().await;
        assert_eq!(status.round, 1);
    }

    #[tokio::test]
    async fn test_validator_selection() {
        let config = ConsensusConfig::default();
        let consensus = ConsensusEngine::new(config);

        // Add validators
        {
            let mut state = consensus.state.write().await;
            for i in 1..=3 {
                state.validators.insert(format!("validator{}", i), ValidatorInfo {
                    address: format!("validator{}", i),
                    stake: 1000,
                    last_proposed: 0,
                    total_proposed: 0,
                    total_validated: 0,
                    uptime: 1.0,
                });
            }
        }

        // Check proposer selection
        let state = consensus.state.read().await;
        let proposer = consensus.select_proposer(&state).unwrap();
        assert!(proposer.starts_with("validator"));
    }
}
