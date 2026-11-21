use katze_system2::{
    Result,
    expression::State,
    host::platform::{LocalDateTime, SystemServices},
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program, SystemHost},
};
use std::{cell::Cell, rc::Rc};

struct Clock(Rc<Cell<u32>>);

impl SystemServices for Clock {
    fn local_date_time(&mut self) -> Option<LocalDateTime> {
        self.0.set(self.0.get() + 1);
        Some(LocalDateTime {
            year: 2000,
            month: 2,
            weekday: 2,
            day: 29,
            hour: 23,
            minute: 59,
            second: 58,
        })
    }
}

#[test]
fn local_time_preserves_field_order_external_addresses_and_aliases() -> Result<()> {
    let mut vm = Interpreter::new(
        Program::from_code(&[0x3b], vec![0x55; 64], 0)?,
        vec![0x66; 16],
    )?;
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let addresses = [0u32, 0x8000_0000, 4, 8, 12, 16, 20];
    let args: Vec<_> = addresses.into_iter().flat_map(u32::to_le_bytes).collect();
    let before = vm.read(0, 64)?.to_vec();
    assert!(
        host.call(11, &args, &mut vm)
            .unwrap_err()
            .message
            .contains("宿主")
    );
    assert_eq!(vm.read(0, 64)?, before);
    assert_eq!(vm.read(0x8000_0000, 4)?, [0x66; 4]);
    let samples = Rc::new(Cell::new(0));
    host.system_services = Some(Box::new(Clock(samples.clone())));
    assert_eq!(host.call(11, &args, &mut vm)?, NativeReturn::Continue);
    let words: Vec<_> = vm
        .read(0, 24)?
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    assert_eq!(words, [2000, 2, 29, 23, 59, 58]);
    assert_eq!(vm.read(0x8000_0000, 4)?, 2u32.to_le_bytes());
    assert_eq!(host.call(11, &[0; 28], &mut vm)?, NativeReturn::Continue);
    assert_eq!(vm.read(0, 4)?, 58u32.to_le_bytes());
    let before = vm.read(0, 64)?.to_vec();
    let mut invalid = args.clone();
    invalid[24..28].copy_from_slice(&62u32.to_le_bytes());
    assert!(host.call(11, &invalid, &mut vm).is_err());
    assert_eq!(vm.read(0, 64)?, before);
    assert!(host.call(11, &args[..24], &mut vm).is_err());
    assert_eq!(samples.get(), 2);

    host.system_services = None;
    let mut clock = Clock(samples.clone());
    let mut wrapper = SystemHost::new(&mut host, &mut clock);
    assert_eq!(wrapper.call(11, &args, &mut vm)?, NativeReturn::Continue);
    assert_eq!(vm.read(0, 4)?, 2000u32.to_le_bytes());
    assert_eq!(samples.get(), 3);
    Ok(())
}

#[test]
fn weekdays_keep_leap_days_signed_truncation_and_integer_wraparound() -> Result<()> {
    let mut vm = Interpreter::new(Program::from_code(&[0x3b], vec![], 0)?, vec![])?;
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    for (date, expected) in [
        ([2000, 1, 1], 6),
        ([2000, 2, 29], 2),
        ([1900, 3, 1], 4),
        ([2026, 11, 17], 2),
        ([0, 0, 0], 0xffff_fffc),
        ([-1, 0, -1], 1),
        ([i32::MIN; 3], 6),
        ([i32::MAX; 3], 5),
    ] {
        let args: Vec<_> = date.into_iter().flat_map(i32::to_le_bytes).collect();
        assert_eq!(
            host.call(786, &args, &mut vm)?,
            NativeReturn::Value(expected),
            "{date:?}"
        );
    }
    Ok(())
}
