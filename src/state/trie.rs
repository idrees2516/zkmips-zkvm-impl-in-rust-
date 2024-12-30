use std::collections::HashMap;
use blake3::Hash;
use patricia_trie::{TrieMut, Trie, TrieDB, TrieDBMut};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TrieError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Invalid node: {0}")]
    InvalidNode(String),
    #[error("Invalid proof: {0}")]
    InvalidProof(String),
}

pub struct MerklePatriciaTrie {
    db: TrieDB,
    root: Option<Hash>,
}

impl MerklePatriciaTrie {
    pub fn new() -> Self {
        Self {
            db: TrieDB::new(),
            root: None,
        }
    }

    pub fn from_root(root: Hash) -> Result<Self, TrieError> {
        let mut trie = Self::new();
        trie.root = Some(root);
        Ok(trie)
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, TrieError> {
        if let Some(root) = self.root {
            let trie = TrieDB::new_with_root(&self.db, root)
                .map_err(|e| TrieError::DatabaseError(e.to_string()))?;
            trie.get(key)
                .map_err(|e| TrieError::DatabaseError(e.to_string()))
        } else {
            Ok(None)
        }
    }

    pub fn insert(&mut self, key: &[u8], value: &[u8]) -> Result<(), TrieError> {
        let mut trie = TrieDBMut::new(&mut self.db);
        trie.insert(key, value)
            .map_err(|e| TrieError::DatabaseError(e.to_string()))?;
        self.root = Some(trie.root());
        Ok(())
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<bool, TrieError> {
        let mut trie = TrieDBMut::new(&mut self.db);
        let result = trie.remove(key)
            .map_err(|e| TrieError::DatabaseError(e.to_string()))?;
        self.root = Some(trie.root());
        Ok(result)
    }

    pub fn root_hash(&self) -> Result<Hash, TrieError> {
        self.root.ok_or_else(|| TrieError::InvalidNode("No root hash".into()))
    }

    pub fn get_proof(&self, key: &[u8]) -> Result<Vec<Vec<u8>>, TrieError> {
        if let Some(root) = self.root {
            let trie = TrieDB::new_with_root(&self.db, root)
                .map_err(|e| TrieError::DatabaseError(e.to_string()))?;
            trie.get_proof(key)
                .map_err(|e| TrieError::DatabaseError(e.to_string()))
        } else {
            Ok(Vec::new())
        }
    }

    pub fn verify_proof(&self, key: &[u8], proof: &[Vec<u8>]) -> Result<bool, TrieError> {
        if let Some(root) = self.root {
            let trie = TrieDB::new_with_root(&self.db, root)
                .map_err(|e| TrieError::DatabaseError(e.to_string()))?;
            trie.verify_proof(key, proof)
                .map_err(|e| TrieError::InvalidProof(e.to_string()))
        } else {
            Ok(false)
        }
    }

    pub fn iter(&self) -> Result<impl Iterator<Item = Result<(Vec<u8>, Vec<u8>), TrieError>>, TrieError> {
        if let Some(root) = self.root {
            let trie = TrieDB::new_with_root(&self.db, root)
                .map_err(|e| TrieError::DatabaseError(e.to_string()))?;
            Ok(trie.iter()
                .map(|result| result.map_err(|e| TrieError::DatabaseError(e.to_string()))))
        } else {
            Ok(std::iter::empty())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trie_operations() {
        let mut trie = MerklePatriciaTrie::new();

        // Insert
        trie.insert(b"key1", b"value1").unwrap();
        trie.insert(b"key2", b"value2").unwrap();

        // Get
        assert_eq!(trie.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(trie.get(b"key2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(trie.get(b"key3").unwrap(), None);

        // Delete
        assert!(trie.delete(b"key1").unwrap());
        assert_eq!(trie.get(b"key1").unwrap(), None);

        // Proof
        let proof = trie.get_proof(b"key2").unwrap();
        assert!(trie.verify_proof(b"key2", &proof).unwrap());
    }

    #[test]
    fn test_trie_iteration() {
        let mut trie = MerklePatriciaTrie::new();

        // Insert multiple items
        let items = vec![
            (b"key1".to_vec(), b"value1".to_vec()),
            (b"key2".to_vec(), b"value2".to_vec()),
            (b"key3".to_vec(), b"value3".to_vec()),
        ];

        for (key, value) in &items {
            trie.insert(key, value).unwrap();
        }

        // Collect all items through iterator
        let mut collected: Vec<_> = trie.iter()
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        collected.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(collected, items);
    }
}
