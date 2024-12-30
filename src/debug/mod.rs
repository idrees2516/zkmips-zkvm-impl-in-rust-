use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use crate::{
    vm::{VM, Value, VMError},
    memory::{MemoryManager, MemoryAddress},
    network::NetworkManager,
};

#[derive(Debug)]
pub struct Debugger {
    vm: Arc<RwLock<VM>>,
    memory: Arc<RwLock<MemoryManager>>,
    network: Arc<RwLock<NetworkManager>>,
    breakpoints: HashMap<usize, Breakpoint>,
    call_stack: Vec<StackFrame>,
    execution_trace: Vec<TraceEntry>,
    profiling_data: ProfilingData,
}

#[derive(Clone, Debug)]
pub struct Breakpoint {
    pub address: usize,
    pub condition: Option<String>,
    pub hit_count: usize,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct StackFrame {
    pub function_name: String,
    pub pc: usize,
    pub locals: HashMap<String, Value>,
    pub stack: Vec<Value>,
    pub memory: HashMap<MemoryAddress, Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct TraceEntry {
    pub timestamp: Instant,
    pub opcode: u8,
    pub pc: usize,
    pub stack_snapshot: Vec<Value>,
    pub memory_snapshot: HashMap<MemoryAddress, Vec<u8>>,
    pub gas_used: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ProfilingData {
    pub opcode_stats: HashMap<u8, OpcodeStats>,
    pub memory_stats: MemoryStats,
    pub gas_stats: GasStats,
}

#[derive(Clone, Debug, Default)]
pub struct OpcodeStats {
    pub count: usize,
    pub total_gas: u64,
    pub total_time: Duration,
    pub avg_stack_depth: f64,
}

#[derive(Clone, Debug, Default)]
pub struct MemoryStats {
    pub total_allocations: usize,
    pub total_deallocations: usize,
    pub peak_memory: usize,
    pub current_memory: usize,
}

#[derive(Clone, Debug, Default)]
pub struct GasStats {
    pub total_gas_used: u64,
    pub gas_per_second: f64,
    pub peak_gas_rate: f64,
}

impl Debugger {
    pub fn new(
        vm: Arc<RwLock<VM>>,
        memory: Arc<RwLock<MemoryManager>>,
        network: Arc<RwLock<NetworkManager>>,
    ) -> Self {
        Self {
            vm,
            memory,
            network,
            breakpoints: HashMap::new(),
            call_stack: Vec::new(),
            execution_trace: Vec::new(),
            profiling_data: ProfilingData::default(),
        }
    }

    pub async fn step(&mut self) -> Result<(), VMError> {
        let start = Instant::now();
        
        // Execute single instruction
        let mut vm = self.vm.write().await;
        let result = vm.step();
        
        // Update profiling data
        self.update_profiling(vm.current_opcode(), start.elapsed()).await;
        
        // Record trace
        self.record_trace(&vm).await;
        
        result
    }

    pub async fn continue_execution(&mut self) -> Result<(), VMError> {
        loop {
            let pc = self.vm.read().await.pc();
            
            if let Some(breakpoint) = self.breakpoints.get_mut(&pc) {
                if breakpoint.enabled {
                    breakpoint.hit_count += 1;
                    if self.check_breakpoint_condition(breakpoint).await {
                        break;
                    }
                }
            }
            
            self.step().await?;
        }
        Ok(())
    }

    pub fn add_breakpoint(&mut self, address: usize, condition: Option<String>) {
        self.breakpoints.insert(address, Breakpoint {
            address,
            condition,
            hit_count: 0,
            enabled: true,
        });
    }

    pub fn remove_breakpoint(&mut self, address: usize) {
        self.breakpoints.remove(&address);
    }

    pub async fn get_stack_trace(&self) -> Vec<StackFrame> {
        let vm = self.vm.read().await;
        let mut frames = Vec::new();
        
        for frame in &self.call_stack {
            frames.push(frame.clone());
        }
        
        frames
    }

    pub async fn get_local_variables(&self) -> HashMap<String, Value> {
        let vm = self.vm.read().await;
        if let Some(frame) = self.call_stack.last() {
            frame.locals.clone()
        } else {
            HashMap::new()
        }
    }

    pub async fn inspect_memory(&self, address: MemoryAddress, size: usize) -> Result<Vec<u8>, VMError> {
        let memory = self.memory.read().await;
        memory.read(address, size)
    }

    pub async fn get_execution_trace(&self, start: usize, end: usize) -> Vec<TraceEntry> {
        self.execution_trace[start.min(self.execution_trace.len())..end.min(self.execution_trace.len())].to_vec()
    }

    pub async fn get_profiling_data(&self) -> ProfilingData {
        self.profiling_data.clone()
    }

    async fn update_profiling(&mut self, opcode: u8, duration: Duration) {
        let stats = self.profiling_data.opcode_stats.entry(opcode).or_default();
        stats.count += 1;
        stats.total_time += duration;
        
        let vm = self.vm.read().await;
        stats.total_gas += vm.last_gas_cost();
        stats.avg_stack_depth = (stats.avg_stack_depth * (stats.count - 1) as f64 + vm.stack_depth() as f64) / stats.count as f64;
    }

    async fn record_trace(&mut self, vm: &VM) {
        let entry = TraceEntry {
            timestamp: Instant::now(),
            opcode: vm.current_opcode(),
            pc: vm.pc(),
            stack_snapshot: vm.get_stack(),
            memory_snapshot: vm.get_memory(),
            gas_used: vm.get_gas_used(),
        };
        
        self.execution_trace.push(entry);
        
        // Limit trace size
        if self.execution_trace.len() > 10000 {
            self.execution_trace.remove(0);
        }
    }

    async fn check_breakpoint_condition(&self, breakpoint: &Breakpoint) -> bool {
        if let Some(condition) = &breakpoint.condition {
            // Implement condition evaluation
            true
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_debugger() {
        // Create test VM and components
        let vm = Arc::new(RwLock::new(VM::new(vec![
            0x01, 0x05, // PUSH 5
            0x01, 0x03, // PUSH 3
            0x02,       // ADD
            0xFF,       // STOP
        ])));
        
        let memory = Arc::new(RwLock::new(MemoryManager::new(MemoryConfig {
            page_size: 4096,
            gc_threshold: 1024 * 1024,
            cache_size: 1000,
        })));
        
        let network = Arc::new(RwLock::new(NetworkManager::new(NetworkConfig {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_peers: vec![],
            max_peers: 50,
            ping_interval: Duration::from_secs(30),
            sync_batch_size: 1000,
            consensus_config: ConsensusConfig::default(),
        }).await.unwrap()));
        
        let mut debugger = Debugger::new(vm, memory, network);
        
        // Add breakpoint
        debugger.add_breakpoint(2, None); // Break at ADD instruction
        
        // Run until breakpoint
        debugger.continue_execution().await.unwrap();
        
        // Check stack
        let stack_trace = debugger.get_stack_trace().await;
        assert!(!stack_trace.is_empty());
        
        // Step over ADD instruction
        debugger.step().await.unwrap();
        
        // Check profiling data
        let profiling = debugger.get_profiling_data().await;
        assert!(profiling.opcode_stats.contains_key(&0x02)); // ADD opcode
    }
}
