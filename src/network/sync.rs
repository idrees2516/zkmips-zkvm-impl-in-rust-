use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use libp2p::PeerId;
use futures::StreamExt;
use crate::{
    crypto::Hash,
    network::{Message, MessageType, NetworkError},
};

#[derive(Debug)]
pub struct StateSync {
    sync_state: Arc<RwLock<SyncState>>,
    batch_size: usize,
    timeout: Duration,
    retry_limit: u32,
}

#[derive(Debug)]
struct SyncState {
    status: SyncStatus,
    pending_requests: HashMap<PeerId, Vec<StateRequest>>,
    completed_requests: HashSet<StateRequest>,
    failed_requests: HashMap<StateRequest, u32>,
    sync_queue: VecDeque<StateRequest>,
    last_progress: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SyncStatus {
    Idle,
    Syncing {
        current_block: u64,
        target_block: u64,
        peers: usize,
    },
    Error(String),
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
struct StateRequest {
    block_number: u64,
    account: String,
    storage_keys: Vec<Hash>,
    timestamp: Instant,
}

impl StateSync {
    pub fn new(batch_size: usize) -> Self {
        Self {
            sync_state: Arc::new(RwLock::new(SyncState::new())),
            batch_size,
            timeout: Duration::from_secs(30),
            retry_limit: 3,
        }
    }

    pub async fn start_sync(&self, peer_id: PeerId) -> Result<(), NetworkError> {
        let mut state = self.sync_state.write().await;
        
        // Initialize sync state if not already syncing
        if let SyncStatus::Idle = state.status {
            state.status = SyncStatus::Syncing {
                current_block: 0,
                target_block: 0,
                peers: 1,
            };
            state.last_progress = Instant::now();
        }
        
        // Add peer to pending requests
        state.pending_requests.entry(peer_id).or_default();
        
        Ok(())
    }

    pub async fn handle_request(&self, request: StateRequest) -> Result<(), NetworkError> {
        let mut state = self.sync_state.write().await;
        
        // Add request to sync queue
        state.sync_queue.push_back(request);
        
        // Process sync queue
        self.process_sync_queue(&mut state).await?;
        
        Ok(())
    }

    pub async fn handle_response(&self, response: StateResponse) -> Result<(), NetworkError> {
        let mut state = self.sync_state.write().await;
        
        // Mark request as completed
        let request = StateRequest {
            block_number: response.block_number,
            account: response.account,
            storage_keys: response.storage_keys,
            timestamp: Instant::now(),
        };
        
        state.completed_requests.insert(request.clone());
        
        // Remove from pending requests
        for requests in state.pending_requests.values_mut() {
            requests.retain(|r| r != &request);
        }
        
        // Update sync status
        if let SyncStatus::Syncing { current_block, target_block, peers } = &mut state.status {
            if response.block_number > *current_block {
                *current_block = response.block_number;
                state.last_progress = Instant::now();
            }
        }
        
        // Process sync queue
        self.process_sync_queue(&mut state).await?;
        
        Ok(())
    }

    pub async fn handle_peer_disconnected(&self, peer_id: PeerId) -> Result<(), NetworkError> {
        let mut state = self.sync_state.write().await;
        
        // Move peer's pending requests back to sync queue
        if let Some(requests) = state.pending_requests.remove(&peer_id) {
            for request in requests {
                state.sync_queue.push_back(request);
            }
        }
        
        // Update sync status
        if let SyncStatus::Syncing { peers, .. } = &mut state.status {
            *peers = (*peers).saturating_sub(1);
        }
        
        Ok(())
    }

    async fn process_sync_queue(&self, state: &mut SyncState) -> Result<(), NetworkError> {
        // Check for timed out requests
        self.check_timeouts(state).await?;
        
        // Process requests in queue
        while let Some(request) = state.sync_queue.pop_front() {
            if state.completed_requests.contains(&request) {
                continue;
            }
            
            // Check retry limit
            let retry_count = state.failed_requests.get(&request).copied().unwrap_or(0);
            if retry_count >= self.retry_limit {
                return Err(NetworkError::SyncError(format!(
                    "Request failed after {} retries", retry_count
                )));
            }
            
            // Find available peer
            if let Some((peer_id, requests)) = state.pending_requests
                .iter_mut()
                .find(|(_, requests)| requests.len() < self.batch_size)
            {
                requests.push(request.clone());
                // Send request to peer
                // This would be implemented by the network layer
            } else {
                // No available peers, put request back in queue
                state.sync_queue.push_back(request);
                break;
            }
        }
        
        // Check if sync is complete
        if state.sync_queue.is_empty() && state.pending_requests.values().all(|r| r.is_empty()) {
            state.status = SyncStatus::Idle;
        }
        
        Ok(())
    }

    async fn check_timeouts(&self, state: &mut SyncState) -> Result<(), NetworkError> {
        let now = Instant::now();
        
        // Check for timed out requests
        for (peer_id, requests) in state.pending_requests.iter_mut() {
            let timed_out: Vec<_> = requests.iter()
                .filter(|r| now.duration_since(r.timestamp) > self.timeout)
                .cloned()
                .collect();
                
            for request in timed_out {
                // Increment retry counter
                *state.failed_requests.entry(request.clone()).or_default() += 1;
                
                // Remove from pending and add back to queue
                requests.retain(|r| r != &request);
                state.sync_queue.push_back(request);
            }
        }
        
        // Check overall sync timeout
        if let SyncStatus::Syncing { .. } = state.status {
            if now.duration_since(state.last_progress) > Duration::from_secs(300) {
                state.status = SyncStatus::Error("Sync timed out".into());
            }
        }
        
        Ok(())
    }

    pub async fn get_status(&self) -> SyncStatus {
        self.sync_state.read().await.status.clone()
    }
}

impl SyncState {
    fn new() -> Self {
        Self {
            status: SyncStatus::Idle,
            pending_requests: HashMap::new(),
            completed_requests: HashSet::new(),
            failed_requests: HashMap::new(),
            sync_queue: VecDeque::new(),
            last_progress: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sync_flow() {
        let sync = StateSync::new(10);
        let peer_id = PeerId::random();

        // Start sync
        sync.start_sync(peer_id).await.unwrap();
        assert!(matches!(sync.get_status().await, SyncStatus::Syncing { .. }));

        // Add request
        let request = StateRequest {
            block_number: 1,
            account: "test".into(),
            storage_keys: vec![],
            timestamp: Instant::now(),
        };
        sync.handle_request(request.clone()).await.unwrap();

        // Handle response
        let response = StateResponse {
            block_number: 1,
            account: "test".into(),
            storage_keys: vec![],
            storage: HashMap::new(),
            proof: StateProof {
                account_proof: vec![],
                storage_proofs: vec![],
            },
        };
        sync.handle_response(response).await.unwrap();

        // Check completion
        assert_eq!(sync.get_status().await, SyncStatus::Idle);
    }

    #[tokio::test]
    async fn test_sync_timeout() {
        let sync = StateSync::new(10);
        let peer_id = PeerId::random();

        // Start sync with artificially old last_progress
        sync.start_sync(peer_id).await.unwrap();
        {
            let mut state = sync.sync_state.write().await;
            state.last_progress = Instant::now() - Duration::from_secs(301);
        }

        // Add request and check timeout
        let request = StateRequest {
            block_number: 1,
            account: "test".into(),
            storage_keys: vec![],
            timestamp: Instant::now() - Duration::from_secs(31),
        };
        sync.handle_request(request).await.unwrap();

        // Status should be error
        assert!(matches!(sync.get_status().await, SyncStatus::Error(_)));
    }
}
