use std::time::Instant;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub id: PeerId,
    pub addr: Option<std::net::SocketAddr>,
    pub status: PeerStatus,
    pub last_seen: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PeerStatus {
    Connected,
    Disconnected,
    Banned,
    Syncing,
}

#[derive(Clone, Debug)]
pub struct Peer {
    pub info: PeerInfo,
    pub message_tx: mpsc::Sender<Vec<u8>>,
    pub capabilities: PeerCapabilities,
    pub reputation: i32,
    pub connection_time: Instant,
    pub ping_stats: PingStats,
}

#[derive(Clone, Debug, Default)]
pub struct PeerCapabilities {
    pub protocols: Vec<String>,
    pub version: u32,
    pub chain_id: u64,
    pub head_block: u64,
    pub total_difficulty: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PingStats {
    pub last_ping: Option<Instant>,
    pub last_pong: Option<Instant>,
    pub min_latency: Option<u64>,
    pub max_latency: Option<u64>,
    pub avg_latency: Option<u64>,
    pub ping_count: u64,
}

impl Peer {
    pub fn new(
        id: PeerId,
        addr: Option<std::net::SocketAddr>,
        message_tx: mpsc::Sender<Vec<u8>>,
    ) -> Self {
        Self {
            info: PeerInfo {
                id,
                addr,
                status: PeerStatus::Connected,
                last_seen: Instant::now(),
            },
            message_tx,
            capabilities: PeerCapabilities::default(),
            reputation: 0,
            connection_time: Instant::now(),
            ping_stats: PingStats::default(),
        }
    }

    pub async fn send_message(&self, message: Vec<u8>) -> Result<(), mpsc::error::SendError<Vec<u8>>> {
        self.message_tx.send(message).await
    }

    pub fn update_ping(&mut self) {
        self.ping_stats.last_ping = Some(Instant::now());
        self.ping_stats.ping_count += 1;
    }

    pub fn update_pong(&mut self) {
        let now = Instant::now();
        if let Some(ping_time) = self.ping_stats.last_ping {
            let latency = now.duration_since(ping_time).as_millis() as u64;
            
            // Update min latency
            self.ping_stats.min_latency = Some(match self.ping_stats.min_latency {
                Some(min) => std::cmp::min(min, latency),
                None => latency,
            });
            
            // Update max latency
            self.ping_stats.max_latency = Some(match self.ping_stats.max_latency {
                Some(max) => std::cmp::max(max, latency),
                None => latency,
            });
            
            // Update average latency
            self.ping_stats.avg_latency = Some(match self.ping_stats.avg_latency {
                Some(avg) => (avg * (self.ping_stats.ping_count - 1) + latency) / self.ping_stats.ping_count,
                None => latency,
            });
        }
        
        self.ping_stats.last_pong = Some(now);
    }

    pub fn update_reputation(&mut self, delta: i32) {
        self.reputation = self.reputation.saturating_add(delta);
        
        // Ban peer if reputation drops too low
        if self.reputation < -100 {
            self.info.status = PeerStatus::Banned;
        }
    }

    pub fn update_capabilities(&mut self, capabilities: PeerCapabilities) {
        self.capabilities = capabilities;
    }

    pub fn is_synced(&self) -> bool {
        self.info.status == PeerStatus::Connected &&
        self.capabilities.head_block > 0
    }

    pub fn is_banned(&self) -> bool {
        self.info.status == PeerStatus::Banned
    }

    pub fn connection_duration(&self) -> std::time::Duration {
        Instant::now().duration_since(self.connection_time)
    }

    pub fn last_seen_duration(&self) -> std::time::Duration {
        Instant::now().duration_since(self.info.last_seen)
    }
}

#[derive(Clone, Debug)]
pub struct PeerManager {
    peers: HashMap<PeerId, Peer>,
    banned_peers: HashSet<PeerId>,
    max_peers: usize,
}

impl PeerManager {
    pub fn new(max_peers: usize) -> Self {
        Self {
            peers: HashMap::new(),
            banned_peers: HashSet::new(),
            max_peers,
        }
    }

    pub fn add_peer(&mut self, peer: Peer) -> Result<(), &'static str> {
        if self.peers.len() >= self.max_peers {
            return Err("Max peers reached");
        }
        
        if self.banned_peers.contains(&peer.info.id) {
            return Err("Peer is banned");
        }
        
        self.peers.insert(peer.info.id, peer);
        Ok(())
    }

    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
    }

    pub fn ban_peer(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.info.status = PeerStatus::Banned;
        }
        self.banned_peers.insert(*peer_id);
        self.remove_peer(peer_id);
    }

    pub fn unban_peer(&mut self, peer_id: &PeerId) {
        self.banned_peers.remove(peer_id);
    }

    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&Peer> {
        self.peers.get(peer_id)
    }

    pub fn get_peer_mut(&mut self, peer_id: &PeerId) -> Option<&mut Peer> {
        self.peers.get_mut(peer_id)
    }

    pub fn get_peers(&self) -> Vec<&Peer> {
        self.peers.values().collect()
    }

    pub fn get_synced_peers(&self) -> Vec<&Peer> {
        self.peers.values()
            .filter(|p| p.is_synced())
            .collect()
    }

    pub fn update_peer_status(&mut self, peer_id: &PeerId, status: PeerStatus) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.info.status = status;
        }
    }

    pub fn cleanup_disconnected_peers(&mut self) {
        self.peers.retain(|_, peer| {
            peer.info.status != PeerStatus::Disconnected
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_peer_reputation() {
        let (tx, _) = mpsc::channel(100);
        let mut peer = Peer::new(PeerId::random(), None, tx);
        
        peer.update_reputation(50);
        assert_eq!(peer.reputation, 50);
        
        peer.update_reputation(-150);
        assert_eq!(peer.reputation, -100);
        assert!(peer.is_banned());
    }

    #[test]
    fn test_peer_manager() {
        let mut manager = PeerManager::new(2);
        let (tx1, _) = mpsc::channel(100);
        let (tx2, _) = mpsc::channel(100);
        let (tx3, _) = mpsc::channel(100);
        
        let peer1 = Peer::new(PeerId::random(), None, tx1);
        let peer2 = Peer::new(PeerId::random(), None, tx2);
        let peer3 = Peer::new(PeerId::random(), None, tx3);
        
        assert!(manager.add_peer(peer1).is_ok());
        assert!(manager.add_peer(peer2).is_ok());
        assert!(manager.add_peer(peer3).is_err()); // Max peers reached
        
        assert_eq!(manager.get_peers().len(), 2);
    }
}
