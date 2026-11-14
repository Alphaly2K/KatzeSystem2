use katze_system2::{
    Step,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};

fn ins(code: &mut Vec<u8>, opcode: u8, arguments: &[u32]) {
    code.push(opcode);
    for value in arguments {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

fn put(vm: &mut Interpreter, address: u32, words: &[u32]) {
    let bytes: Vec<_> = words.iter().flat_map(|n| n.to_le_bytes()).collect();
    vm.write(address, &bytes).unwrap();
}

fn words(vm: &Interpreter, address: u32, count: usize) -> Vec<u32> {
    vm.read(address, count * 4)
        .unwrap()
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

fn call(host: &mut CoreHost<'_>, vm: &mut Interpreter, index: u32, args: &[u32]) -> NativeReturn {
    host.call(
        index,
        &args
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>(),
        vm,
    )
    .unwrap()
}

#[test]
fn ordinary_vectors_keep_alias_feedback_matrix_snapshots_and_native_stack_contracts() {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[0x1357_9bdf]);
    for (index, args, output) in [
        (355, vec![8], Some(0)),
        (356, vec![8], Some(4)),
        (352, vec![32], None),
        (353, vec![69, 65], None),
        (354, vec![101, 97], None),
        (357, vec![0x8000_0000, 0x8000_0004], None),
        (358, vec![0x8000_0020, 0x8000_0020], None),
        (350, vec![128], None),
        (350, vec![0x8000_0080], None),
        (351, vec![196, 192], None),
        (351, vec![0x8000_00c0, 196], None),
    ] {
        for value in &args {
            ins(&mut code, 0x25, &[*value]);
        }
        ins(&mut code, 0x25, &[u32::MAX]);
        ins(&mut code, 0x2b, &[index, (args.len() as u32 + 1) * 4]);
        if let Some(output) = output {
            ins(&mut code, 0x26, &[output]);
        }
    }
    code.push(0x3b);
    for budget in [1, 4096] {
        let mut vm = Interpreter::new(
            Program::from_code(&code, vec![0; 272], 64).unwrap(),
            vec![0; 256],
        )
        .unwrap();
        put(&mut vm, 8, &[3., 4., 0., 0.].map(f32::to_bits));
        put(&mut vm, 32, &[1., 0., 0., 0.].map(f32::to_bits));
        for address in [65, 97] {
            put(&mut vm, address, &[1., 2., 3., 4., 5.].map(f32::to_bits));
        }
        put(&mut vm, 0x8000_0000, &[1., 2., 3., 4.].map(f32::to_bits));
        put(
            &mut vm,
            0x8000_0020,
            &(1..=16).map(|n| (n as f32).to_bits()).collect::<Vec<_>>(),
        );
        let identity = std::array::from_fn::<_, 16, _>(|i| if i % 5 == 0 { 1.0 } else { 0.0 });
        put(&mut vm, 128, &identity.map(f32::to_bits));
        put(&mut vm, 0x8000_0080, &[1.0f32.to_bits(); 16]);
        put(&mut vm, 192, &(0..20).collect::<Vec<_>>());
        let mut state = State::new(0);
        let mut host = CoreHost::new(&mut state);
        for _ in 0..100 {
            if vm.run(&mut host, budget).unwrap() == Step::Halted {
                break;
            }
        }
        assert_eq!(vm.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
        assert_eq!(words(&vm, 0, 2), [5., 25.].map(f32::to_bits));
        assert_eq!(words(&vm, 32, 4), [0x3f7f_ffff, 0, 0, 0]);
        assert_eq!(words(&vm, 69, 4), [3., 6., 10., 15.].map(f32::to_bits));
        assert_eq!(words(&vm, 101, 4), [1., 2., 2., 3.].map(f32::to_bits));
        assert_eq!(
            words(&vm, 0x8000_0000, 4),
            [2., 4., 6., 8.].map(f32::to_bits)
        );
        assert_eq!(
            words(&vm, 0x8000_0020, 4),
            [90., 100., 110., 120.].map(f32::to_bits)
        );
        assert_eq!(words(&vm, 0x8000_0030, 1), [5f32.to_bits()]);
        assert_eq!(
            words(&vm, 128, 16),
            (0..16)
                .map(|i| if i % 5 == 0 { 0x3f7f_ffff } else { 0 })
                .collect::<Vec<_>>()
        );
        assert_eq!(words(&vm, 0x8000_0080, 16), [1.0f32.to_bits(); 16]);
        let copied = [0, 1, 2, 3, 3, 5, 6, 7, 7, 9, 10, 11, 11, 13, 14, 15];
        assert_eq!(words(&vm, 196, 16), copied);
        assert_eq!(words(&vm, 0x8000_00c0, 16), copied);
    }
}

#[test]
fn nonfinite_operands_and_invalid_ranges_keep_sse_payloads_and_prior_writes() {
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0; 128], 64).unwrap(),
        vec![0; 32],
    )
    .unwrap();
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    put(&mut vm, 0, &[0x7fc0_1234; 4]);
    put(&mut vm, 16, &[0x7fc0_5678; 4]);
    assert_eq!(
        call(&mut host, &mut vm, 353, &[0, 16]),
        NativeReturn::Continue
    );
    assert_eq!(
        words(&vm, 0, 4),
        [0x7fc0_1234, 0x7fc0_5678, 0x7fc0_5678, 0x7fc0_5678]
    );
    assert_eq!(
        call(&mut host, &mut vm, 356, &[0]),
        NativeReturn::Value(0x7fc0_1234)
    );
    assert_eq!(
        call(&mut host, &mut vm, 355, &[0]),
        NativeReturn::Value(0x7fc0_1234)
    );
    put(&mut vm, 32, &[0; 4]);
    call(&mut host, &mut vm, 352, &[32]);
    assert_eq!(words(&vm, 32, 4), [0xffc0_0000; 4]);
    let before = vm.read(0, 128).unwrap().to_vec();
    for (index, args) in [
        (350, vec![0x8000_0000]),
        (352, vec![0x8000_0014]),
        (357, vec![0, u32::MAX]),
        (358, vec![0, 0x8000_0000]),
    ] {
        let args: Vec<_> = args.into_iter().flat_map(u32::to_le_bytes).collect();
        assert!(host.call(index, &args, &mut vm).is_err());
        assert_eq!(&*vm.read(0, 128).unwrap(), before);
    }
    for args in [&[][..], &[0u8; 4][..]] {
        assert!(host.call(358, args, &mut vm).is_err());
    }
    put(&mut vm, 0x8000_0018, &[1., 2.].map(f32::to_bits));
    put(&mut vm, 64, &[10., 20., 30., 40.].map(f32::to_bits));
    let args = [0x8000_0018u32, 64]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    assert!(host.call(353, &args, &mut vm).is_err());
    assert_eq!(words(&vm, 0x8000_0018, 2), [11., 22.].map(f32::to_bits));
    put(&mut vm, 0x8000_0010, &[1, 2, 3, 4]);
    let args = [64u32, 0x8000_0010]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    assert!(host.call(351, &args, &mut vm).is_err());
    assert_eq!(words(&vm, 64, 4), [1, 2, 3, 4]);
    assert_eq!(words(&vm, 80, 4), [0; 4]);
}
