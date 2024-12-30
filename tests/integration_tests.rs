use zkvm::{ZKVM, vm::{Value, VMError}};
use proptest::prelude::*;

fn create_valid_program(operations: Vec<(u8, u8)>) -> Vec<u8> {
    let mut program = Vec::new();
    for (op, val) in operations {
        match op % 5 {
            0 => {
                // PUSH
                program.extend_from_slice(&[0x01, val]);
            }
            1 => {
                // ADD
                program.push(0x02);
            }
            2 => {
                // MUL
                program.push(0x03);
            }
            3 => {
                // STORE
                program.extend_from_slice(&[0x04, val % 16]);
            }
            4 => {
                // LOAD
                program.extend_from_slice(&[0x05, val % 16]);
            }
            _ => unreachable!(),
        }
    }
    program.push(0xFF); // STOP
    program
}

proptest! {
    #[test]
    fn test_random_valid_programs(
        operations in prop::collection::vec((0u8..5, 0u8..255), 1..50)
    ) {
        let program = create_valid_program(operations);
        let mut zkvm = ZKVM::new(program).unwrap();
        
        // Execute
        zkvm.execute().unwrap();
        
        // Generate and verify proof
        let proof_data = zkvm.generate_proof().unwrap();
        assert!(zkvm.verify_proof(&proof_data).unwrap());
    }
}

#[test]
fn test_stack_underflow() {
    let program = vec![
        0x02, // ADD with empty stack
        0xFF,
    ];
    
    let mut zkvm = ZKVM::new(program).unwrap();
    match zkvm.execute() {
        Err(VMError::StackUnderflow) => (),
        _ => panic!("Expected stack underflow error"),
    }
}

#[test]
fn test_invalid_opcode() {
    let program = vec![
        0xFE, // Invalid opcode
        0xFF,
    ];
    
    let mut zkvm = ZKVM::new(program).unwrap();
    match zkvm.execute() {
        Err(VMError::InvalidOpcode(_)) => (),
        _ => panic!("Expected invalid opcode error"),
    }
}

#[test]
fn test_memory_operations() {
    let program = vec![
        0x01, 0x42, // PUSH 66
        0x04, 0x00, // STORE at address 0
        0x05, 0x00, // LOAD from address 0
        0xFF,
    ];
    
    let mut zkvm = ZKVM::new(program).unwrap();
    zkvm.execute().unwrap();
    
    let stack = zkvm.vm.get_stack();
    assert_eq!(stack.len(), 1);
    
    if let Value::Int(value) = &stack[0] {
        assert_eq!(*value, 66);
    } else {
        panic!("Expected integer value");
    }
}

#[test]
fn test_arithmetic_operations() {
    let program = vec![
        0x01, 0x05, // PUSH 5
        0x01, 0x03, // PUSH 3
        0x02,       // ADD
        0x01, 0x02, // PUSH 2
        0x03,       // MUL
        0xFF,
    ];
    
    let mut zkvm = ZKVM::new(program).unwrap();
    zkvm.execute().unwrap();
    
    let stack = zkvm.vm.get_stack();
    assert_eq!(stack.len(), 1);
    
    if let Value::Int(value) = &stack[0] {
        assert_eq!(*value, 16); // (5 + 3) * 2 = 16
    } else {
        panic!("Expected integer value");
    }
}
