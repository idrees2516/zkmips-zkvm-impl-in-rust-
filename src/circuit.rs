use bellman::{
    Circuit, ConstraintSystem, LinearCombination, SynthesisError, Variable,
    groth16::{Proof, VerifyingKey},
};
use ff::{Field, PrimeField};
use std::marker::PhantomData;
use blake2::{Blake2b512, Digest};
use rayon::prelude::*;

#[derive(Clone)]
pub struct VMState<F: PrimeField> {
    pub stack: Vec<F>,
    pub memory: Vec<F>,
    pub storage: Vec<(F, F)>,
    pub program_counter: F,
    pub gas_remaining: F,
}

#[derive(Clone)]
pub struct VMCircuit<F: PrimeField> {
    pub initial_state: Option<VMState<F>>,
    pub final_state: Option<VMState<F>>,
    pub program: Vec<u8>,
    pub max_steps: usize,
    _marker: PhantomData<F>,
}

impl<F: PrimeField> VMCircuit<F> {
    pub fn new(program: Vec<u8>, max_steps: usize) -> Self {
        Self {
            initial_state: None,
            final_state: None,
            program,
            max_steps,
            _marker: PhantomData,
        }
    }

    pub fn with_witness(
        program: Vec<u8>,
        max_steps: usize,
        initial_state: VMState<F>,
        final_state: VMState<F>,
    ) -> Self {
        Self {
            initial_state: Some(initial_state),
            final_state: Some(final_state),
            program,
            max_steps,
            _marker: PhantomData,
        }
    }

    fn alloc_state<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        state: &Option<VMState<F>>,
        prefix: &str,
    ) -> Result<AllocatedState<F>, SynthesisError> {
        let stack = if let Some(state) = state {
            state.stack.iter().enumerate().map(|(i, &value)| {
                cs.alloc(
                    || format!("{}_{}_stack_{}", prefix, i, value),
                    || Ok(value),
                )
            }).collect::<Result<Vec<_>, _>>()?
        } else {
            vec![]
        };

        let memory = if let Some(state) = state {
            state.memory.iter().enumerate().map(|(i, &value)| {
                cs.alloc(
                    || format!("{}_{}_memory_{}", prefix, i, value),
                    || Ok(value),
                )
            }).collect::<Result<Vec<_>, _>>()?
        } else {
            vec![]
        };

        let storage = if let Some(state) = state {
            state.storage.iter().map(|&(key, value)| {
                Ok((
                    cs.alloc(
                        || format!("{}_storage_key_{}", prefix, key),
                        || Ok(key),
                    )?,
                    cs.alloc(
                        || format!("{}_storage_value_{}", prefix, value),
                        || Ok(value),
                    )?,
                ))
            }).collect::<Result<Vec<_>, _>>()?
        } else {
            vec![]
        };

        let program_counter = cs.alloc(
            || format!("{}_pc", prefix),
            || {
                state.as_ref()
                    .map(|s| s.program_counter)
                    .ok_or(SynthesisError::AssignmentMissing)
            },
        )?;

        let gas_remaining = cs.alloc(
            || format!("{}_gas", prefix),
            || {
                state.as_ref()
                    .map(|s| s.gas_remaining)
                    .ok_or(SynthesisError::AssignmentMissing)
            },
        )?;

        Ok(AllocatedState {
            stack,
            memory,
            storage,
            program_counter,
            gas_remaining,
        })
    }
}

#[derive(Clone)]
struct AllocatedState<F: PrimeField> {
    stack: Vec<Variable>,
    memory: Vec<Variable>,
    storage: Vec<(Variable, Variable)>,
    program_counter: Variable,
    gas_remaining: Variable,
}

