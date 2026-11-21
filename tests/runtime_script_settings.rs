use katze_system2::{
    Result,
    expression::State,
    host::storage::{ConfigurationFile, ConfigurationServices, ScriptSettings},
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Snapshot {
    writes: Vec<u32>,
    bytes: Vec<u8>,
    reject: bool,
}

struct Config(Rc<RefCell<Snapshot>>);

impl ConfigurationServices for Config {
    fn write(&mut self, _: u32, _: ConfigurationFile, _: &[u8]) -> Result<bool> {
        unreachable!("script settings have their own file")
    }

    fn write_script_settings(&mut self, task: u32, bytes: &[u8]) -> Result<bool> {
        let mut data = self.0.borrow_mut();
        data.writes.push(task);
        if data.reject {
            return Ok(false);
        }
        data.bytes = bytes.to_vec();
        Ok(true)
    }
}

fn invoke(host: &mut impl Host, vm: &mut Interpreter, index: u32, words: &[u32]) -> Result<u32> {
    let arguments: Vec<_> = words.iter().copied().flat_map(u32::to_le_bytes).collect();
    let result = host.call(index, &arguments, vm)?;
    match result {
        NativeReturn::Value(value) => Ok(value),
        _ => panic!("settings natives return a DWORD"),
    }
}

#[test]
fn settings_keep_raw_values_and_share_case_insensitive_keys_across_restarts() -> Result<()> {
    let mut data = vec![b'Z'; 256];
    for (at, name) in [
        (0, b"Group\\\x82\xa0-%d[G152]\0".as_slice()),
        (32, b"gROUP\\\x82\xa0-7\0"),
        (64, b"Count\0"),
        (80, b"COUNT\0"),
        (96, b"Raw\0"),
        (112, b"\x82\xa0%raw\x01\0tail"),
        (136, b"missing\0"),
    ] {
        data[at..at + name.len()].copy_from_slice(name);
    }
    data[152..156].copy_from_slice(&7u32.to_le_bytes());
    let program = Program::from_code(&[], data.clone(), 32)?;
    let mut vm = Interpreter::new(program, data)?;
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let stored = Rc::new(RefCell::new(Snapshot::default()));
    host.configuration_services = Some(Box::new(Config(stored.clone())));
    host.task_id = 17;
    assert_eq!(invoke(&mut host, &mut vm, 192, &[136, u32::MAX])?, 0);
    assert_eq!(invoke(&mut host, &mut vm, 194, &[0, 64, 0xabcd_0004])?, 1);
    assert_eq!(invoke(&mut host, &mut vm, 195, &[0, 96, 0x8000_0070])?, 1);
    host.task_id = 29;
    assert_eq!(invoke(&mut host, &mut vm, 192, &[32, 80])?, 1);
    assert_eq!(invoke(&mut host, &mut vm, 192, &[32, 96])?, 0);
    assert_eq!(invoke(&mut host, &mut vm, 193, &[32, 80, u32::MAX, 0])?, 4);
    assert_eq!(invoke(&mut host, &mut vm, 193, &[32, 80, 176, 3])?, 0);
    assert_eq!(&*vm.read(176, 8)?, b"ZZZZZZZZ");
    assert_eq!(invoke(&mut host, &mut vm, 193, &[32, 80, 176, 4])?, 4);
    assert_eq!(&*vm.read(176, 8)?, b"\x04\x00\xcd\xabZZZZ");
    assert_eq!(
        invoke(&mut host, &mut vm, 193, &[32, 96, 0x8000_00b0, 32])?,
        7
    );
    assert_eq!(&*vm.read(0x8000_00b0, 8)?, b"\x82\xa0%raw\x01Z");

    stored.borrow_mut().reject = true;
    assert_eq!(invoke(&mut host, &mut vm, 194, &[32, 80, 7])?, 0);
    assert_eq!(invoke(&mut host, &mut vm, 193, &[32, 80, 184, 4])?, 4);
    assert_eq!(&*vm.read(184, 4)?, b"\x04\x00\xcd\xab");
    let file = stored.borrow();
    assert_eq!(file.writes, [17, 17, 29]);
    assert_eq!(host.script_settings.to_bytes(), file.bytes);
    let mut other_state = State::new(0);
    let mut reopened = CoreHost::new(&mut other_state);
    reopened.script_settings = ScriptSettings::from_bytes(&file.bytes)?;
    assert_eq!(invoke(&mut reopened, &mut vm, 193, &[32, 96, 192, 8])?, 7);
    assert_eq!(&*vm.read(192, 8)?, b"\x82\xa0%raw\x01Z");
    assert!(ScriptSettings::from_bytes(&file.bytes[..file.bytes.len() - 1]).is_err());
    Ok(())
}
