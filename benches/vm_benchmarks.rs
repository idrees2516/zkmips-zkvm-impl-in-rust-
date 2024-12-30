use criterion::{black_box, criterion_group, criterion_main, Criterion};
use zkvm::{ZKVM, vm::Value};

fn create_benchmark_program(size: usize) -> Vec<u8> {
    let mut program = Vec::with_capacity(size * 3);
    for i in 0..size {
        program.extend_from_slice(&[
            0x01, i as u8,  // PUSH i
            0x02,          // ADD
        ]);
    }
    program.push(0xFF);    // STOP
    program
}

fn bench_vm_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_execution");
    
    for size in [10, 100, 1000].iter() {
        group.bench_function(format!("execute_{}_ops", size), |b| {
            let program = create_benchmark_program(*size);
            b.iter(|| {
                let mut vm = ZKVM::new(black_box(program.clone())).unwrap();
                vm.execute().unwrap();
            });
        });
    }
    group.finish();
}

fn bench_proof_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("proof_generation");
    
    for size in [10, 100].iter() {
        group.bench_function(format!("prove_{}_ops", size), |b| {
            let program = create_benchmark_program(*size);
            let mut zkvm = ZKVM::new(program).unwrap();
            zkvm.execute().unwrap();
            b.iter(|| {
                zkvm.generate_proof().unwrap();
            });
        });
    }
    group.finish();
}

fn bench_proof_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("proof_verification");
    
    for size in [10, 100].iter() {
        group.bench_function(format!("verify_{}_ops", size), |b| {
            let program = create_benchmark_program(*size);
            let mut zkvm = ZKVM::new(program).unwrap();
            zkvm.execute().unwrap();
            let proof_data = zkvm.generate_proof().unwrap();
            b.iter(|| {
                zkvm.verify_proof(&proof_data).unwrap();
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_vm_execution,
    bench_proof_generation,
    bench_proof_verification
);
criterion_main!(benches);
