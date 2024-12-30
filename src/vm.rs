use std::collections::{HashMap, VecDeque};
use thiserror::Error;
use parking_lot::RwLock;
use std::sync::Arc;
use blake2::{Blake2b512, Digest};
use rayon::prelude::*;

#[derive(Error, Debug)]
pub enum VMError {
    #[error("Stack underflow")]
    StackUnderflow,
    #[error("Stack overflow")]
    StackOverflow,
    #[error("Invalid opcode: {0}")]
    InvalidOpcode(u8),
    #[error("Memory access error: {0}")]
    MemoryError(String),
    #[error("Execution error: {0}")]
    ExecutionError(String),
    #[error("Gas limit exceeded")]
    GasLimitExceeded,
    #[error("Invalid jump destination")]
    InvalidJumpDestination,
    #[error("Contract creation error: {0}")]
    ContractCreationError(String),
    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Bytes(Vec<u8>),
    Address([u8; 32]),
    Contract(ContractData),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContractData {
    pub code: Vec<u8>,
    pub storage: HashMap<[u8; 32], Value>,
    pub balance: u64,
}

#[derive(Clone, Debug)]
pub struct GasConfig {
    pub base: u64,
    pub op_cost: HashMap<u8, u64>,
    pub memory_expansion: u64,
    pub contract_creation: u64,
}

impl Default for GasConfig {
    fn default() -> Self {
        let mut op_cost = HashMap::new();
        op_cost.insert(0x01, 3);  // PUSH
        op_cost.insert(0x02, 5);  // ADD
        op_cost.insert(0x03, 5);  // MUL
        op_cost.insert(0x04, 20); // STORE
        op_cost.insert(0x05, 20); // LOAD
        op_cost.insert(0x06, 8);  // JUMP
        op_cost.insert(0x07, 10); // JUMPI
        op_cost.insert(0x08, 3);  // EQ
        op_cost.insert(0x09, 3);  // LT
        op_cost.insert(0x0A, 3);  // GT
        op_cost.insert(0x0B, 400); // CREATE
        op_cost.insert(0x0C, 40);  // CALL
        op_cost.insert(0x0D, 5);   // RETURN
        op_cost.insert(0x0E, 50);  // SHA3
        op_cost.insert(0x0F, 20);  // BALANCE

        Self {
            base: 2,
            op_cost,
            memory_expansion: 3,
            contract_creation: 32000,
        }
    }
}

pub struct ExecutionContext {
    stack: Vec<Value>,
    memory: HashMap<usize, Value>,
    storage: HashMap<[u8; 32], Value>,
    program_counter: usize,
    gas_remaining: u64,
    gas_config: GasConfig,
    call_stack: VecDeque<CallFrame>,
    state_root: [u8; 32],
    logs: Vec<Log>,
}

#[derive(Clone, Debug)]
pub struct CallFrame {
    pub caller: [u8; 32],
    pub address: [u8; 32],
    pub value: u64,
    pub gas_limit: u64,
    pub code: Vec<u8>,
    pub return_data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Log {
    pub address: [u8; 32],
    pub topics: Vec<[u8; 32]>,
    pub data: Vec<u8>,
}

impl ExecutionContext {
    pub fn new(gas_limit: u64) -> Self {
        Self {
            stack: Vec::with_capacity(1024),
            memory: HashMap::new(),
            storage: HashMap::new(),
            program_counter: 0,
            gas_remaining: gas_limit,
            gas_config: GasConfig::default(),
            call_stack: VecDeque::new(),
            state_root: [0; 32],
            logs: Vec::new(),
        }
    }

    fn use_gas(&mut self, amount: u64) -> Result<(), VMError> {
        if self.gas_remaining < amount {
            return Err(VMError::GasLimitExceeded);
        }
        self.gas_remaining -= amount;
        Ok(())
    }

    fn compute_state_root(&mut self) {
        let mut hasher = Blake2b512::new();
        
        // Hash storage
        let mut storage_vec: Vec<_> = self.storage.iter().collect();
        storage_vec.par_sort_by_key(|&(k, _)| k);
        
        for (key, value) in storage_vec {
            hasher.update(key);
            match value {
                Value::Int(i) => hasher.update(&i.to_le_bytes()),
                Value::Bool(b) => hasher.update(&[*b as u8]),
                Value::Bytes(b) => hasher.update(b),
                Value::Address(a) => hasher.update(a),
                Value::Contract(c) => {
                    hasher.update(&c.code);
                    hasher.update(&c.balance.to_le_bytes());
                }
            }
        }

        // Hash logs
        for log in &self.logs {
            hasher.update(&log.address);
            for topic in &log.topics {
                hasher.update(topic);
            }
            hasher.update(&log.data);
        }

        let result = hasher.finalize();
        self.state_root.copy_from_slice(&result[..32]);
    }
}

pub struct VM {
    context: Arc<RwLock<ExecutionContext>>,
    program: Vec<u8>,
}

impl VM {
    pub fn new(program: Vec<u8>) -> Self {
        Self {
            context: Arc::new(RwLock::new(ExecutionContext::new(1_000_000))),
            program,
        }
    }

