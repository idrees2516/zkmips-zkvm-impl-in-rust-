use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, RwLock},
};
use futures::StreamExt;
use libp2p::{
    core::transport::Transport,
    identity, noise, tcp, yamux,
    PeerId, Swarm,
};
use thiserror::Error;

mod message;
mod peer;
mod sync;
mod consensus;

pub use message::{Message, MessageType};
pub use peer::{Peer, PeerInfo, PeerStatus};
pub use sync::{StateSync, SyncStatus};
pub use consensus::{ConsensusEngine, ConsensusConfig};

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Message error: {0}")]
    MessageError(String),
    #[error("Peer error: {0}")]
    PeerError(String),
    #[error("Sync error: {0}")]
    SyncError(String),
    #[error("Consensus error: {0}")]
    ConsensusError(String),
}

pub type NetworkResult<T> = Result<T, NetworkError>;

pub struct NetworkManager {
    swarm: Swarm<NetworkBehaviour>,
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    state_sync: Arc<StateSync>,
    consensus: Arc<ConsensusEngine>,
    message_tx: mpsc::Sender<Message>,
    message_rx: mpsc::Receiver<Message>,
    config: NetworkConfig,
}

#[derive(Clone)]
pub struct NetworkConfig {
    pub listen_addr: SocketAddr,
    pub bootstrap_peers: Vec<String>,
    pub max_peers: usize,
    pub ping_interval: Duration,
    pub sync_batch_size: usize,
    pub consensus_config: ConsensusConfig,
}

impl NetworkManager {
    pub async fn new(config: NetworkConfig) -> NetworkResult<Self> {
        let (message_tx, message_rx) = mpsc::channel(1000);
        
        // Create identity keypair
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        
        // Create transport
        let transport = libp2p::development_transport(local_key.clone()).await?;
        
        // Create network behaviour
        let behaviour = NetworkBehaviour::new(
            local_peer_id,
            message_tx.clone(),
            config.clone(),
        ).await?;
        
        // Create swarm
        let mut swarm = Swarm::new(transport, behaviour, local_peer_id);
        
        // Listen on configured address
        swarm.listen_on(config.listen_addr.into())?;
        
        // Create state sync and consensus
        let state_sync = Arc::new(StateSync::new(config.sync_batch_size));
        let consensus = Arc::new(ConsensusEngine::new(config.consensus_config.clone()));
        
        Ok(Self {
            swarm,
            peers: Arc::new(RwLock::new(HashMap::new())),
            state_sync,
            consensus,
            message_tx,
            message_rx,
            config,
        })
    }

    pub async fn start(&mut self) -> NetworkResult<()> {
        // Connect to bootstrap peers
        for addr in &self.config.bootstrap_peers {
            if let Ok(mut addr) = addr.parse() {
                self.swarm.dial(addr)?;
            }
        }
        
        // Start main event loop
        loop {
            tokio::select! {
                event = self.swarm.next() => {
                    match event {
                        Some(event) => self.handle_swarm_event(event).await?,
                        None => break,
                    }
                }
                msg = self.message_rx.recv() => {
                    match msg {
                        Some(msg) => self.handle_message(msg).await?,
                        None => break,
                    }
                }
            }
        }
        
        Ok(())
    }

    async fn handle_swarm_event(&mut self, event: SwarmEvent) -> NetworkResult<()> {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Listening on {:?}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                self.handle_peer_connected(peer_id).await?;
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                self.handle_peer_disconnected(peer_id).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> NetworkResult<()> {
        match message.message_type {
            MessageType::Block(block) => {
                self.consensus.process_block(block).await?;
            }
            MessageType::Transaction(tx) => {
                self.consensus.process_transaction(tx).await?;
            }
            MessageType::StateRequest(request) => {
                self.state_sync.handle_request(request).await?;
            }
            MessageType::StateResponse(response) => {
                self.state_sync.handle_response(response).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_peer_connected(&mut self, peer_id: PeerId) -> NetworkResult<()> {
        let mut peers = self.peers.write().await;
        if peers.len() >= self.config.max_peers {
            return Err(NetworkError::PeerError("Max peers reached".into()));
        }
        
        let peer_info = PeerInfo {
            id: peer_id,
            addr: None,
            status: PeerStatus::Connected,
            last_seen: Instant::now(),
        };
        peers.insert(peer_id, peer_info);
        
        // Start sync process with new peer
        self.state_sync.start_sync(peer_id).await?;
        
        Ok(())
    }

    async fn handle_peer_disconnected(&mut self, peer_id: PeerId) -> NetworkResult<()> {
        let mut peers = self.peers.write().await;
        peers.remove(&peer_id);
        
        // Clean up any sync state for disconnected peer
        self.state_sync.handle_peer_disconnected(peer_id).await?;
        
        Ok(())
    }

    pub async fn broadcast(&mut self, message: Message) -> NetworkResult<()> {
        let peers = self.peers.read().await;
        for peer_id in peers.keys() {
            self.swarm.behaviour_mut().send_message(*peer_id, message.clone())?;
        }
        Ok(())
    }

    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        self.peers.read().await.values().cloned().collect()
    }

    pub async fn get_sync_status(&self) -> SyncStatus {
        self.state_sync.get_status().await
    }

    pub async fn get_consensus_status(&self) -> ConsensusStatus {
        self.consensus.get_status().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_network_connection() {
        let config1 = NetworkConfig {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_peers: vec![],
            max_peers: 50,
            ping_interval: Duration::from_secs(30),
            sync_batch_size: 1000,
            consensus_config: ConsensusConfig::default(),
        };
        
        let config2 = NetworkConfig {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_peers: vec![],
            max_peers: 50,
            ping_interval: Duration::from_secs(30),
            sync_batch_size: 1000,
            consensus_config: ConsensusConfig::default(),
        };

        let mut node1 = NetworkManager::new(config1).await.unwrap();
        let mut node2 = NetworkManager::new(config2).await.unwrap();

        // Get node1's address and add it to node2's bootstrap peers
        let node1_addr = node1.swarm.listeners().next().unwrap();
        node2.config.bootstrap_peers.push(node1_addr.to_string());

        // Start both nodes
        let node1_handle = tokio::spawn(async move {
            node1.start().await.unwrap();
        });

        let node2_handle = tokio::spawn(async move {
            node2.start().await.unwrap();
        });

        // Wait for connection
        timeout(Duration::from_secs(5), async {
            loop {
                if node1.get_peers().await.len() > 0 && node2.get_peers().await.len() > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap();

        // Cleanup
        node1_handle.abort();
        node2_handle.abort();
    }
}
