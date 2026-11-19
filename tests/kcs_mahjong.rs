use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().copied().flat_map(u32::to_le_bytes).collect()
}

#[test]
fn mahjong_handles_keep_independent_packets_over_shared_rounds() -> Result<()> {
    use NativeReturn::{Continue, Value};

    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(Program::from_code(&[], vec![0; 128], 32)?, vec![0; 32])?;
    let mut call = |index, args: &[u32]| host.call(index, &words(args), &mut vm);
    let Value(first) = call(470, &[])? else {
        panic!("missing first handle")
    };
    let Value(second) = call(470, &[])? else {
        panic!("missing second handle")
    };
    assert_ne!(first, second);
    assert_eq!(call(481, &[second])?, Value(u32::MAX));
    assert_eq!(call(481, &[0])?, Value(0));

    vm.write(0, &words(&[11, 22, 33]))?;
    vm.write(0x8000_0000, &words(&[100, 200, 300, u32::MAX]))?;
    let mut call = |index, args: &[u32]| host.call(index, &words(args), &mut vm);
    assert_eq!(
        call(472, &[first, 4, 7, 8, 9, 10, 11, 0, 0x8000_0000])?,
        Continue
    );
    let packet = [
        0, 4, 1, 7, 2, 8, 3, 9, 10, 11, 4, 1, 11, 4, 2, 22, 4, 3, 33, 5, 0, 100, 5, 1, 200, 5, 2,
        300,
    ];
    assert_eq!(call(481, &[first])?, Value(packet.len() as u32 - 1));
    for (index, word) in packet.into_iter().enumerate() {
        assert_eq!(
            call(480, &[first, (index as u32).wrapping_sub(1)])?,
            Value(word)
        );
    }
    assert_eq!(call(495, &[second, 2])?, Value(300));

    vm.write(0x8000_0000, &words(&[0, 150, 600, 0]))?;
    let mut call = |index, args: &[u32]| host.call(index, &words(args), &mut vm);
    assert_eq!(
        call(472, &[second, 2, 0, 1, 2, 3, 4, 0, 0x8000_0000])?,
        Continue
    );
    assert_eq!(call(480, &[first, 0])?, Value(4));
    assert_eq!(call(480, &[second, 0])?, Value(2));
    assert_eq!(call(475, &[first])?, Value(2));
    assert_eq!(call(492, &[first])?, Value(61));
    assert_eq!(call(495, &[first, 0])?, Value(100));
    assert_eq!(call(495, &[first, 1])?, Value(150));
    assert_eq!(call(495, &[first, 2])?, Value(0));
    assert_eq!(call(471, &[first])?, Continue);
    assert!(call(475, &[first]).is_err());
    assert_eq!(call(495, &[second, 1])?, Value(150));
    Ok(())
}

#[test]
fn mahjong_deals_from_the_rolled_wall_and_preserves_the_dealers_extra_tile() -> Result<()> {
    use NativeReturn::{Continue, Value};

    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    host.set_mahjong_seed(4357);
    let expected_random = host.random.clone().next_integer();
    let mut vm = Interpreter::new(Program::from_code(&[], vec![0; 256], 32)?, vec![0; 256])?;
    let Value(handle) = host.call(470, &[], &mut vm)? else {
        panic!("missing Mahjong handle")
    };
    vm.write(0x8000_0000, &words(&[25_000, 27_000, 29_000, 31_000]))?;
    let mut call = |index, args: &[u32]| host.call(index, &words(args), &mut vm);
    assert_eq!(
        call(472, &[handle, 2, 1, 0, 1, 2, 3, 0, 0x8000_0000])?,
        Continue
    );
    assert_eq!(call(483, &[handle, 1, 2])?, Continue);
    assert_eq!(call(473, &[handle])?, Value(10003));
    assert_eq!(call(483, &[handle, 7, 2])?, Continue);
    assert_eq!(call(473, &[handle])?, Value(10004));
    assert_eq!(call(483, &[handle, 1, 2])?, Continue);
    for event in 20001..=20004 {
        assert_eq!(call(473, &[handle])?, Value(event));
    }
    for (index, expected) in [
        48, 539, 131072, 304, 538, 131073, 384, 537, 131074, 274, 536, 131075,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(call(480, &[handle, index as u32])?, Value(expected));
    }
    for _ in 0..7 {
        assert_eq!(call(473, &[handle])?, Value(20004));
    }
    assert_eq!(call(473, &[handle])?, Value(20005));
    assert_eq!(call(473, &[handle])?, Value(20006));
    assert_eq!(call(492, &[handle])?, Value(34));
    assert_eq!(call(479, &[handle, 4, 0x8000_0040])?, Value(14));
    assert_eq!(call(479, &[handle, 1, 0x9000_0000])?, Value(13));
    let data = vm.read(0x8000_0040, 14 * 4)?;
    let tiles: Vec<_> = data
        .chunks_exact(4)
        .map(|tile| u32::from_le_bytes(tile.try_into().unwrap()))
        .collect();
    assert!(
        tiles
            .windows(2)
            .all(|pair| (pair[0] & 0x3ff) < (pair[1] & 0x3ff))
    );
    assert_eq!(host.random.next_integer(), expected_random);
    Ok(())
}
