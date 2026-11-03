use std::{cell::RefCell, rc::Rc};

use katze_system2::{
    Error, Result,
    audio::{SoundFactory, SoundObject},
    expression::State,
    kcs::{CoreHost, Program, ResourceServices},
    resource::{Category, ResourceMode},
    runtime::{KcsFactory, ProgramLoader, RootObject, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers},
};

fn command(target: u32, code: u32, value: u32) -> Command {
    Command {
        target,
        sender: 0,
        code,
        data: [0, value, 0],
    }
}

struct Source {
    address: u32,
    extra: bool,
}
const NAME: &[u8] = b"loops_%u[G8].tbl\0";

impl ProgramLoader for Source {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        fn ins(code: &mut Vec<u8>, op: u8, words: &[u32]) {
            code.push(op);
            for word in words {
                code.push(3);
                code.extend(word.to_le_bytes());
            }
        }
        let mut code = vec![];
        ins(&mut code, 0x25, &[0xdeadbeef]);
        ins(&mut code, 0x25, &[0xdeadc0de]);
        ins(&mut code, 0x25, &[self.address]);
        if self.extra {
            ins(&mut code, 0x25, &[0]);
        }
        ins(&mut code, 0x2b, &[400, if self.extra { 12 } else { 8 }]);
        ins(&mut code, 0x26, &[0]);
        ins(&mut code, 0x3b, &[]);
        let mut data = vec![0; 256];
        data[8..12].copy_from_slice(&7u32.to_le_bytes());
        data[64..64 + NAME.len()].copy_from_slice(NAME);
        Ok(Some(Program::from_code(&code, data, 128)?))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Resolve(Vec<u8>),
    Read(Vec<u8>, ResourceMode),
}
struct Resources {
    calls: Rc<RefCell<Vec<Call>>>,
    bytes: Option<Vec<u8>>,
    reject_name: bool,
}

impl ResourceServices for Resources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"sound");
        self.calls.borrow_mut().push(Call::Resolve(name.to_vec()));
        if self.reject_name && name == b"bad.ogg" {
            return Err(Error::new(0, "合成名称补全失败"));
        }
        Category {
            prefix: b"sound.int".to_vec(),
            extension: b"ogg".to_vec(),
        }
        .resolve(name)
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        self.calls
            .borrow_mut()
            .push(Call::Read(name.to_vec(), mode));
        Ok(self.bytes.clone())
    }
}

type Rt = Runtime<SoundFactory<KcsFactory<Source>>>;
fn runtime(address: u32, enabled: bool, extra: bool, sound_target: u32) -> (Rt, u32) {
    let mut kcs = KcsFactory::new(Source { address, extra });
    kcs.external_bytes = 256;
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(u32::MAX, sound_target)),
        State::new(0),
        SoundFactory {
            enabled,
            fallback: kcs,
        },
    );
    assert_eq!(rt.create(0, kind::SOUND, b"sound", b"").unwrap(), 1);
    let caller = rt.create(0, kind::ORDINARY, b"caller", b"").unwrap();
    if address & 0x80000000 != 0 {
        rt.object_mut(caller)
            .unwrap()
            .interpreter_mut()
            .unwrap()
            .write(address, NAME)
            .unwrap();
    }
    (rt, caller)
}

