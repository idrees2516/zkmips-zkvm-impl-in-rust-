use zkvm::{ZKVM, vm::Value};
use bellman::PrimeField;
use ff::Field;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    env_logger::init();

    // Example program: Compute (5 + 3) * 2
    let program = vec![
        0x01, 0x05, // PUSH 5
        0x01, 0x03, // PUSH 3
        0x02,       // ADD
        0x01, 0x02, // PUSH 2
        0x03,       // MUL
        0x04, 0x00, // STORE result at address 0
        0xFF,       // STOP
    ];

    // Create and execute VM
    let mut zkvm = ZKVM::new(program)?;
    zkvm.execute()?;

    // Generate proof
    let proof_data = zkvm.generate_proof()?;

    // Verify proof
    let is_valid = zkvm.verify_proof(&proof_data)?;
    println!("Proof verification result: {}", is_valid);

    Ok(())
}