impl<F: PrimeField> Circuit<F> for VMCircuit<F> {
    fn synthesize<CS: ConstraintSystem<F>>(self, cs: &mut CS) -> Result<(), SynthesisError> {
        // Allocate initial and final states
        let initial_state = self.alloc_state(cs, &self.initial_state, "initial")?;
        let final_state = self.alloc_state(cs, &self.final_state, "final")?;

        // Enforce constraints for each step of execution
        let mut current_state = initial_state.clone();
        
        for step in 0..self.max_steps {
            let cs = &mut cs.namespace(|| format!("step_{}", step));
            
            // Get current opcode
            let pc_value = self.initial_state.as_ref().map(|s| s.program_counter);
            let opcode = if let Some(pc) = pc_value {
                let pc_usize = pc.to_repr().as_ref()[0] as usize;
                if pc_usize < self.program.len() {
                    Some(F::from(self.program[pc_usize] as u64))
                } else {
                    None
                }
            } else {
                None
            };

            let opcode_var = cs.alloc(
                || format!("opcode_{}", step),
                || opcode.ok_or(SynthesisError::AssignmentMissing),
            )?;

            // Enforce state transition based on opcode
            match self.program.get(step) {
                Some(&0x01) => { // PUSH
                    // Enforce stack push operation
                    let value = cs.alloc(
                        || format!("push_value_{}", step),
                        || {
                            self.initial_state
                                .as_ref()
                                .map(|s| F::from(self.program[step + 1] as u64))
                                .ok_or(SynthesisError::AssignmentMissing)
                        },
                    )?;
                    
                    current_state.stack.push(value);
                    
                    // Update program counter
                    cs.enforce(
                        || format!("pc_advance_{}", step),
                        |lc| lc + current_state.program_counter,
                        |lc| lc + CS::one(),
                        |lc| lc + final_state.program_counter,
                    );
                }
                Some(&0x02) => { // ADD
                    if current_state.stack.len() >= 2 {
                        let a = current_state.stack.pop().unwrap();
                        let b = current_state.stack.pop().unwrap();
                        
                        let result = cs.alloc(
                            || format!("add_result_{}", step),
                            || {
                                if let Some(state) = &self.initial_state {
                                    let a_val = state.stack[state.stack.len() - 1];
                                    let b_val = state.stack[state.stack.len() - 2];
                                    Ok(a_val + b_val)
                                } else {
                                    Err(SynthesisError::AssignmentMissing)
                                }
                            },
                        )?;
                        
                        // Enforce a + b = result
                        cs.enforce(
                            || format!("add_{}", step),
                            |lc| lc + a,
                            |lc| lc + b,
                            |lc| lc + result,
                        );
                        
                        current_state.stack.push(result);
                    }
                }
                Some(&0x03) => { // MUL
                    if current_state.stack.len() >= 2 {
                        let a = current_state.stack.pop().unwrap();
                        let b = current_state.stack.pop().unwrap();
                        
                        let result = cs.alloc(
                            || format!("mul_result_{}", step),
                            || {
                                if let Some(state) = &self.initial_state {
                                    let a_val = state.stack[state.stack.len() - 1];
                                    let b_val = state.stack[state.stack.len() - 2];
                                    Ok(a_val * b_val)
                                } else {
                                    Err(SynthesisError::AssignmentMissing)
                                }
                            },
                        )?;
                        
                        // Enforce a * b = result
                        cs.enforce(
                            || format!("mul_{}", step),
                            |lc| lc + a,
                            |lc| lc + b,
                            |lc| lc + result,
                        );
                        
                        current_state.stack.push(result);
                    }
                }
                Some(&0x04) => { // STORE
                    if current_state.stack.len() >= 2 {
                        let value = current_state.stack.pop().unwrap();
                        let addr = current_state.stack.pop().unwrap();
                        
                        // Extend memory if needed
                        while current_state.memory.len() <= step {
                            let zero = cs.alloc(
                                || format!("memory_zero_{}", current_state.memory.len()),
                                || Ok(F::zero()),
                            )?;
                            current_state.memory.push(zero);
                        }
                        
                        // Store value at address
                        current_state.memory[step] = value;
                        
                        // Enforce memory update
                        cs.enforce(
                            || format!("store_{}", step),
                            |lc| lc + addr,
                            |lc| lc + CS::one(),
                            |lc| lc + value,
                        );
                    }
                }
                Some(&0x0E) => { // SHA3
                    if current_state.stack.len() >= 2 {
                        let size = current_state.stack.pop().unwrap();
                        let offset = current_state.stack.pop().unwrap();
                        
                        // Compute hash of memory range
                        let mut hasher = Blake2b512::new();
                        if let Some(state) = &self.initial_state {
                            let offset_val = state.stack[state.stack.len() - 2];
                            let size_val = state.stack[state.stack.len() - 1];
                            let memory_slice = &state.memory[
                                offset_val.to_repr().as_ref()[0] as usize..
                                (offset_val + size_val).to_repr().as_ref()[0] as usize
                            ];
                            for &value in memory_slice {
                                hasher.update(&value.to_repr());
                            }
                        }
                        
                        let hash_result = cs.alloc(
                            || format!("sha3_result_{}", step),
                            || {
                                if let Some(_) = &self.initial_state {
                                    let result = hasher.finalize();
                                    Ok(F::from_repr(result[..32].try_into().unwrap())
                                        .unwrap_or(F::zero()))
                                } else {
                                    Err(SynthesisError::AssignmentMissing)
                                }
                            },
                        )?;
                        
                        current_state.stack.push(hash_result);
                    }
                }
                _ => {}
            }
        }

        // Final state constraints
        cs.enforce(
            || "final_pc",
            |lc| lc + current_state.program_counter,
            |lc| lc + CS::one(),
            |lc| lc + final_state.program_counter,
        );

        cs.enforce(
            || "final_gas",
            |lc| lc + current_state.gas_remaining,
            |lc| lc + CS::one(),
            |lc| lc + final_state.gas_remaining,
        );

        // Stack constraints
        for (i, (current, final_var)) in current_state.stack.iter()
            .zip(final_state.stack.iter())
            .enumerate()
        {
            cs.enforce(
                || format!("final_stack_{}", i),
                |lc| lc + *current,
                |lc| lc + CS::one(),
                |lc| lc + *final_var,
            );
        }

        // Memory constraints
        for (i, (current, final_var)) in current_state.memory.iter()
            .zip(final_state.memory.iter())
            .enumerate()
        {
            cs.enforce(
                || format!("final_memory_{}", i),
                |lc| lc + *current,
                |lc| lc + CS::one(),
                |lc| lc + *final_var,
            );
        }

        // Storage constraints
        for (i, ((current_key, current_val), (final_key, final_val))) in 
            current_state.storage.iter()
            .zip(final_state.storage.iter())
            .enumerate()
        {
            cs.enforce(
                || format!("final_storage_key_{}", i),
                |lc| lc + *current_key,
                |lc| lc + CS::one(),
                |lc| lc + *final_key,
            );

            cs.enforce(
                || format!("final_storage_value_{}", i),
                |lc| lc + *current_val,
                |lc| lc + CS::one(),
                |lc| lc + *final_val,
            );
        }

        Ok(())
    }
}
