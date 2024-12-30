pub mod circuit;
pub mod vm;
pub mod proof;

use std::sync::Arc;
use parking_lot::RwLock;
use ff::PrimeField;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ZKVMError {
    #[error("VM Error: {0}")]
    VMError(#[from] vm::VMError),
    #[error("Proof Error: {0}")]
    ProofError(Box<dyn std::error::Error>),
    #[error("Circuit Error: {0}")]
    CircuitError(Box<dyn std::error::Error>),
    #[error("State Error: {0}")]
    StateError(String),
}

pub struct ZKVM<F: PrimeField> {
    vm: vm::VM,
    proof_system: Arc<proof::ProofSystem<F>>,
    circuit: Option<circuit::VMCircuit<F>>,
    state: Arc<RwLock<VMState>>,
}

pub struct VMState {
    pub gas_used: u64,
    pub execution_trace: Vec<ExecutionStep>,
    pub state_root: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct ExecutionStep {
    pub opcode: u8,
    pub stack_snapshot: Vec<vm::Value>,
    pub memory_snapshot: std::collections::HashMap<usize, vm::Value>,
    pub gas_cost: u64,
}

impl Default for VMState {
    fn default() -> Self {
        Self {
            gas_used: 0,
            execution_trace: Vec::new(),
            state_root: [0; 32],
        }
    }
}

impl<F: PrimeField> ZKVM<F> {
    pub fn new(program: Vec<u8>) -> Result<Self, ZKVMError> {
        let vm = vm::VM::new(program.clone());
        let circuit = circuit::VMCircuit::new(program.clone(), 1000);
        let proof_system = proof::ProofSystem::setup(circuit.clone())
            .map_err(|e| ZKVMError::ProofError(e))?;
        
        Ok(Self {
            vm,
            proof_system: Arc::new(proof_system),
            circuit: Some(circuit),
            state: Arc::new(RwLock::new(VMState::default())),
        })
    }

    pub fn execute(&mut self) -> Result<(), ZKVMError> {
        // Execute VM
        self.vm.execute()?;
        
        // Update state
        let mut state = self.state.write();
        state.gas_used = self.vm.get_gas_remaining();
        state.state_root = self.vm.get_state_root();
        
        // Record execution trace
        let step = ExecutionStep {
            opcode: 0, // Get from current instruction
            stack_snapshot: self.vm.get_stack(),
            memory_snapshot: self.vm.get_memory(),
            gas_cost: 0, // Get from gas calculation
        };
        state.execution_trace.push(step);
        
        Ok(())
    }

    pub fn generate_proof(&mut self) -> Result<proof::ProofData<F>, ZKVMError> {
        // Create circuit with current state
        let circuit = self.circuit.take()
            .ok_or_else(|| ZKVMError::StateError("Circuit already consumed".to_string()))?;
            
        // Generate proof
        self.proof_system.prove(circuit)
            .map_err(|e| ZKVMError::ProofError(e))
    }

    pub fn verify_proof(&self, proof_data: &proof::ProofData<F>) -> Result<bool, ZKVMError> {
        self.proof_system.verify(proof_data)
            .map_err(|e| ZKVMError::ProofError(e))
    }

    pub fn batch_verify(&self, proofs: &[proof::ProofData<F>]) -> Result<bool, ZKVMError> {
        self.proof_system.batch_verify(proofs)
            .map_err(|e| ZKVMError::ProofError(e))
    }

    pub fn get_execution_trace(&self) -> Vec<ExecutionStep> {
        self.state.read().execution_trace.clone()
    }

    pub fn get_state_root(&self) -> [u8; 32] {
        self.state.read().state_root
    }

    pub fn get_gas_used(&self) -> u64 {
        self.state.read().gas_used
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff::Field;

    fn create_test_program() -> Vec<u8> {
        vec![
            0x01, 0x05, // PUSH 5
            0x01, 0x03, // PUSH 3
            0x02,       // ADD
            0x04, 0x00, // STORE at address 0
            0x05, 0x00, // LOAD from address 0
            0xFF,       // STOP
        ]
    }

    #[test]
    fn test_vm_execution() {
        let program = create_test_program();
        let mut vm = vm::VM::new(program);
        assert!(vm.execute().is_ok());
        
        let stack = vm.get_stack();
        assert!(!stack.is_empty());
        
        if let vm::Value::Int(result) = &stack[0] {
            assert_eq!(*result, 8);
        } else {
            panic!("Expected integer result");
        }
    }

    #[test]
    fn test_proof_generation_and_verification() {
        // Add test implementation
    }

    #[test]
    fn test_batch_verification() {
        // Add test implementation
    }

    #[test]
    fn test_gas_accounting() {
        // Add test implementation
    }

    #[test]
    fn test_execution_trace() {
        // Add test implementation
    }
}