#[test]
fn ordinary_loop_tables_reach_sound_state_and_idle_clocks_under_both_budgets() {
    for budget in [1, 4096] {
        for address in [64, 0x80000040] {
            for enabled in [false, true] {
                let (mut rt, caller) = runtime(address, enabled, false, 1);
                rt.set_budget(budget, 1 << 20);
                let calls = Rc::new(RefCell::new(vec![]));
                rt.set_resources(Resources {
                    calls: calls.clone(),
                    bytes: Some(b"\"a.ogg\" 1 20\nB.ogg 5 -1\nA.ogg 30 99\ninvalid 2 1".to_vec()),
                    reject_name: false,
                });
                rt.update(10, 3).unwrap();
                let audio = rt.object(1).unwrap().audio_state().unwrap();
                assert_eq!(audio.channel_time(0).unwrap(), if enabled { 10 } else { 0 });
                if enabled {
                    assert_eq!(audio.loop_points().entries().len(), 2);
                    let point = audio.loop_points().find(b"SOUND.INT/a.ogg").unwrap();
                    assert_eq!((point.start, point.end), (30, 20));
                    assert_eq!(audio.loop_points().entries()[0].name, b"sound.int/a.ogg");
                } else {
                    assert!(audio.loop_points().entries().is_empty());
                }
                assert!(audio.channel_time(64).is_err());
                let vm = rt.object(caller).unwrap().interpreter().unwrap();
                assert_eq!(vm.read(0, 4).unwrap(), 0xdeadbeefu32.to_le_bytes());
                assert!(vm.stack_bytes().is_empty());
                assert_eq!(
                    *calls.borrow(),
                    [
                        Call::Resolve(b"loops_7.tbl".to_vec()),
                        Call::Read(b"sound.int/loops_7.tbl".to_vec(), ResourceMode::LooseFirst),
                        Call::Resolve(b"a.ogg".to_vec()),
                        Call::Resolve(b"B.ogg".to_vec()),
                        Call::Resolve(b"A.ogg".to_vec()),
                        Call::Resolve(b"invalid".to_vec()),
                    ]
                );
                rt.dispatch_command(0, command(1, 29, 1)).unwrap();
                rt.update(u32::MAX, 0).unwrap();
                rt.update(42, 0).unwrap();
                let audio = rt.object(1).unwrap().audio_state().unwrap();
                for channel in 0..64 {
                    assert_eq!(audio.channel_time(channel).unwrap(), 0);
                }
                if enabled {
                    assert_eq!(audio.loop_points().entries().len(), 2);
                    rt.dispatch_command(0, command(1, 3, 0)).unwrap();
                    assert_eq!(rt.object(1).unwrap().control().deletion_requested, 1);
                    assert!(rt.object(1).unwrap().audio_state().is_some());
                    rt.update(43, 0).unwrap();
                    assert!(rt.object(1).is_none());
                } else {
                    rt.dispatch_command(0, command(1, 3, 0)).unwrap();
                    assert_eq!(rt.object(1).unwrap().control().deletion_requested, 0);
                    rt.destroy(1).unwrap();
                    rt.update(43, 0).unwrap();
                }
                assert_eq!(rt.create(0, kind::SOUND, b"replacement", b"").unwrap(), 1);
                assert!(
                    rt.object(1)
                        .unwrap()
                        .audio_state()
                        .unwrap()
                        .loop_points()
                        .entries()
                        .is_empty()
                );
            }
        }
    }
}

#[test]
fn loop_load_failures_keep_completed_registrations_and_check_parameters_first() {
    for bytes in [None, Some(vec![])] {
        let (mut rt, caller) = runtime(64, true, false, 99);
        let calls = Rc::new(RefCell::new(vec![]));
        rt.set_resources(Resources {
            calls: calls.clone(),
            bytes,
            reject_name: false,
        });
        rt.update(0, 0).unwrap();
        assert_eq!(calls.borrow().len(), 2);
        assert_eq!(
            rt.object(caller)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(0, 4)
                .unwrap(),
            0xdeadbeefu32.to_le_bytes()
        );
    }
    let (mut rt, _) = runtime(64, true, false, 1);
    rt.set_resources(Resources {
        calls: Rc::new(RefCell::new(vec![])),
        bytes: Some(b"a.ogg 1 20\nbad.ogg 2 30".to_vec()),
        reject_name: true,
    });
    assert!(rt.update(0, 0).unwrap_err().message.contains("补全失败"));
    assert_eq!(
        rt.object(1)
            .unwrap()
            .audio_state()
            .unwrap()
            .loop_points()
            .entries()
            .len(),
        1
    );
    let (mut rt, _) = runtime(64, true, false, 99);
    rt.set_resources(Resources {
        calls: Rc::new(RefCell::new(vec![])),
        bytes: Some(b"a.ogg".to_vec()),
        reject_name: false,
    });
    assert!(rt.update(0, 0).unwrap_err().message.contains("目标不可达"));
    let (mut rt, _) = runtime(64, true, true, 1);
    assert!(rt.update(0, 0).unwrap_err().message.contains("参数字节数"));
    let (mut rt, _) = runtime(64, true, false, 1);
    assert!(rt.update(0, 0).unwrap_err().message.contains("声音资源"));
}

