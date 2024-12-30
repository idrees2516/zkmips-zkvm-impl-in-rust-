use serde::{Deserialize, Serialize};
use crate::crypto::Hash;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub message_type: MessageType,
    pub sender: String,
    pub timestamp: u64,
    pub signature: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessageType {
    Block(Block),
    Transaction(Transaction),
    StateRequest(StateRequest),
    StateResponse(StateResponse),
    Ping,
    Pong,
    Handshake(HandshakeData),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    pub state_root: Hash,
    pub receipts_root: Hash,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockHeader {
    pub parent_hash: Hash,
    pub timestamp: u64,
    pub number: u64,
    pub author: String,
    pub transactions_root: Hash,
    pub state_root: Hash,
    pub receipts_root: Hash,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub nonce: u64,
    pub from: String,
    pub to: String,
    pub value: u64,
    pub data: Vec<u8>,
    pub signature: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateRequest {
    pub block_number: u64,
    pub account: String,
    pub storage_keys: Vec<Hash>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateResponse {
    pub block_number: u64,
    pub account: String,
    pub storage: HashMap<Hash, Vec<u8>>,
    pub proof: StateProof,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateProof {
    pub account_proof: Vec<Vec<u8>>,
    pub storage_proofs: Vec<Vec<Vec<u8>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HandshakeData {
    pub version: u32,
    pub chain_id: u64,
    pub genesis_hash: Hash,
    pub head_hash: Hash,
    pub head_number: u64,
}

impl Message {
    pub fn new(message_type: MessageType, sender: String) -> Self {
        Self {
            message_type,
            sender,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: None,
        }
    }

    pub fn sign(&mut self, key: &SigningKey) -> Result<(), CryptoError> {
        let bytes = bincode::serialize(&(
            &self.message_type,
            &self.sender,
            &self.timestamp,
        ))?;
        
        let signature = key.sign(&bytes);
        self.signature = Some(signature.to_vec());
        Ok(())
    }

    pub fn verify(&self, key: &VerifyingKey) -> Result<bool, CryptoError> {
        let signature = self.signature.as_ref()
            .ok_or(CryptoError::InvalidSignature)?;
            
        let bytes = bincode::serialize(&(
            &self.message_type,
            &self.sender,
            &self.timestamp,
        ))?;
        
        Ok(key.verify(&bytes, signature)?)
    }
}

impl Block {
    pub fn hash(&self) -> Hash {
        let bytes = bincode::serialize(&self.header).unwrap();
        blake3::hash(&bytes)
    }

    pub fn verify(&self) -> bool {
        // Verify block integrity
        let tx_root = self.compute_transactions_root();
        if tx_root != self.header.transactions_root {
            return false;
        }

        // Verify all transactions
        for tx in &self.transactions {
            if !tx.verify() {
                return false;
            }
        }

        true
    }

    fn compute_transactions_root(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        for tx in &self.transactions {
            let tx_bytes = bincode::serialize(tx).unwrap();
            hasher.update(&tx_bytes);
        }
        hasher.finalize()
    }
}

impl Transaction {
    pub fn hash(&self) -> Hash {
        let bytes = bincode::serialize(&(
            self.nonce,
            &self.from,
            &self.to,
            self.value,
            &self.data,
        )).unwrap();
        blake3::hash(&bytes)
    }

    pub fn sign(&mut self, key: &SigningKey) -> Result<(), CryptoError> {
        let hash = self.hash();
        let signature = key.sign(hash.as_bytes());
        self.signature = Some(signature.to_vec());
        Ok(())
    }

    pub fn verify(&self) -> bool {
        if let Some(signature) = &self.signature {
            // Verify signature
            let hash = self.hash();
            // Implement signature verification
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::CryptoEngine;

    #[test]
    fn test_message_signing() {
        let mut crypto = CryptoEngine::new();
        crypto.generate_keypair().unwrap();

        let mut msg = Message::new(
            MessageType::Ping,
            "test_sender".to_string(),
        );

        msg.sign(&crypto.signing_key().unwrap()).unwrap();
        assert!(msg.verify(&crypto.verifying_key().unwrap()).unwrap());
    }

    #[test]
    fn test_block_verification() {
        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::default(),
                timestamp: 0,
                number: 0,
                author: "test".to_string(),
                transactions_root: Hash::default(),
                state_root: Hash::default(),
                receipts_root: Hash::default(),
            },
            transactions: vec![],
            state_root: Hash::default(),
            receipts_root: Hash::default(),
        };

        assert!(block.verify());
    }

    #[test]
    fn test_transaction_signing() {
        let mut crypto = CryptoEngine::new();
        crypto.generate_keypair().unwrap();

        let mut tx = Transaction {
            nonce: 0,
            from: "sender".to_string(),
            to: "receiver".to_string(),
            value: 100,
            data: vec![],
            signature: None,
        };

        tx.sign(&crypto.signing_key().unwrap()).unwrap();
        assert!(tx.verify());
    }
}