    pub fn execute(&self) -> Result<(), VMError> {
        let mut context = self.context.write();
        
        while context.program_counter < self.program.len() {
            let opcode = self.program[context.program_counter];
            
            // Use gas for operation
            let gas_cost = context.gas_config.op_cost.get(&opcode)
                .copied()
                .unwrap_or(context.gas_config.base);
            context.use_gas(gas_cost)?;

            match opcode {
                // Existing opcodes
                0x01 => { // PUSH
                    let value = self.program[context.program_counter + 1];
                    if context.stack.len() >= 1024 {
                        return Err(VMError::StackOverflow);
                    }
                    context.stack.push(Value::Int(value as i64));
                    context.program_counter += 2;
                }
                0x02 => { // ADD
                    let b = match context.stack.pop() {
                        Some(Value::Int(v)) => v,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let a = match context.stack.pop() {
                        Some(Value::Int(v)) => v,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    context.stack.push(Value::Int(a + b));
                    context.program_counter += 1;
                }
                0x03 => { // MUL
                    let b = match context.stack.pop() {
                        Some(Value::Int(v)) => v,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let a = match context.stack.pop() {
                        Some(Value::Int(v)) => v,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    context.stack.push(Value::Int(a * b));
                    context.program_counter += 1;
                }
                0x04 => { // STORE
                    let addr = match context.stack.pop() {
                        Some(Value::Int(v)) => v as usize,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let value = context.stack.pop()
                        .ok_or(VMError::StackUnderflow)?;
                    context.memory.insert(addr, value);
                    context.program_counter += 1;
                }
                0x05 => { // LOAD
                    let addr = match context.stack.pop() {
                        Some(Value::Int(v)) => v as usize,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let value = context.memory.get(&addr)
                        .ok_or_else(|| VMError::MemoryError(format!("Address not found: {}", addr)))?
                        .clone();
                    context.stack.push(value);
                    context.program_counter += 1;
                }
                // New advanced opcodes
                0x0B => { // CREATE
                    let value = match context.stack.pop() {
                        Some(Value::Int(v)) => v as u64,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let code_size = match context.stack.pop() {
                        Some(Value::Int(v)) => v as usize,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    
                    context.use_gas(context.gas_config.contract_creation)?;
                    
                    let code: Vec<u8> = self.program[context.program_counter + 1..
                                                   context.program_counter + 1 + code_size]
                        .to_vec();
                    
                    let contract = ContractData {
                        code,
                        storage: HashMap::new(),
                        balance: value,
                    };
                    
                    let mut hasher = Blake2b512::new();
                    hasher.update(&contract.code);
                    let mut address = [0u8; 32];
                    address.copy_from_slice(&hasher.finalize()[..32]);
                    
                    context.stack.push(Value::Address(address));
                    context.stack.push(Value::Contract(contract));
                    
                    context.program_counter += 1 + code_size;
                }
                0x0C => { // CALL
                    let address = match context.stack.pop() {
                        Some(Value::Address(addr)) => addr,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let value = match context.stack.pop() {
                        Some(Value::Int(v)) => v as u64,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let gas_limit = match context.stack.pop() {
                        Some(Value::Int(v)) => v as u64,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    
                    let contract = match context.memory.get(&(address[0] as usize)) {
                        Some(Value::Contract(c)) => c.clone(),
                        _ => return Err(VMError::ExecutionError("Contract not found".to_string())),
                    };
                    
                    let caller = [0u8; 32]; // Current context address
                    let frame = CallFrame {
                        caller,
                        address,
                        value,
                        gas_limit,
                        code: contract.code,
                        return_data: Vec::new(),
                    };
                    
                    context.call_stack.push_back(frame);
                    context.program_counter += 1;
                }
                0x0D => { // RETURN
                    let size = match context.stack.pop() {
                        Some(Value::Int(v)) => v as usize,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let offset = match context.stack.pop() {
                        Some(Value::Int(v)) => v as usize,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    
                    if let Some(frame) = context.call_stack.back_mut() {
                        frame.return_data = self.program[offset..offset + size].to_vec();
                    }
                    
                    context.program_counter += 1;
                }
                0x0E => { // SHA3
                    let size = match context.stack.pop() {
                        Some(Value::Int(v)) => v as usize,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    let offset = match context.stack.pop() {
                        Some(Value::Int(v)) => v as usize,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    
                    let mut hasher = Blake2b512::new();
                    hasher.update(&self.program[offset..offset + size]);
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&hasher.finalize()[..32]);
                    
                    context.stack.push(Value::Bytes(hash.to_vec()));
                    context.program_counter += 1;
                }
                0x0F => { // BALANCE
                    let address = match context.stack.pop() {
                        Some(Value::Address(addr)) => addr,
                        _ => return Err(VMError::StackUnderflow),
                    };
                    
                    let balance = match context.memory.get(&(address[0] as usize)) {
                        Some(Value::Contract(c)) => c.balance,
                        _ => return Err(VMError::ExecutionError("Contract not found".to_string())),
                    };
                    
                    context.stack.push(Value::Int(balance as i64));
                    context.program_counter += 1;
                }
                0xFF => break, // STOP
                _ => return Err(VMError::InvalidOpcode(opcode)),
            }
        }

        // Compute final state root
        context.compute_state_root();
        Ok(())
    }

    pub fn get_stack(&self) -> Vec<Value> {
        self.context.read().stack.clone()
    }

    pub fn get_memory(&self) -> HashMap<usize, Value> {
        self.context.read().memory.clone()
    }

    pub fn get_storage(&self) -> HashMap<[u8; 32], Value> {
        self.context.read().storage.clone()
    }

    pub fn get_state_root(&self) -> [u8; 32] {
        self.context.read().state_root
    }

    pub fn get_logs(&self) -> Vec<Log> {
        self.context.read().logs.clone()
    }

    pub fn get_gas_remaining(&self) -> u64 {
        self.context.read().gas_remaining
    }
}
