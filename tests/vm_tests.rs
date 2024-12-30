use zkvm::{
    vm::{VM, Value, VMError},
    circuit::VMCircuit,
    proof::{ProofSystem, ProofData},
    ZKVM,
};
use bellman::groth16::*;
use ff::{Field, PrimeField};
use rand::thread_rng;

#[test]
fn test_basic_arithmetic() {
    let program = vec![
        0x01, 0x05, // PUSH 5
        0x01, 0x03, // PUSH 3
        0x02,       // ADD
        0x01, 0x02, // PUSH 2
        0x03,       // MUL
        0xFF,       // STOP
    ];

    let mut vm = VM::new(program);
    assert!(vm.execute().is_ok());

    let stack = vm.get_stack();
    assert_eq!(stack.len(), 1);
    
    if let Value::Int(result) = &stack[0] {
        assert_eq!(*result, 16); // (5 + 3) * 2 = 16
    } else {
        panic!("Expected integer result");
    }
}

#[test]
fn test_memory_operations() {
    let program = vec![
        0x01, 0x2A, // PUSH 42
        0x04, 0x00, // STORE at address 0
        0x01, 0x37, // PUSH 55
        0x04, 0x01, // STORE at address 1
        0x05, 0x00, // LOAD from address 0
        0x05, 0x01, // LOAD from address 1
        0x02,       // ADD
        0xFF,       // STOP
    ];

    let mut vm = VM::new(program);
    assert!(vm.execute().is_ok());

    let stack = vm.get_stack();
    assert_eq!(stack.len(), 1);
    
    if let Value::Int(result) = &stack[0] {
        assert_eq!(*result, 97); // 42 + 55 = 97
    } else {
        panic!("Expected integer result");
    }

    let memory = vm.get_memory();
    assert_eq!(memory.len(), 2);
    
    if let Value::Int(value) = &memory[&0] {
        assert_eq!(*value, 42);
    } else {
        panic!("Expected integer in memory[0]");
    }
    
    if let Value::Int(value) = &memory[&1] {
        assert_eq!(*value, 55);
    } else {
        panic!("Expected integer in memory[1]");
    }
}

#[test]
fn test_stack_underflow() {
    let program = vec![
        0x02, // ADD with empty stack
        0xFF,
    ];

    let mut vm = VM::new(program);
    match vm.execute() {
        Err(VMError::StackUnderflow) => (),
        _ => panic!("Expected stack underflow error"),
    }
}

#[test]
fn test_stack_overflow() {
    let mut program = Vec::new();
    // Push 1025 values (max stack size is 1024)
    for i in 0..1025 {
        program.extend_from_slice(&[0x01, i as u8]);
    }
    program.push(0xFF);

    let mut vm = VM::new(program);
    match vm.execute() {
        Err(VMError::StackOverflow) => (),
        _ => panic!("Expected stack overflow error"),
    }
}

#[test]
fn test_invalid_opcode() {
    let program = vec![
        0xFE, // Invalid opcode
        0xFF,
    ];

    let mut vm = VM::new(program);
    match vm.execute() {
        Err(VMError::InvalidOpcode(_)) => (),
        _ => panic!("Expected invalid opcode error"),
    }
}

#[test]
fn test_gas_accounting() {
    let program = vec![
        0x01, 0x05, // PUSH 5 (3 gas)
        0x01, 0x03, // PUSH 3 (3 gas)
        0x02,       // ADD (5 gas)
        0x04, 0x00, // STORE (20 gas)
        0xFF,       // STOP (0 gas)
    ];

    let mut vm = VM::new(program);
    assert!(vm.execute().is_ok());

    let gas_used = 1_000_000 - vm.get_gas_remaining();
    assert_eq!(gas_used, 31); // 3 + 3 + 5 + 20 = 31
}

#[test]
fn test_contract_creation() {
    let contract_code = vec![
        0x01, 0x05, // PUSH 5
        0x01, 0x03, // PUSH 3
        0x02,       // ADD
        0xFF,       // STOP
    ];

    let mut program = vec![
        0x01, contract_code.len() as u8, // PUSH code size
        0x01, 0x64,                      // PUSH 100 (initial balance)
        0x0B,                            // CREATE
    ];
    program.extend_from_slice(&contract_code);
    program.push(0xFF);                  // STOP

    let mut vm = VM::new(program);
    assert!(vm.execute().is_ok());

    let stack = vm.get_stack();
    assert_eq!(stack.len(), 2);
    
    match &stack[0] {
        Value::Contract(contract) => {
            assert_eq!(contract.code, contract_code);
            assert_eq!(contract.balance, 100);
        }
        _ => panic!("Expected contract on stack"),
    }
}

#[test]
fn test_contract_call() {
    let contract_code = vec![
        0x01, 0x05, // PUSH 5
        0x01, 0x03, // PUSH 3
        0x02,       // ADD
        0x0D,       // RETURN
        0xFF,       // STOP
    ];

    let mut program = vec![
        // First create the contract
        0x01, contract_code.len() as u8, // PUSH code size
        0x01, 0x64,                      // PUSH 100 (initial balance)
        0x0B,                            // CREATE
    ];
    program.extend_from_slice(&contract_code);
    
    // Then call it
    program.extend_from_slice(&[
        0x01, 0x0A,  // PUSH 10 (gas limit)
        0x01, 0x00,  // PUSH 0 (value to send)
        0x0C,        // CALL
        0xFF,        // STOP
    ]);

    let mut vm = VM::new(program);
    assert!(vm.execute().is_ok());

    // Check call frame return data
    let call_frames = vm.get_call_frames();
    assert_eq!(call_frames.len(), 1);
    assert_eq!(call_frames[0].return_data, vec![8]); // 5 + 3 = 8
}

#[test]
fn test_sha3_hash() {
    let program = vec![
        0x01, 0x05, // PUSH 5
        0x04, 0x00, // STORE at address 0
        0x01, 0x20, // PUSH 32 (size)
        0x01, 0x00, // PUSH 0 (offset)
        0x0E,       // SHA3
        0xFF,       // STOP
    ];

    let mut vm = VM::new(program);
    assert!(vm.execute().is_ok());

    let stack = vm.get_stack();
    assert_eq!(stack.len(), 1);
    
    match &stack[0] {
        Value::Bytes(hash) => {
            assert_eq!(hash.len(), 32);
            // Add specific hash value check
        }
        _ => panic!("Expected bytes (hash) on stack"),
    }
}

#[test]
fn test_state_root() {
    let program = vec![
        0x01, 0x05, // PUSH 5
        0x04, 0x00, // STORE at address 0
        0x01, 0x03, // PUSH 3
        0x04, 0x01, // STORE at address 1
        0xFF,       // STOP
    ];

    let mut vm = VM::new(program);
    assert!(vm.execute().is_ok());

    let state_root = vm.get_state_root();
    assert_ne!(state_root, [0; 32]);
    // Add specific state root check
}