#[test]
fn sound_commands_gate_buffers_and_preserve_unwritten_return_slots() {
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let mut sound = SoundObject::new(true);
    let mut output = 0xdeadbeef;
    sound
        .dispatch_command(kind::SOUND, &command(1, 199, 0), &mut output, &mut host, 1)
        .unwrap();
    assert_eq!(output, 0xdeadbeef);
    for channel in [0, 64, u32::MAX] {
        for code in [182, 183, 184, 185, 187, 190, 218, 219] {
            let mut request = command(1, code, 17);
            request.data[0] = channel;
            sound
                .dispatch_command(kind::SOUND, &request, &mut output, &mut host, 1)
                .unwrap();
            assert_eq!(output, 0xdeadbeef);
        }
        for code in [186, 188, 191] {
            let mut query = command(1, code, 0);
            query.data[0] = channel;
            sound
                .dispatch_command(kind::SOUND, &query, &mut output, &mut host, 1)
                .unwrap();
            assert_eq!(output, 0);
            output = 0xdeadbeef;
        }
    }
    let mut guarded_load = command(1, 180, 0);
    guarded_load.data[0] = 64;
    sound
        .dispatch_command(kind::SOUND, &guarded_load, &mut output, &mut host, 1)
        .unwrap();
    assert_eq!(output, 0xdeadbeef);
    guarded_load.data[0] = 0;
    assert!(
        sound
            .dispatch_command(kind::SOUND, &guarded_load, &mut output, &mut host, 1)
            .unwrap_err()
            .message
            .contains("记录缓冲区")
    );
    assert!(
        sound
            .dispatch_command(kind::SOUND, &command(1, 181, 0), &mut output, &mut host, 1)
            .unwrap_err()
            .message
            .contains("声音对象命令")
    );
    let mut record = [b'A'; 48];
    record[32..48].fill(0);
    sound
        .dispatch_command_with_buffers(
            kind::SOUND,
            &command(1, 199, 0),
            &mut CommandBuffers {
                data1: Some(&mut record),
                ..Default::default()
            },
            &mut output,
            &mut host,
            1,
        )
        .unwrap();
    record[40..48].copy_from_slice(&1i64.to_le_bytes());
    assert!(
        sound
            .dispatch_command_with_buffers(
                kind::SOUND,
                &command(1, 199, 0),
                &mut CommandBuffers {
                    data1: Some(&mut record),
                    ..Default::default()
                },
                &mut output,
                &mut host,
                1
            )
            .is_err()
    );
    let mut short = [0; 4];
    assert!(
        sound
            .dispatch_command_with_buffers(
                kind::SOUND,
                &command(1, 199, 0),
                &mut CommandBuffers {
                    data1: Some(&mut short),
                    ..Default::default()
                },
                &mut output,
                &mut host,
                1
            )
            .is_err()
    );
    sound.state().0.deletion_requested = -1;
    sound
        .dispatch_command_with_buffers(
            kind::SOUND,
            &command(1, 199, 0),
            &mut CommandBuffers {
                data1: Some(&mut short),
                ..Default::default()
            },
            &mut output,
            &mut host,
            1,
        )
        .unwrap();
    assert_eq!(output, 0xdeadbeef);
    let mut disabled = SoundObject::new(false);
    disabled
        .dispatch_command_with_buffers(
            kind::SOUND,
            &command(1, 199, 0),
            &mut CommandBuffers {
                data1: Some(&mut short),
                ..Default::default()
            },
            &mut output,
            &mut host,
            1,
        )
        .unwrap();
    assert_eq!(output, 0);
    assert!(disabled.audio().loop_points().entries().is_empty());
    output = 0xdeadbeef;
    disabled
        .dispatch_command(kind::SOUND, &command(1, 13, 0), &mut output, &mut host, 1)
        .unwrap();
    assert_eq!(output, 0);
}
