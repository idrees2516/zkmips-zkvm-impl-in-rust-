use zkvm::{
    vm::{VM, Value},
    circuit::VMCircuit,
    proof::{ProofSystem, ProofData, ProofSystemBuilder},
    ZKVM,
};
use bellman::groth16::*;
use ff::{Field, PrimeField};
use rand::thread_rng;
use std::time::Instant;

fn create_test_program() -> Vec<u8> {
    vec![
        0x01, 0x05, // PUSH 5
        0x01, 0x03, // PUSH 3
        0x02,       // ADD
        0x01, 0x02, // PUSH 2
        0x03,       // MUL
        0x04, 0x00, // STORE result
        0xFF,       // STOP
    ]
}

#[test]
fn test_proof_generation() {
    let program = create_test_program();
    let mut zkvm = ZKVM::new(program).unwrap();
    
    // Execute program
    assert!(zkvm.execute().is_ok());
    
    // Generate proof
    let proof_data = zkvm.generate_proof().unwrap();
    
    // Verify proof structure
    assert!(!proof_data.public_inputs.is_empty());
    assert_eq!(proof_data.hash.len(), 32);
}

#[test]
fn test_proof_verification() {
    let program = create_test_program();
    let mut zkvm = ZKVM::new(program).unwrap();
    
    // Execute and generate proof
    zkvm.execute().unwrap();
    let proof_data = zkvm.generate_proof().unwrap();
    
    // Verify proof
    assert!(zkvm.verify_proof(&proof_data).unwrap());
}

#[test]
fn test_invalid_proof_rejection() {
    let program = create_test_program();
    let mut zkvm = ZKVM::new(program).unwrap();
    
    // Execute and generate proof
    zkvm.execute().unwrap();
    let mut proof_data = zkvm.generate_proof().unwrap();
    
    // Tamper with the proof
    proof_data.public_inputs[0] = proof_data.public_inputs[0] + proof_data.public_inputs[0];
    
    // Verification should fail
    assert!(!zkvm.verify_proof(&proof_data).unwrap());
}

#[test]
fn test_batch_verification() {
    let program = create_test_program();
    let mut zkvm = ZKVM::new(program.clone()).unwrap();
    
    // Generate multiple proofs
    let mut proofs = Vec::new();
    for _ in 0..5 {
        zkvm.execute().unwrap();
        let proof_data = zkvm.generate_proof().unwrap();
        proofs.push(proof_data);
    }
    
    // Verify all proofs in batch
    assert!(zkvm.batch_verify(&proofs).unwrap());
}

#[test]
fn test_proof_caching() {
    let program = create_test_program();
    let mut zkvm = ZKVM::new(program).unwrap();
    
    // Execute and generate proof
    zkvm.execute().unwrap();
    let proof_data = zkvm.generate_proof().unwrap();
    
    // First verification
    let start = Instant::now();
    assert!(zkvm.verify_proof(&proof_data).unwrap());
    let first_duration = start.elapsed();
    
    // Second verification (should be faster due to caching)
    let start = Instant::now();
    assert!(zkvm.verify_proof(&proof_data).unwrap());
    let second_duration = start.elapsed();
    
    assert!(second_duration < first_duration);
}

#[test]
fn test_proof_system_builder() {
    let program = create_test_program();
    
    let proof_system = ProofSystemBuilder::new()
        .with_cache_size(2000)
        .with_parallel_verification(true)
        .with_verification_batch_size(50)
        .build(VMCircuit::new(program.clone(), 1000))
        .unwrap();
    
    // Test the configured proof system
    let mut zkvm = ZKVM::new(program).unwrap();
    zkvm.execute().unwrap();
    let proof_data = zkvm.generate_proof().unwrap();
    
    assert!(zkvm.verify_proof(&proof_data).unwrap());
}

#[test]
fn test_large_computation_proof() {
    // Create a program with many operations
    let mut program = Vec::new();
    for i in 0..100 {
        program.extend_from_slice(&[
            0x01, i as u8,     // PUSH i
            0x01, (i+1) as u8, // PUSH i+1
            0x02,              // ADD
            0x04, i as u8,     // STORE result
        ]);
    }
    program.push(0xFF); // STOP
    
    let mut zkvm = ZKVM::new(program).unwrap();
    
    // Execute and generate proof
    zkvm.execute().unwrap();
    let proof_data = zkvm.generate_proof().unwrap();
    
    // Verify proof
    assert!(zkvm.verify_proof(&proof_data).unwrap());
}

#[test]
fn test_concurrent_verification() {
    use rayon::prelude::*;
    
    let program = create_test_program();
    let mut zkvm = ZKVM::new(program.clone()).unwrap();
    
    // Generate multiple proofs
    let mut proofs = Vec::new();
    for _ in 0..10 {
        zkvm.execute().unwrap();
        let proof_data = zkvm.generate_proof().unwrap();
        proofs.push(proof_data);
    }
    
    // Verify proofs concurrently
    let results: Vec<bool> = proofs.par_iter()
        .map(|proof| zkvm.verify_proof(proof).unwrap())
        .collect();
    
    assert!(results.iter().all(|&x| x));
}

#[test]
fn test_proof_serialization() {
    use bincode::{serialize, deserialize};
    
    let program = create_test_program();
    let mut zkvm = ZKVM::new(program).unwrap();
    
    // Generate proof
    zkvm.execute().unwrap();
    let proof_data = zkvm.generate_proof().unwrap();
    
    // Serialize proof
    let serialized = serialize(&proof_data).unwrap();
    
    // Deserialize and verify
    let deserialized: ProofData = deserialize(&serialized).unwrap();
    assert!(zkvm.verify_proof(&deserialized).unwrap());
}
