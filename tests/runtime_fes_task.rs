use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{GraphicsFactory, Projection, RenderTarget, TextureFilter, Transform},
    input::{InputDevice, InputFactory, KeyEvent, KeyEventKind, KeySample, PointerSample},
    kcs::{Host, Interpreter, Program, ResourceServices},
    resource::ResourceMode,
    runtime::{
        BaseObject, ObjectFactory, OrdinaryObject, RootObject, Runtime, TaskObject,
        fes::FesFactory, kind,
    },
    task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::Write,
    rc::Rc,
};

type Queries = Rc<RefCell<Vec<(u32, u32, bool)>>>;
type ControlCalls = Rc<RefCell<Vec<(Command, Vec<u8>)>>>;
type TestRuntime = Runtime<ButtonFactory<FesFactory<GraphicsFactory<InputFactory<Factory>>>>>;

struct ButtonFactory<F> {
    fallback: F,
    fail: Rc<Cell<bool>>,
    attempts: Rc<Cell<u32>>,
    scroll: Option<ControlCalls>,
}
impl<F: ObjectFactory> ObjectFactory for ButtonFactory<F> {
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind == kind::BUTTON && name == b"low button" {
            self.attempts.set(self.attempts.get() + 1);
        }
        if kind == kind::BUTTON && self.fail.get() {
            return Err(Error::new(0, "合成按钮创建失败"));
        }
        if kind == kind::SCROLL2 {
            if let Some(calls) = &self.scroll {
                assert_eq!(name, b"FE scroll2");
                return Ok(Box::new(ScrollTransport {
                    id,
                    base: BaseObject::new(),
                    calls: calls.clone(),
                }));
            }
        }
        self.fallback.create(id, kind, name)
    }
}

struct Factory {
    child: Rc<Cell<Option<u32>>>,
}
impl ObjectFactory for Factory {
    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if kind == kind::ORDINARY && name == b"native driver" {
            fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
                code.push(op);
                for value in args {
                    code.push(3);
                    code.extend(value.to_le_bytes());
                }
            }
            let mut code = Vec::new();
            ins(&mut code, 0x25, &[0xdead_beef]);
            for (native, args) in [
                (449, vec![3, 4]),
                (447, vec![3, 64]),
                (882, vec![3, 160, 0xdead_beef]),
                (882, vec![3, 160, 0x8000_0001]),
                (882, vec![3, 192, 9]),
                (882, vec![3, 200, 19]),
                (882, vec![3, 208, 33]),
                (882, vec![0, 160, 77]),
                (882, vec![u32::MAX, 160, 77]),
                (645, vec![3, 2, 17]),
                (645, vec![3, 511, 0x8000_0001]),
                (645, vec![3, 512, 0x1234_5678]),
                (645, vec![0, 2, 99]),
                (645, vec![u32::MAX, 2, 99]),
                (448, vec![3]),
                (630, vec![3, 112, 144, 0x8000_0001]),
                (630, vec![3, 112, 0, u32::MAX]),
                (630, vec![3, 0, 0, 9]),
                (630, vec![0, 0, 0, 9]),
                (630, vec![u32::MAX, 0, 0, 9]),
            ] {
                for value in &args {
                    ins(&mut code, 0x25, &[*value]);
                }
                ins(&mut code, 0x2b, &[native, args.len() as u32 * 4]);
            }
            for (target, address) in [(3, 4), (0, 8), (u32::MAX, 12)] {
                ins(&mut code, 0x25, &[target]);
                ins(&mut code, 0x2b, &[452, 4]);
                ins(&mut code, 0x26, &[address]);
            }
            for (index, address) in [(2, 16), (511, 20), (512, 24), (u32::MAX, 28)] {
                ins(&mut code, 0x25, &[3]);
                ins(&mut code, 0x25, &[index]);
                ins(&mut code, 0x2b, &[646, 8]);
                ins(&mut code, 0x26, &[address]);
            }
            for (key, address) in [(160, 36), (192, 40), (200, 44), (208, 48), (220, 52)] {
                ins(&mut code, 0x25, &[3]);
                ins(&mut code, 0x25, &[key]);
                ins(&mut code, 0x2b, &[883, 8]);
                ins(&mut code, 0x26, &[address]);
            }
            ins(&mut code, 0x26, &[0]);
            ins(&mut code, 0x3b, &[]);
            let mut data = vec![0; 240];
            data[32..36].copy_from_slice(&7u32.to_le_bytes());
            let format = b"complete_%d[G32]\0";
            data[64..64 + format.len()].copy_from_slice(format);
            data[112..125].copy_from_slice(b"BLOCK%d[G32]\0");
            data[144..151].copy_from_slice(b"sprite\0");
            data[160..171].copy_from_slice(b"key%d[G32]\0");
            data[192..197].copy_from_slice(b"KEY7\0");
            data[200..203].copy_from_slice(b"\x82\xa0\0");
            data[220..228].copy_from_slice(b"missing\0");
            let mut task = katze_system2::kcs::OrdinaryTask::new(
                Program::from_code(&code, data, 128)?,
                Vec::new(),
            )?;
            task.task_id = id;
            return Ok(Box::new(OrdinaryObject::new(task)));
        }
        if kind != kind::BASE {
            return Err(Error::new(0, "合成工厂没有该类型"));
        }
        if name == b"driver" || name == b"delete parent" {
            Ok(Box::new(Driver {
                base: BaseObject {
                    clock: TaskClock {
                        origin: if name == b"driver" { 400 } else { 0 },
                        ..TaskClock::default()
                    },
                    ..BaseObject::new()
                },
                id,
                child: self.child.clone(),
                context: Vec::new(),
                delete_parent: name == b"delete parent",
            }))
        } else {
            Ok(Box::new(BaseObject::new()))
        }
    }
}

struct Files {
    files: BTreeMap<Vec<u8>, Vec<u8>>,
    reads: Rc<RefCell<Vec<Vec<u8>>>>,
    fail_media: Rc<Cell<bool>>,
}
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        if category == b"mot" {
            assert_eq!(name, b"dummy");
            return Ok(b"draw.kcs".to_vec());
        }
        assert_eq!(category, b"fes");
        if name == b"child" {
            return Ok(b"child.fes".to_vec());
        }
        assert!(name == b"complete_7" || name.is_empty());
        Ok(b"complete.fes\0ignored".to_vec())
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.reads.borrow_mut().push(name.to_vec());
        if self.fail_media.get() && name != b"complete.fes" {
            return Err(Error::new(0, "合成媒体读取失败"));
        }
        Ok(self.files.get(name).cloned())
    }
}

struct Device {
    queries: Queries,
    mask: Rc<Cell<bool>>,
    pointer: Rc<Cell<PointerSample>>,
}
impl InputDevice for Device {
    fn poll(&mut self) -> Result<()> {
        Ok(())
    }
    fn pointer(&mut self) -> Result<PointerSample> {
        Ok(self.pointer.get())
    }
    fn key(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample> {
        self.queries.borrow_mut().push((key, group, repeat));
        Ok(KeySample {
            mask: if self.mask.get() && key == 12 {
                [0, 2, 0x80000000, 8][group as usize]
            } else {
                0
            },
            repeat: 0,
        })
    }
    fn take_wheel(&mut self) -> Result<u32> {
        Ok(0)
    }
    fn is_foreground(&mut self) -> Result<bool> {
        Ok(true)
    }
    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        Ok(())
    }
    fn history(&mut self) -> Result<[u8; 32]> {
        Ok([0; 32])
    }
    fn show_cursor(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
    fn cursor_visible(&mut self) -> Result<u32> {
        Ok(1)
    }
    fn set_pointer(&mut self, position: [i32; 2]) -> Result<()> {
        self.pointer.set(PointerSample {
            position,
            ..self.pointer.get()
        });
        Ok(())
    }
    fn clear_keys(&mut self) -> Result<()> {
        Ok(())
    }
}

fn packed(source: &[u8]) -> Vec<u8> {
    let mut writer = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    writer.write_all(source).unwrap();
    let compressed = writer.finish().unwrap();
    let mut bytes = b"FES\0".to_vec();
    for value in [compressed.len() as u32, source.len() as u32, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(compressed);
    bytes
}

fn command(target: u32, code: u32, a: u32, b: u32) -> Command {
    Command {
        target,
        sender: 0,
        code,
        data: [a, b, 0],
    }
}

fn send(runtime: &mut TestRuntime, target: u32, code: u32, a: u32, b: u32) -> u32 {
    runtime
        .dispatch_command(0, command(target, code, a, b))
        .unwrap()
}

fn file(runtime: &mut TestRuntime, target: u32, name: &[u8]) -> Result<u32> {
    let mut name = name.to_vec();
    runtime.dispatch_command_with_buffers(
        0,
        command(target, 338, 0, 1),
        &mut CommandBuffers {
            data0: None,
            data1: Some(&mut name),
            external: None,
        },
    )
}

struct Setup {
    runtime: TestRuntime,
    queries: Queries,
    mask: Rc<Cell<bool>>,
    reads: Rc<RefCell<Vec<Vec<u8>>>>,
    child: Rc<Cell<Option<u32>>>,
    fail_button: Rc<Cell<bool>>,
    button_attempts: Rc<Cell<u32>>,
    pointer: Rc<Cell<PointerSample>>,
    fail_media: Rc<Cell<bool>>,
}

#[test]
fn configuration_command_preserves_output_and_main_wait_across_budgets() {
    for budget in [1, 4096] {
        let mut runtime = setup(
            budget,
            b"#DEFINE\nBASE item\n#START\nwait 1000\n#MES_CHANGECONFIG\n$0+=1\n",
        )
        .runtime;
        let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
        let notify = Command {
            target: fes,
            sender: 37,
            code: 19,
            data: [u32::MAX; 3],
        };
        assert_eq!(runtime.dispatch_command(0, notify).unwrap(), 0);
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.update(100, 1).unwrap();
        let deadline = runtime.object(fes).unwrap().fes_scene().unwrap().deadline();
        for expected in [1, 2] {
            assert_eq!(runtime.dispatch_command(0, notify).unwrap(), 0);
            assert_eq!(runtime.numbers().shared[0], expected);
            assert_eq!(
                runtime.object(fes).unwrap().fes_scene().unwrap().deadline(),
                deadline
            );
        }
        send(&mut runtime, fes, 348, 0, 1);
        assert_eq!(runtime.dispatch_command(0, notify).unwrap(), 0);
        assert_eq!(runtime.numbers().shared[0], 3);
        assert_eq!(runtime.object(fes).unwrap().paused_fes(), Some(1));
        runtime.dispatch_command(0, command(fes, 3, 0, 0)).unwrap();
        assert_eq!(runtime.dispatch_command(0, notify).unwrap(), 0);
        assert_eq!(runtime.numbers().shared[0], 3);
    }
}

#[test]
fn frame_configuration_retries_the_fes_handler_before_another_body_update() {
    for budget in [1, 4096] {
        let mut runtime = setup(
            budget,
            b"#START\n$1+=1\nwait 1000\n#MES_CHANGECONFIG\n$0+=1\nsetconf 7 messpd\n",
        )
        .runtime;
        let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.host_mut().system_settings_dirty = 7;
        assert!(
            runtime
                .update(100, 1)
                .unwrap_err()
                .message
                .contains("全局设置")
        );
        assert_eq!(runtime.numbers().shared[..2], [1, 1]);
        assert_eq!(runtime.host().system_settings_dirty, 7);
        let deadline = runtime.object(fes).unwrap().fes_scene().unwrap().deadline();
        runtime.host_mut().system_settings = Some(Default::default());
        runtime.update(200, 9).unwrap();
        assert_eq!(runtime.numbers().shared[..2], [1, 1]);
        assert_eq!(runtime.host().system_settings_dirty, 0);
        assert_eq!(runtime.object(fes).unwrap().clock().time, 100);
        assert_eq!(
            runtime.object(fes).unwrap().fes_scene().unwrap().deadline(),
            deadline
        );
        runtime.update(201, 1).unwrap();
        assert_eq!(runtime.numbers().shared[..2], [1, 1]);
    }
}

fn setup(budget: usize, source: &[u8]) -> Setup {
    let queries = Rc::new(RefCell::new(Vec::new()));
    let mask = Rc::new(Cell::new(false));
    let child = Rc::new(Cell::new(None));
    let reads = Rc::new(RefCell::new(Vec::new()));
    let fail_button = Rc::new(Cell::new(false));
    let button_attempts = Rc::new(Cell::new(0));
    let pointer = Rc::new(Cell::new(PointerSample::default()));
    let fail_media = Rc::new(Cell::new(false));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(1, 2)),
        State::new(8),
        ButtonFactory {
            fail: fail_button.clone(),
            attempts: button_attempts.clone(),
            scroll: None,
            fallback: FesFactory {
                fallback: GraphicsFactory {
                    fallback: InputFactory {
                        fallback: Factory {
                            child: child.clone(),
                        },
                    },
                },
            },
        },
    );
    runtime.set_budget(budget, 10000);
    runtime.host_mut().surface_size = [8, 6];
    runtime
        .set_input_device(Device {
            queries: queries.clone(),
            mask: mask.clone(),
            pointer: pointer.clone(),
        })
        .unwrap();
    runtime.set_resources(Files {
        files: BTreeMap::from([(b"complete.fes".to_vec(), packed(source))]),
        reads: reads.clone(),
        fail_media: fail_media.clone(),
    });
    runtime.drawing_mut().enabled = true;
    runtime.drawing_mut().active = true;
    runtime.drawing_mut().target = Some(RenderTarget::new([8, 6]).unwrap());
    assert_eq!(runtime.create(0, kind::INPUT, b"input", b"").unwrap(), 1);
    assert_eq!(
        runtime
            .create(0, kind::BASE, b"unused sound slot", b"")
            .unwrap(),
        2
    );
    Setup {
        runtime,
        queries,
        mask,
        reads,
        child,
        fail_button,
        button_attempts,
        pointer,
        fail_media,
    }
}

#[test]
fn configuration_reads_share_host_settings_in_body_and_event_handlers() {
    let source = b"#START\ngetconf ($7+=1)-1 messpd\ngetconf 1 wndcolor\ngetconf 2 lang_text_main\ngetconf 3 lang_text_sub\ngetconf 4 missing ($7+=1) ($7+=1)\ngetconf -1 wndframe\ngetconf 5 play 47 1\ngetconf 6 screen -1 -37\ngetconf 8 iswindow -1 -37\nwait 999\n#READ\ngetconf 0 messpd\ngetconf 1 dragwnd\ngetconf 2 wndframe\ngetconf 3 lang_ui\ngetconf 5 play 46 1\ngetconf 6 screen\ngetconf 8 iswindow\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        runtime.numbers_mut().shared.resize(9, 0);
        let mut settings = katze_system2::settings::SystemSettings::default();
        settings.message[..4].copy_from_slice(&37u32.to_le_bytes());
        settings.window[..4].copy_from_slice(&0x80123456u32.to_le_bytes());
        settings.fullscreen = 0x80000001;
        runtime.host_mut().system_settings = Some(settings);
        runtime.host_mut().windowed_mode = Some(7);
        runtime.host_mut().languages.message_index = 9;
        runtime.host_mut().languages.primary_index = 3;
        runtime.numbers_mut().shared[4] = 71;
        let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.update(100, 1).unwrap();
        assert_eq!(
            runtime.numbers().shared[..8],
            [
                37,
                0x80123456u32 as i32,
                9,
                -1,
                71,
                0,
                0x80000001u32 as i32,
                3
            ]
        );
        runtime.host_mut().system_settings.as_mut().unwrap().message[..4]
            .copy_from_slice(&11u32.to_le_bytes());
        assert_eq!(runtime.numbers().shared[8], 7);
        runtime
            .host_mut()
            .system_settings
            .as_mut()
            .unwrap()
            .fullscreen = 39;
        runtime.host_mut().windowed_mode = Some(0);
        let mut block = b"READ\0".to_vec();
        runtime
            .dispatch_command_with_buffers(
                0,
                command(fes, 343, 0, 0),
                &mut CommandBuffers {
                    data1: Some(&mut block),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(runtime.numbers().shared[..8], [11, 1, 1, 3, 71, 1, 39, 3]);
        assert_eq!(runtime.numbers().shared[8], 0);
        assert_eq!(runtime.host().system_settings_dirty, 0);
    }
}

#[test]
fn configuration_writes_apply_presets_raw_values_and_volumes_in_body_and_handlers() {
    let source = b"#START\nsetconf ($6+=1)*10 messpd ($6+=1) ($6+=1)\nsetconf 7 autospd\nsetconf 50 vol_master\nsetconf 30 vol_bgm\nsetconf 0x81223344 wndcolor\nsetconf -7 waitpcm\nsetconf -3 effcut\nsetconf 9 play 46 1\nsetconf 0 enable_sceneanm\nsetconf 1 lang_text_sub\ngetconf 0 messpd\nwait 999\n#WRITE\nsetconf -1 contauto\nsetconf 8 autospd\nsetconf 1 autospd_b\nsetconf 0 vol_master\nsetconf 2 lang_text_sub\ngetconf 1 lang_text_sub\ngetconf 2 autospd_b\ngetconf 3 autospd_c\ngetconf 4 contauto\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        let mut settings = katze_system2::settings::SystemSettings::default();
        settings.auto_wait_presets[7] = [31, 9];
        settings.auto_wait_presets[8] = [37, 11];
        runtime.host_mut().system_settings = Some(settings);
        runtime.host_mut().languages.prefixes = vec![b"a".to_vec(), b"b".to_vec()];
        let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.update(100, 1).unwrap();
        let word = |bytes: &[u8], offset| {
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
        };
        let settings = runtime.host().system_settings.as_ref().unwrap();
        assert_eq!(runtime.numbers().shared[0], 10);
        assert_eq!(runtime.numbers().shared[6], 3);
        assert_eq!(word(&settings.message, 4), 7);
        assert_eq!(word(&settings.message, 12), 31);
        assert_eq!(word(&settings.message, 16), 9);
        assert_eq!(word(&settings.message, 28), (-7i32) as u32);
        assert_eq!(word(&settings.effects, 4), (-3i32) as u32);
        assert_eq!(word(&settings.window, 0), 0x81223344);
        assert_eq!(word(&settings.sound, 20 + 47 * 116), 9);
        assert_eq!(word(&settings.sound, 0), 15);
        assert_eq!(word(&settings.sound, 4), 50);
        assert_eq!(settings.scene_animation_enabled, 0);
        assert_eq!(runtime.host().languages.subtitle_index, 1);
        assert_eq!(runtime.host().system_settings_dirty, 0);
        runtime.host_mut().system_settings_dirty = 0;
        let mut block = b"WRITE\0".to_vec();
        runtime
            .dispatch_command_with_buffers(
                0,
                command(fes, 343, 0, 0),
                &mut CommandBuffers {
                    data1: Some(&mut block),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(runtime.numbers().shared[..5], [10, -1, 1, 11, -1]);
        let settings = runtime.host().system_settings.as_ref().unwrap();
        assert_eq!(&settings.sound[..16], [0; 16]);
        assert_eq!(word(&settings.message, 4), 8);
        assert_eq!(runtime.host().system_settings_dirty, 1);
    }
}

#[test]
fn language_writes_complete_without_notifications_for_unchanged_ui_and_invalid_indices() {
    let source = b"#START\nsetconf ($7+=1) lang_ui ($7+=1) ($7+=1)\nsetconf 3 lang_text_main\nsetconf -1 lang_ui\nsetconf 0x80000000 lang_text_main\ngetconf 0 lang_ui\ngetconf 1 lang_text_main\nwait 999\n#WRITE\nsetconf 1 lang_ui\nsetconf -1 lang_text_main\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        runtime.host_mut().system_settings = Some(Default::default());
        runtime.host_mut().languages.prefixes = vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()];
        runtime.host_mut().languages.primary_index = 1;
        runtime.host_mut().languages.message_index = 1;
        let languages = runtime.host().languages.clone();
        let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.update(100, 1).unwrap();
        assert_eq!(runtime.numbers().shared[..2], [1, 1]);
        assert_eq!(runtime.numbers().shared[7], 3);
        assert_eq!(runtime.host().languages, languages);
        assert_eq!(runtime.host().system_settings_dirty, 0);
        let settings = runtime.host().system_settings.clone();
        runtime.host_mut().system_settings_dirty = 0;
        let mut block = b"WRITE\0".to_vec();
        runtime
            .dispatch_command_with_buffers(
                0,
                command(fes, 343, 0, 0),
                &mut CommandBuffers {
                    data1: Some(&mut block),
                    ..Default::default()
                },
            )
            .unwrap();
        runtime.update(101, 1).unwrap();
        assert_eq!(runtime.host().languages, languages);
        assert_eq!(runtime.host().system_settings, settings);
        assert_eq!(runtime.host().system_settings_dirty, 0);
        assert!(runtime.host().language_change_requests.is_empty());
    }
}

#[test]
fn language_configuration_changes_publish_ordered_requests_after_body_and_handlers() {
    use katze_system2::language::{LanguageChangeRequest, LanguageTarget::*};
    let source = b"#START\nsetconf 1 lang_ui\ngetconf 0 lang_ui\nsetconf 2 lang_text_main\ngetconf 1 lang_text_main\nsetconf 1 lang_ui\nsetconf 1 lang_text_main\ngetconf 2 lang_text_main\nsetconf 0 lang_ui\ngetconf 3 lang_ui\nwait 999\n#WRITE\nsetconf 2 lang_ui\nsetconf 1 lang_text_main\nsetconf 0 lang_text_main\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        runtime.host_mut().system_settings = Some(Default::default());
        runtime.host_mut().languages.prefixes = vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()];
        let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.update(100, 1).unwrap();
        assert_eq!(runtime.numbers().shared[..4], [1, 2, 1, 0]);
        let expected = |targets: &[katze_system2::language::LanguageTarget]| {
            targets
                .iter()
                .map(|&target| LanguageChangeRequest {
                    task_id: fes,
                    target,
                })
                .collect::<Vec<_>>()
        };
        let first = expected(&[Interface, MainText, MainText, Interface]);
        assert_eq!(runtime.host().language_change_requests, first);
        runtime.host_mut().system_settings_dirty = 0;
        let mut block = b"WRITE\0".to_vec();
        runtime
            .dispatch_command_with_buffers(
                0,
                command(fes, 343, 0, 0),
                &mut CommandBuffers {
                    data1: Some(&mut block),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(runtime.host().languages.primary_index, 2);
        assert_eq!(runtime.host().languages.message_index, 0);
        assert_eq!(runtime.host().system_settings_dirty, 1);
        assert_eq!(runtime.host().language_change_requests, first);
        runtime.update(101, 1).unwrap();
        assert_eq!(
            runtime.host().language_change_requests,
            expected(&[
                Interface, MainText, MainText, Interface, Interface, MainText, MainText
            ])
        );
        runtime.update(102, 1).unwrap();
        assert_eq!(runtime.host().language_change_requests.len(), 7);
    }
}

#[test]
fn font_configuration_keeps_raw_aliases_and_updates_message_defaults_in_handlers() {
    let source = b"#START\nsetconf -7 fshadow 0 99\nsetconf 9 fshadow 1 -99\nsetconf 0x80000000 fshadow 999\nsetconf 13 fontprop\nsetconf -3 fontfix\ngetconf 0 fshadow 0\ngetconf 1 fshadow 1\ngetconf 2 fshadow 2\ngetconf 3 fontweight\ngetconf 4 fontprop\ngetconf 5 fontfix\nwait 999\n#WRITE\nsetconf 0 fontweight\nsetconf 17 fshadow 1 ($7+=1)\ngetconf 6 fshadow 1 ($7+=1)\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        runtime.host_mut().system_settings = Some(Default::default());
        let selected: Vec<_> = "Selected Face".encode_utf16().collect();
        runtime.host_mut().message_defaults = Some(katze_system2::message::MessageDefaults {
            font_name: selected.clone(),
            reveal: true,
            ..Default::default()
        });
        let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.update(100, 1).unwrap();
        assert_eq!(
            runtime.numbers().shared[..6],
            [-7, 9, i32::MIN, i32::MIN, 13, -3]
        );
        assert_eq!(
            runtime
                .host()
                .system_settings
                .as_ref()
                .unwrap()
                .font_controls,
            [(-7i32) as u32, 9, i32::MIN as u32, 13, (-3i32) as u32]
        );
        let defaults = runtime.host().message_defaults.as_ref().unwrap();
        assert_eq!(
            (defaults.outline, defaults.shadow, defaults.bold),
            ((-7i32) as u32, 9, true)
        );
        assert_eq!(defaults.font_name, selected);
        assert!(defaults.reveal);
        assert_eq!(defaults.glyph_parameter, Some((-3i32) as u32));
        assert_eq!(runtime.host().system_settings_dirty, 0);
        runtime.host_mut().system_settings_dirty = 0;
        let mut block = b"WRITE\0".to_vec();
        runtime
            .dispatch_command_with_buffers(
                0,
                command(fes, 343, 0, 0),
                &mut CommandBuffers {
                    data1: Some(&mut block),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(runtime.numbers().shared[6..8], [17, 2]);
        let defaults = runtime.host().message_defaults.as_ref().unwrap();
        assert_eq!((defaults.shadow, defaults.bold), (17, false));
        assert_eq!(runtime.host().system_settings_dirty, 1);
    }
}

#[test]
fn system_voice_configuration_checks_combined_index_but_accesses_first_index() {
    let source = b"#START\nsetconf -7 sysplay 46 1\nsetconf 19 play 46 1\nsetconf 99 sysplay 47 1\nsetconf -3 activewindow\nsetconf 0x80000001 use_sceneskip\nsetconf 9 use_jtalk\ngetconf 0 sysplay 46 1\ngetconf 1 sysplay 47\ngetconf 2 sysplay 47 1\ngetconf 3 play 46 1\ngetconf 4 activewindow\ngetconf 5 use_sceneskip\ngetconf 6 use_jtalk\nwait 999\n#WRITE\nsetconf 37 sysplay 47 -1\ngetconf 0 sysplay 46 1\ngetconf 1 sysplay 47 -1\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        runtime.host_mut().system_settings = Some(Default::default());
        let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.update(100, 1).unwrap();
        assert_eq!(
            runtime.numbers().shared[..7],
            [-7, 1, 0, 19, -3, 0x80000001u32 as i32, 9]
        );
        assert_eq!(runtime.host().system_settings_dirty, 0);
        let mut block = b"WRITE\0".to_vec();
        runtime
            .dispatch_command_with_buffers(
                0,
                command(fes, 343, 0, 0),
                &mut CommandBuffers {
                    data1: Some(&mut block),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(runtime.numbers().shared[..2], [-7, 37]);
    }
}

#[test]
fn configuration_write_validation_keeps_argument_effects_and_refresh_boundaries() {
    for budget in [1, 4096] {
        for (line, effects, dirty, error) in [
            ("setconf", 0, 0, None),
            ("setconf ($7+=1)", 1, 0, None),
            ("setconf ($7+=1) unknown ($7+=1) ($7+=1)", 3, 1, None),
            ("setconf ($7+=1) MESSPD ($7+=1) ($7+=1)", 3, 1, None),
            ("setconf ($7+=1) play 47 1", 1, 1, None),
            ("setconf ($7+=1) sysplay 47 1", 1, 1, None),
            ("setconf ($7+=1) sysplay -1 1", 1, 0, Some("首索引")),
            ("setconf 32 autospd ($7+=1)", 1, 0, Some("预设编号")),
            ("setconf ($7+=1) fshadow -1 ($7+=1)", 2, 0, Some("阴影索引")),
            ("setconf ($7+=1) lang_ui ($7+=1)", 2, 1, None),
        ] {
            let source = format!("#START\n{line}\nwait 999\n");
            let Setup { mut runtime, .. } = setup(budget, source.as_bytes());
            let settings = katze_system2::settings::SystemSettings::default();
            runtime.host_mut().system_settings = Some(settings.clone());
            let fes = runtime.create(0, kind::FES, b"configuration", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            file(&mut runtime, fes, b"complete.fes").unwrap();
            let result = runtime
                .dispatch_command(0, command(fes, 339, 0, 0))
                .and_then(|_| runtime.update(100, 1));
            if let Some(error) = error {
                assert!(result.unwrap_err().message.contains(error));
            } else {
                result.unwrap();
            }
            let mut expected = settings;
            if dirty != 0 {
                expected.refresh_sound_volumes();
            }
            assert_eq!(runtime.host().system_settings.as_ref().unwrap(), &expected);
            assert_eq!(runtime.numbers().shared[7], effects);
            assert_eq!(runtime.host().system_settings_dirty, 0);
        }
    }
}

#[test]
fn shared_image_references_switch_in_handlers_and_release_with_the_fes_task() {
    for image_kind in ["CIMAGE", "IMAGE"] {
        let source = format!(
            "#DEFINE\n{image_kind} item[2]\n#OBJECT SIZE COLOR PRI DISP\nitem 2,2 0xff00ff00 5 1\n#START\nitem refex $0 ($7+=100)\nwait 999\n#SWITCH\nitem refex $1\n#CLEAR\nitem refex\n#REBIND\nitem refex $2\n"
        );
        for budget in [1, 4096] {
            let Setup { mut runtime, .. } = setup(budget, source.as_bytes());
            let list = runtime.create(0, kind::IMAGE_LIST, b"shared", b"").unwrap();
            send(&mut runtime, list, 285, 8, 6);
            let mut images = Vec::new();
            for color in [0xff123456u32, 0xff654321, 0xffaabbcc] {
                let image = runtime.images_mut().create(0, [2, 2]).unwrap();
                for pixel in runtime
                    .images_mut()
                    .get_mut(image)
                    .unwrap()
                    .pixels_mut()
                    .chunks_exact_mut(4)
                {
                    pixel.copy_from_slice(&color.to_le_bytes());
                }
                images.push(image);
            }
            for (slot, &image) in runtime.numbers_mut().shared.iter_mut().zip(&images) {
                *slot = image as i32;
            }
            let fes = runtime.create(0, kind::FES, b"references", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            send(&mut runtime, fes, 340, 0, list);
            file(&mut runtime, fes, b"complete.fes").unwrap();
            send(&mut runtime, fes, 339, 0, 0);
            runtime.update(100, 1).unwrap();
            assert_eq!(runtime.numbers().shared[7], 0);
            runtime.images_mut().release_for(0, images[0]).unwrap();
            assert!(runtime.images().get(images[0]).is_ok());
            let block = |runtime: &mut TestRuntime, text: &[u8]| {
                let mut name = text.to_vec();
                name.push(0);
                runtime
                    .dispatch_command_with_buffers(
                        0,
                        command(fes, 343, 0, 0),
                        &mut CommandBuffers {
                            data1: Some(&mut name),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                send(runtime, list, 298, 0, 0);
            };
            block(&mut runtime, b"SWITCH");
            assert!(runtime.images().get(images[0]).is_err());
            assert_eq!(
                runtime.drawing().target.as_ref().unwrap().pixel(0, 0),
                Some(0xff654321)
            );
            runtime.images_mut().release_for(0, images[1]).unwrap();
            assert!(runtime.images().get(images[1]).is_ok());
            block(&mut runtime, b"CLEAR");
            assert!(runtime.images().get(images[1]).is_err());
            assert_eq!(
                runtime.drawing().target.as_ref().unwrap().pixel(0, 0),
                Some(if image_kind == "CIMAGE" {
                    0xff00ff00
                } else {
                    0
                }),
            );
            block(&mut runtime, b"REBIND");
            runtime.images_mut().release_for(0, images[2]).unwrap();
            assert!(runtime.images().get(images[2]).is_ok());
            runtime.destroy(fes).unwrap();
            assert!(runtime.images().get(images[2]).is_err());
            assert!(runtime.object(list).is_some());
        }
    }
}

#[test]
fn destroying_a_fes_during_draw_restores_submission_and_releases_its_sources() {
    let source = b"#DEFINE\nCIMAGE sprite\nPLANE panel\n#OBJECT SIZE COLOR PRI DISP\nsprite 8,6 0xff123456 0 1\n#OBJECT SIZE DISP VRAM\npanel 8,6 0 0,0\n#START\npanel draw\nwait 1000\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        let list = runtime.create(0, kind::IMAGE_LIST, b"shared", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, list, 289, 0, 0);
        let fes = runtime.create(0, kind::FES, b"transition", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        send(&mut runtime, fes, 340, 0, list);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        runtime.update(100, 1).unwrap();
        assert!(
            runtime
                .tree()
                .children(fes)
                .unwrap()
                .any(|id| { runtime.tree().node(id).unwrap().kind == kind::FADE })
        );
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(0, 0),
            Some(0)
        );
        runtime.destroy(fes).unwrap();
        assert!(runtime.object(fes).is_none());
        assert!(runtime.object(list).is_some());
        runtime.update(101, 1).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(0, 0),
            Some(0)
        );
        assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
    }
}

#[test]
fn fes_tasks_load_without_starting_then_drive_pixels_and_keep_exit_properties() {
    let source = b"#DEFINE\nCIMAGE sprite\nPLANE view\n#OBJECT SIZE COLOR PRI POS DISP\nsprite 2,2 0xff123456 5 1,1 1\n#OBJECT POS SIZE VRAM PRI DISP\nview 0,0 8,6 0,0 500 1\n#START\n$0=$0+1\nwait 2\n$1=$1+1\nexit 37\n";
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            queries,
            reads,
            ..
        } = setup(budget, source);
        runtime.update(100, 7).unwrap();
        let fes = runtime
            .create(0, kind::FES, b"ignored task name", b"original")
            .unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        assert_eq!(
            runtime.object(fes).unwrap().creation_context(),
            Some(b"".as_slice())
        );
        let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, list, 289, 1, 0);
        send(&mut runtime, fes, 340, 0, list);
        file(&mut runtime, fes, b"complete.fes\0").unwrap();
        assert_eq!(runtime.numbers().shared[0], 0);
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 1);
        queries.borrow_mut().clear();
        send(&mut runtime, fes, 339, 0, 0);
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(runtime.numbers().shared[1], 0);
        assert_eq!(
            *queries.borrow(),
            [
                (12, 0, false),
                (12, 1, false),
                (12, 2, false),
                (12, 3, false)
            ]
        );
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 3);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(2, 1),
            Some(0)
        );
        runtime.update(101, 11).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(2, 1),
            Some(0xff123456)
        );
        let mut output = RenderTarget::new([8, 6]).unwrap();
        runtime
            .render_frame(
                &mut output,
                &Projection::default(),
                *Transform::default().matrix(),
                TextureFilter::Point,
            )
            .unwrap();
        assert_eq!(output.pixel(1, 1), Some(0xff123456));
        send(&mut runtime, fes, 29, 0, 1);
        runtime.update(102, 1).unwrap();
        assert_eq!(runtime.object(fes).unwrap().clock().time, 101);
        assert_eq!(runtime.numbers().shared[1], 1);
        assert_eq!(send(&mut runtime, fes, 23, 0, 0), 37);
        send(&mut runtime, fes, 24, 0, 77);
        runtime.update(103, 1).unwrap();
        assert_eq!(send(&mut runtime, fes, 23, 0, 0), 77);
        assert_eq!(runtime.numbers().shared[0..2], [2, 1]);
        assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
        runtime.destroy(fes).unwrap();
        assert!(runtime.object(fes).is_none());
        assert!(runtime.object(list).is_none());
        assert_eq!(runtime.drawables_mut().order().len(), 0);
        assert!(runtime.update(104, 1).unwrap());
    }
}

#[test]
fn fes_release_removes_only_its_entries_from_an_ancestor_image_list() {
    let source = b"#DEFINE\nCIMAGE sprite\n#OBJECT SIZE COLOR PRI POS DISP\nsprite 2,2 0xff123456 5 1,1 1\n#START\nwait 1000\n";
    for budget in [1, 4096] {
        let mut runtime = setup(budget, source).runtime;
        let list = runtime
            .create(0, kind::IMAGE_LIST, b"shared list", b"")
            .unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, list, 292, 0, 1);
        let mut scenes = Vec::new();
        for _ in 0..2 {
            let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            send(&mut runtime, fes, 340, 0, list);
            file(&mut runtime, fes, b"complete.fes").unwrap();
            send(&mut runtime, fes, 339, 0, 0);
            scenes.push(fes);
        }
        runtime.update(100, 1).unwrap();
        assert_eq!(send(&mut runtime, list, 294, 0, 0), 2);
        runtime.destroy(scenes[0]).unwrap();
        assert_eq!(send(&mut runtime, list, 294, 0, 0), 1);
        runtime.update(101, 1).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0xff123456)
        );
        runtime.destroy(scenes[1]).unwrap();
        assert_eq!(send(&mut runtime, list, 294, 0, 0), 0);
        runtime.update(102, 1).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0)
        );
    }
}

#[test]
fn ordinary_natives_bind_load_and_start_fes_with_shared_values_and_pixels() {
    let source = b"#DEFINE\nCIMAGE sprite\nPLANE view\n#OBJECT SIZE COLOR PRI POS DISP\nsprite 2,2 0xff123456 5 1,1 1\n#OBJECT POS SIZE VRAM PRI DISP\nview 0,0 8,6 0,0 500 1\n#START\n$0=$0+1\n$1=\\2\nexit 37\nwait 20\n#BLOCK7\n$2+=1\n$3=@\n#sprite.BLOCK7\n$4=@\n";
    for budget in [1, 4096] {
        for empty_name in [false, true] {
            let Setup {
                mut runtime, reads, ..
            } = setup(budget, source);
            let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
            let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
            assert_eq!((fes, list), (3, 4));
            send(&mut runtime, list, 285, 8, 6);
            send(&mut runtime, list, 289, 1, 0);
            let caller = runtime
                .create(0, kind::ORDINARY, b"native driver", b"")
                .unwrap();
            if empty_name {
                runtime
                    .object_mut(caller)
                    .unwrap()
                    .interpreter_mut()
                    .unwrap()
                    .write(64, &[0])
                    .unwrap();
            }
            runtime.update(0, 1).unwrap();
            assert_eq!(runtime.numbers().shared[0], 1);
            assert_eq!(runtime.numbers().shared[1], 17);
            assert_eq!(
                runtime.numbers().shared[2..5],
                [1, 0, 0x8000_0001u32 as i32]
            );
            assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
            let vm = runtime.object(caller).unwrap().interpreter().unwrap();
            assert_eq!(vm.read(0, 4).unwrap(), 0xdead_beefu32.to_le_bytes());
            for (address, expected) in [(4, 37u32), (8, u32::MAX), (12, u32::MAX)] {
                assert_eq!(vm.read(address, 4).unwrap(), expected.to_le_bytes());
            }
            for (address, expected) in [(16, 17u32), (20, 0x8000_0001), (24, 0), (28, 0)] {
                assert_eq!(vm.read(address, 4).unwrap(), expected.to_le_bytes());
            }
            for (address, expected) in [(36, 0x8000_0001u32), (40, 9), (44, 19), (48, 33), (52, 0)]
            {
                assert_eq!(vm.read(address, 4).unwrap(), expected.to_le_bytes());
            }
            assert!(vm.stack_bytes().is_empty());
            runtime.update(1, 1).unwrap();
            let mut output = RenderTarget::new([8, 6]).unwrap();
            runtime
                .render_frame(
                    &mut output,
                    &Projection::default(),
                    *Transform::default().matrix(),
                    TextureFilter::Point,
                )
                .unwrap();
            assert_eq!(output.pixel(1, 1), Some(0xff123456));
            assert_eq!(runtime.numbers().shared[0], 1);
        }
    }
}

#[test]
fn named_values_preserve_raw_keys_and_survive_script_loading() {
    fn value(rt: &mut TestRuntime, target: u32, key: &[u8], value: Option<u32>) -> u32 {
        let mut key = key.to_vec();
        key.push(0);
        let mut result = [0x55; 8];
        assert_eq!(
            rt.dispatch_command_with_buffers(
                0,
                command(
                    target,
                    if value.is_some() { 358 } else { 357 },
                    0,
                    value.unwrap_or(0)
                ),
                &mut CommandBuffers {
                    data0: Some(&mut key),
                    data1: Some(&mut result),
                    external: None,
                },
            )
            .unwrap(),
            0,
        );
        assert_eq!(&result[4..], &[0x55; 4]);
        u32::from_le_bytes(result[..4].try_into().unwrap())
    }

    for budget in [1, 4096] {
        let mut setup = setup(budget, b"#START\nexit 3\n");
        let rt = &mut setup.runtime;
        let target = rt.create(0, kind::FES, b"values", b"").unwrap();
        assert_eq!(send(rt, target, 357, 1, 1), 0);
        assert_eq!(send(rt, target, 358, 1, 77), 0);
        send(rt, target, 337, 0, 0);
        for (key, n) in [
            (b"".as_slice(), 17),
            (b"raw\xff", 0x8000_0001),
            (b"RAW\xff", 39),
        ] {
            value(rt, target, key, Some(n));
            assert_eq!(value(rt, target, key, None), n);
        }
        value(rt, target, b"raw\xff\0ignored", Some(79));
        file(rt, target, b"complete.fes\0").unwrap();
        assert_eq!(value(rt, target, b"raw\xff", None), 79);
        assert_eq!(value(rt, target, b"RAW\xff", None), 39);
        assert_eq!(value(rt, target, b"", None), 17);
        assert_eq!(value(rt, target, b"missing", None), 0);
        let other = rt.create(0, kind::FES, b"other", b"").unwrap();
        send(rt, other, 337, 0, 0);
        assert_eq!(value(rt, other, b"raw\xff", None), 0);
    }
}

fn parent_scene(setup: &mut Setup, source: &[u8], child: &[u8]) -> (u32, u32) {
    setup.runtime.set_resources(Files {
        files: BTreeMap::from([
            (b"complete.fes".to_vec(), packed(source)),
            (b"child.fes".to_vec(), packed(child)),
        ]),
        reads: setup.reads.clone(),
        fail_media: setup.fail_media.clone(),
    });
    let fes = setup.runtime.create(0, kind::FES, b"parent", b"").unwrap();
    send(&mut setup.runtime, fes, 337, 0, 0);
    setup
        .runtime
        .object_mut(fes)
        .unwrap()
        .set_creation_context(b"parent context")
        .unwrap();
    let list = setup
        .runtime
        .create(fes, kind::IMAGE_LIST, b"list", b"")
        .unwrap();
    send(&mut setup.runtime, list, 285, 8, 6);
    send(&mut setup.runtime, list, 289, 1, 0);
    send(&mut setup.runtime, fes, 340, 0, list);
    file(&mut setup.runtime, fes, b"complete.fes").unwrap();
    (fes, list)
}

#[test]
fn named_blocks_refresh_media_respect_pause_and_keep_main_wait_and_exit_notifications() {
    let source = b"#DEFINE\nCIMAGE sprite\nPLANE view\n#OBJECT SIZE COLOR PRI POS DISP\nsprite 2,2 0xff123456 5 1,1 1\n#OBJECT POS SIZE VRAM PRI DISP\nview 0,0 8,6 0,0 500 1\n#START\n$0+=1\nwait 100\n$1+=1\n#sprite.MOVE\n$2=@\n@ disp 0\ndecode\n$3=999\n#sprite.SHOW\n@ disp 1\n#sprite.WAIT\n$4+=1\nwait 5\n$4=999\n#sprite.EXIT\nexit 9\n";
    for budget in [1, 4096] {
        for paused in [false, true] {
            let mut setup = setup(budget, source);
            let (fes, list) = parent_scene(&mut setup, source, b"#START\n");
            let runtime = &mut setup.runtime;
            send(runtime, fes, 339, 0, 0);
            runtime.update(10, 1).unwrap();
            if paused {
                send(runtime, fes, 348, 0, 1);
            }
            let block = |runtime: &mut TestRuntime, text: &[u8]| {
                let mut name = text.to_vec();
                name.push(0);
                let mut context = [0; 36];
                context[..7].copy_from_slice(b"sprite\0");
                runtime
                    .dispatch_command_with_buffers(
                        0,
                        command(fes, 343, 0, 0),
                        &mut CommandBuffers {
                            data0: Some(&mut context),
                            data1: Some(&mut name),
                            external: None,
                        },
                    )
                    .unwrap();
            };
            block(runtime, b"MOVE");
            send(runtime, list, 298, 0, 0);
            let mut output = RenderTarget::new([8, 6]).unwrap();
            runtime
                .render_frame(
                    &mut output,
                    &Projection::default(),
                    *Transform::default().matrix(),
                    TextureFilter::Point,
                )
                .unwrap();
            assert_eq!(output.pixel(1, 1), Some(0));
            block(runtime, b"SHOW");
            block(runtime, b"WAIT");
            block(runtime, b"WAIT");
            block(runtime, b"");
            block(runtime, b"missing");
            assert_eq!(runtime.numbers().shared[..5], [1, 0, 0, 0, 2]);
            send(runtime, list, 298, 0, 0);
            runtime
                .render_frame(
                    &mut output,
                    &Projection::default(),
                    *Transform::default().matrix(),
                    TextureFilter::Point,
                )
                .unwrap();
            assert_eq!(
                output.pixel(1, 1),
                Some(if paused { 0 } else { 0xff123456 })
            );
            block(runtime, b"EXIT");
            assert_eq!(send(runtime, fes, 23, 0, 0), u32::MAX);
            if paused {
                runtime.update(11, 1).unwrap();
                assert_eq!(send(runtime, fes, 23, 0, 0), u32::MAX);
                send(runtime, fes, 348, 0, 0);
            }
            runtime.update(12, 1).unwrap();
            assert_eq!(send(runtime, fes, 23, 0, 0), 9);
            assert_eq!(runtime.numbers().shared[..5], [1, 0, 0, 0, 2]);
            runtime
                .render_frame(
                    &mut output,
                    &Projection::default(),
                    *Transform::default().matrix(),
                    TextureFilter::Point,
                )
                .unwrap();
            assert_eq!(output.pixel(1, 1), Some(0xff123456));
            runtime.update(100, 1).unwrap();
            assert_eq!(runtime.numbers().shared[1], 1);
        }
    }
}

fn child_scenes(runtime: &TestRuntime, parent: u32) -> Vec<u32> {
    runtime
        .tree()
        .children(parent)
        .unwrap()
        .filter(|&id| runtime.tree().node(id).unwrap().kind == kind::FES)
        .collect()
}

#[test]
fn child_fes_retries_loading_without_recreating_or_reevaluating_and_shares_pixels_and_exit_code() {
    let parent = b"#START\nexecfes child ($0+=1) 9 2 3 4 5 6 7 8 9 10 11 12 13 14 15 ($7+=100)\n$1+=1\nwait 1\n$2=_EXITCODE_\nendfes\n$3+=1\nwait 100\n";
    let child = b"#DEFINE\nCIMAGE sprite\nPLANE view\n#OBJECT SIZE COLOR PRI POS DISP\nsprite 2,2 0xff123456 5 1,1 1\n#OBJECT POS SIZE VRAM PRI DISP\nview 0,0 8,6 0,0 500 1\n#START\n$4+=1\n$5=\\0+\\1+\\15\nexit 37\nwait 100\n";
    for budget in [1, 4096] {
        let mut setup = setup(budget, parent);
        let (fes, list) = parent_scene(&mut setup, parent, child);
        setup.fail_media.set(true);
        assert!(
            setup
                .runtime
                .dispatch_command(0, command(fes, 339, 0, 0))
                .is_err()
        );
        assert_eq!(setup.runtime.numbers().shared[..6], [1, 0, 0, 0, 0, 0]);
        let children = child_scenes(&setup.runtime, fes);
        assert_eq!(children.len(), 1);
        let child = children[0];
        assert_eq!(
            setup.runtime.object(child).unwrap().creation_context(),
            Some(b"parent context".as_slice())
        );
        for (index, expected) in [(0, 1), (1, 9), (15, 15), (511, 0), (512, 0), (u32::MAX, 0)] {
            assert_eq!(send(&mut setup.runtime, child, 347, index, 0), expected);
        }
        setup.fail_media.set(false);
        send(&mut setup.runtime, fes, 339, 0, 0);
        assert_eq!(child_scenes(&setup.runtime, fes), children);
        assert_eq!(setup.runtime.numbers().shared[..6], [1, 1, 0, 0, 1, 25]);
        assert_eq!(setup.runtime.numbers().shared[7], 0);
        assert_eq!(send(&mut setup.runtime, child, 347, 0, 0), 1);
        send(&mut setup.runtime, child, 346, 511, 0x8000_0001);
        send(&mut setup.runtime, child, 346, 512, 0x1234_5678);
        assert_eq!(send(&mut setup.runtime, child, 347, 511, 0), 0x8000_0001);
        assert_eq!(send(&mut setup.runtime, child, 347, 512, 0), 0);
        assert_eq!(send(&mut setup.runtime, child, 23, 0, 0), 37);
        setup.runtime.update(1, 1).unwrap();
        let mut output = RenderTarget::new([8, 6]).unwrap();
        setup
            .runtime
            .render_frame(
                &mut output,
                &Projection::default(),
                *Transform::default().matrix(),
                TextureFilter::Point,
            )
            .unwrap();
        assert_eq!(output.pixel(1, 1), Some(0xff123456));
        assert_eq!(setup.runtime.numbers().shared[..4], [1, 1, 37, 0]);
        assert_eq!(
            setup
                .runtime
                .object(child)
                .unwrap()
                .control()
                .deletion_requested,
            1
        );
        assert!(setup.runtime.object(list).is_some());
        setup.runtime.update(2, 1).unwrap();
        assert!(setup.runtime.object(child).is_none());
        assert_eq!(setup.runtime.numbers().shared[3], 1);
        assert!(setup.runtime.object(list).is_some());
        assert_eq!(
            *setup.reads.borrow(),
            [
                b"complete.fes".to_vec(),
                b"child.fes".to_vec(),
                b"child.fes".to_vec()
            ]
        );
    }
}

#[test]
fn child_script_ends_yield_without_a_child_and_handler_requests_wait_through_pause() {
    let parent = b"#START\nendfes ignored ($7+=100)\n$0+=1\nendkcs\n$0+=10\nexecfes child 4\nexecfes child ($6+=1)\nwait 100\n#_FES_SLEEP_\nendfes\n$1+=1\n#_FES_WAKE_\nexecfes child ($6+=10)\n$1+=100\n";
    let child = b"#START\n$4+=1\nwait 1000\n";
    for budget in [1, 4096] {
        let mut setup = setup(budget, parent);
        let (fes, _) = parent_scene(&mut setup, parent, child);
        send(&mut setup.runtime, fes, 339, 0, 0);
        assert_eq!(setup.runtime.numbers().shared[0], 0);
        setup.runtime.update(1, 1).unwrap();
        assert_eq!(setup.runtime.numbers().shared[0], 1);
        setup.runtime.update(2, 1).unwrap();
        assert_eq!(setup.runtime.numbers().shared[0], 11);
        assert_eq!(setup.runtime.numbers().shared[4], 1);
        assert_eq!(setup.runtime.numbers().shared[6..8], [1, 0]);
        let children = child_scenes(&setup.runtime, fes);
        assert_eq!(children.len(), 1);
        let child = children[0];
        send(&mut setup.runtime, fes, 348, 0, 1);
        assert_eq!(setup.runtime.numbers().shared[1], 0);
        assert_eq!(
            setup
                .runtime
                .object(child)
                .unwrap()
                .control()
                .deletion_requested,
            0
        );
        setup.runtime.update(3, 1).unwrap();
        assert_eq!(
            setup
                .runtime
                .object(child)
                .unwrap()
                .control()
                .deletion_requested,
            0
        );
        send(&mut setup.runtime, fes, 348, 0, 0);
        setup.runtime.update(4, 1).unwrap();
        assert_eq!(
            setup
                .runtime
                .object(child)
                .unwrap()
                .control()
                .deletion_requested,
            1
        );
        assert_eq!(setup.runtime.numbers().shared[1], 0);
        assert_eq!(setup.runtime.numbers().shared[4], 2);
        assert_eq!(setup.runtime.numbers().shared[6], 11);
        let next = child_scenes(&setup.runtime, fes);
        assert_eq!(next.len(), 2);
        setup.runtime.update(5, 1).unwrap();
        assert!(setup.runtime.object(child).is_none());
        assert_eq!(child_scenes(&setup.runtime, fes).len(), 1);
    }
}

struct Driver {
    base: BaseObject,
    id: u32,
    child: Rc<Cell<Option<u32>>>,
    context: Vec<u8>,
    delete_parent: bool,
}
impl TaskObject for Driver {
    fn control(&self) -> &Control {
        self.base.control()
    }
    fn clock(&self) -> &TaskClock {
        self.base.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.base.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn creation_context(&self) -> Option<&[u8]> {
        Some(&self.context)
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.context = context.to_vec();
        Ok(())
    }
    fn update_body(&mut self, host: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        if self.delete_parent {
            let parent = host.parent_task_id()?;
            host.dispatch_task_command(
                Command {
                    sender: self.id,
                    ..command(parent, 3, 0, 0)
                },
                &mut CommandBuffers::default(),
                &mut 0,
            )?;
            return Ok(true);
        }
        if self.child.get().is_some() {
            return Ok(true);
        }
        self.context = b"updated parent".to_vec();
        host.set_task_id(self.id);
        host.set_task_clock(&self.base.clock);
        host.set_task_context(&self.context);
        let child = host.create_task_child(kind::FES, b"nested", b"passed")?;
        self.child.set(Some(child));
        let mut file = b"complete.fes\0".to_vec();
        for code in [337, 338, 339] {
            let mut output = 0x13579bdf;
            let mut buffers = CommandBuffers {
                data0: None,
                data1: if code == 338 { Some(&mut file) } else { None },
                external: None,
            };
            assert!(host.dispatch_task_command(
                Command {
                    sender: self.id,
                    ..command(child, code, 0, 0)
                },
                &mut buffers,
                &mut output
            )?);
            assert_eq!(output, 0x13579bdf);
        }
        Ok(true)
    }
    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.base
            .dispatch_command(kind, command, output, host, budget)
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.base.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.base.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.base.request_delete(host, budget)
    }
}

#[test]
fn fes_start_reads_the_running_parent_elapsed_clock_and_current_context() {
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            child,
            reads,
            ..
        } = setup(budget, b"#START\n$0=_FRAME_\n$1=$1+1\nwait 3\nexit 13\n");
        let parent = runtime
            .create(0, kind::BASE, b"driver", b"old parent")
            .unwrap();
        runtime.update(500, 9).unwrap();
        let child = child.get().unwrap();
        assert_eq!(runtime.tree().node(child).unwrap().parent, Some(parent));
        assert_eq!(runtime.object(child).unwrap().clock().time, 100);
        assert_eq!(
            runtime.object(child).unwrap().creation_context(),
            Some(b"updated parent".as_slice())
        );
        assert_eq!(runtime.numbers().shared[0..2], [1, 1]);
        runtime.update(501, 3).unwrap();
        assert_eq!(send(&mut runtime, child, 23, 0, 0), 13);
        runtime.update(503, 1).unwrap();
        assert_eq!(send(&mut runtime, child, 23, 0, 0), 13);
        assert_eq!(runtime.numbers().shared[0..2], [404, 2]);
        assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
    }
}

#[test]
fn failed_loads_preserve_the_task_and_input_modes_publish_system_values() {
    let Setup {
        mut runtime,
        queries,
        mask,
        reads,
        ..
    } = setup(
        1,
        b"#START\n$0=$0+(_TOPOBJ_!=0)\n$2=_CLICK_L_\nwait 1000\n$3=_CLICK_L_\nwait 1\n$4=_CLICK_L_\nexit 3\n",
    );
    let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
    send(&mut runtime, fes, 337, 0, 0);
    file(&mut runtime, fes, b"missing").unwrap();
    assert_eq!(send(&mut runtime, fes, 23, 0, 0), (-2i32) as u32);
    file(&mut runtime, fes, b"complete.fes").unwrap();
    mask.set(true);
    send(&mut runtime, 1, 151, 12, 0);
    queries.borrow_mut().clear();
    assert_eq!(send(&mut runtime, 1, 145, 12, u32::MAX), 0x8000000a);
    assert_eq!(
        *queries.borrow(),
        [
            (12, 0, false),
            (12, 1, false),
            (12, 2, false),
            (12, 3, false)
        ]
    );
    send(&mut runtime, fes, 339, 0, 0);
    assert_eq!(runtime.numbers().shared[0], 1);
    assert_eq!(runtime.numbers().shared[2], -1);
    runtime.update(1, 1).unwrap();
    assert_eq!(runtime.numbers().shared[0], 1);
    assert_eq!(runtime.numbers().shared[3], -1);
    mask.set(false);
    runtime.update(2, 1).unwrap();
    assert_eq!(runtime.numbers().shared[4], 0);
    assert_eq!(send(&mut runtime, fes, 23, 0, 0), 3);
    assert_eq!(
        *reads.borrow(),
        [b"missing".to_vec(), b"complete.fes".to_vec()]
    );
    assert!(runtime.object(fes).is_some());
}

#[test]
fn immediate_child_updates_can_delete_the_running_fes_owner_before_start() {
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            queries,
            ..
        } = setup(budget, b"#START\n$0=$0+1\nexit 3\n");
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        let child = runtime
            .create(fes, kind::BASE, b"delete parent", b"")
            .unwrap();
        queries.borrow_mut().clear();
        send(&mut runtime, fes, 339, 0, 0);
        assert_eq!(runtime.numbers().shared[0], 0);
        assert!(queries.borrow().is_empty());
        assert_eq!(runtime.object(fes).unwrap().control().deletion_requested, 1);
        assert!(runtime.update(1, 1).unwrap());
        assert!(runtime.object(fes).is_none());
        assert!(runtime.object(child).is_none());
    }
}

#[test]
fn empty_fes_contexts_start_preserve_failed_properties_and_load_later_without_resetting_time() {
    for budget in [1, 4096] {
        for name in [
            None,
            Some(b"".as_slice()),
            Some(b"missing.fes"),
            Some(b"empty.fes"),
            Some(b"nul.fes"),
            Some(b"other.fes"),
            Some(b"start.fes"),
            Some(b"normal.fes"),
        ] {
            let Setup {
                mut runtime,
                reads,
                fail_media,
                button_attempts,
                ..
            } = setup(budget, b"#START\n$0=\\0\n$1=_FRAME_\nexit 37\n");
            runtime.set_resources(Files {
                files: BTreeMap::from([
                    (
                        b"complete.fes".to_vec(),
                        packed(b"#START\n$0=\\0\n$1=_FRAME_\nexit 37\n"),
                    ),
                    (b"empty.fes".to_vec(), Vec::new()),
                    (b"nul.fes".to_vec(), vec![0]),
                    (b"other.fes".to_vec(), packed(b"#UNUSED\nexit 23\n")),
                    (b"start.fes".to_vec(), packed(b"#START\n")),
                    (b"normal.fes".to_vec(), packed(b"#START\nexit 37\n")),
                ]),
                reads,
                fail_media,
            });
            runtime.update(100, 1).unwrap();
            let fes = runtime.create(0, kind::FES, b"empty", b"").unwrap();
            assert!(
                runtime
                    .dispatch_command(0, command(fes, 339, 0, 0))
                    .is_err()
            );
            send(&mut runtime, fes, 337, 0, 0);
            if let Some(name) = name {
                file(&mut runtime, fes, name).unwrap();
            }
            send(&mut runtime, fes, 346, 0, 17);
            send(&mut runtime, fes, 346, 511, 0x8000_0001);
            let missing = matches!(name, Some(b"" | b"missing.fes"));
            let normal = name == Some(b"normal.fes".as_slice());
            let expected = if normal {
                37
            } else if missing {
                (-2i32) as u32
            } else {
                u32::MAX
            };
            send(&mut runtime, fes, 339, 0, 0);
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), expected);
            runtime.update(500, 1).unwrap();
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), expected);
            assert_eq!(send(&mut runtime, fes, 347, 511, 0), 0x8000_0001);
            assert_eq!(button_attempts.get(), 1);
            if name.is_none() || missing {
                file(&mut runtime, fes, b"complete.fes").unwrap();
                assert_eq!(send(&mut runtime, fes, 23, 0, 0), expected);
                send(&mut runtime, fes, 339, 0, 0);
                assert_eq!(runtime.numbers().shared[0], 0);
                runtime.update(700, 1).unwrap();
                assert_eq!(runtime.numbers().shared[..2], [17, 601]);
                assert_eq!(send(&mut runtime, fes, 23, 0, 0), 37);
                assert_eq!(send(&mut runtime, fes, 347, 511, 0), 0x8000_0001);
                assert_eq!(button_attempts.get(), 1);
            }
        }
    }
}

#[test]
fn first_update_creates_one_button_before_loading_or_starting_the_scene() {
    for depth in [1, 3, 5] {
        let Setup {
            mut runtime,
            button_attempts,
            ..
        } = setup(1, b"#START\nexit 0\n");
        let mut parent = 0;
        for _ in 1..depth {
            parent = runtime
                .create(parent, kind::BASE, b"ancestor", b"")
                .unwrap();
        }
        let fes = runtime.create(parent, kind::FES, b"unloaded", b"").unwrap();
        runtime.input_coordinates_mut().logical_size = [800, 600];
        runtime.host_mut().surface_size = [320, 240];
        runtime.update(123, 1).unwrap();
        let children: Vec<_> = runtime.tree().children(fes).unwrap().collect();
        assert_eq!(children.len(), 1);
        let button = children[0];
        assert_eq!(runtime.tree().node(button).unwrap().kind, kind::BUTTON);
        assert_eq!(runtime.object(button).unwrap().clock().time, 0);
        assert!(runtime.drawables().get(button).is_none());
        let region = runtime
            .input_state(1)
            .unwrap()
            .regions()
            .get(button)
            .unwrap();
        assert_eq!(region.priority(), depth);
        assert_eq!(region.event_mask(), 7);
        assert_eq!(
            (
                region.bounds().left,
                region.bounds().top,
                region.bounds().right,
                region.bounds().bottom
            ),
            (0, 0, 320, 240)
        );
        assert_eq!(region.enabled(), 1);
        runtime.host_mut().surface_size = [640, 480];
        runtime.update(124, 1).unwrap();
        assert_eq!(button_attempts.get(), 1);
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 1);
        assert_eq!(runtime.object(button).unwrap().clock().time, 124);
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(button)
                .unwrap()
                .bounds()
                .right,
            320
        );
        send(&mut runtime, button, 3, 0, 0);
        runtime.update(125, 1).unwrap();
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 0);
        assert_eq!(button_attempts.get(), 1);
    }
}

#[test]
fn failed_first_button_creation_is_reported_once_and_does_not_reserve_an_id() {
    let Setup {
        mut runtime,
        fail_button,
        button_attempts,
        ..
    } = setup(1, b"#START\nexit 0\n");
    let fes = runtime.create(0, kind::FES, b"unloaded", b"").unwrap();
    fail_button.set(true);
    assert!(
        runtime
            .update(1, 1)
            .unwrap_err()
            .message
            .contains("按钮创建失败")
    );
    assert_eq!(button_attempts.get(), 1);
    assert_eq!(runtime.tree().children(fes).unwrap().count(), 0);
    fail_button.set(false);
    runtime.update(2, 1).unwrap();
    assert_eq!(button_attempts.get(), 1);
    assert_eq!(runtime.tree().children(fes).unwrap().count(), 0);
    assert_eq!(
        runtime.create(0, kind::BASE, b"unused", b"").unwrap(),
        fes + 1
    );
}

#[test]
fn auxiliary_input_reaches_script_values_and_transients_clear_after_the_round() {
    for budget in [1, 4096] {
        let Setup { mut runtime, pointer, .. } = setup(budget, b"#START\nwait 1000\n$0=_CLICK_L_\n$2=_MOUSE_X_\n$3=_MOUSE_Y_\n$4=_SCRL_U_\nwait 1\n$1=_CLICK_L_\n$5=_SCRL_U_\n$6=_MOUSE_X_\n$7=_MOUSE_Y_\nwait 20\n");
        pointer.set(PointerSample {
            position: [2, 3],
            active: 1,
        });
        runtime.input_coordinates_mut().logical_size = [8, 6];
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        runtime.update(100, 1).unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        let button = runtime
            .tree()
            .children(fes)
            .unwrap()
            .find(|id| runtime.tree().node(*id).unwrap().kind == kind::BUTTON)
            .unwrap();
        runtime.update_input_selection(1, Some([2, 3])).unwrap();
        assert!(
            !runtime
                .dispatch_input_key(
                    1,
                    KeyEvent {
                        kind: KeyEventKind::Change,
                        code: 4,
                        key: 0,
                        pressed: 1,
                        mask: 2,
                        point: [2, 3],
                    }
                )
                .unwrap()
        );
        assert_eq!(send(&mut runtime, button, 4, 8, 1), 1);
        runtime.update(101, 1).unwrap();
        assert_eq!(runtime.numbers().shared[0], -1);
        assert_eq!(&runtime.numbers().shared[2..5], &[2, 3, -1]);
        pointer.set(PointerSample::default());
        runtime.update(102, 1).unwrap();
        assert_eq!(
            (runtime.numbers().shared[1], runtime.numbers().shared[5]),
            (0, 0)
        );
        assert_eq!(&runtime.numbers().shared[6..8], &[i32::MIN; 2]);
        assert_eq!(send(&mut runtime, button, 4, 0, 0), 1);
        assert_eq!(send(&mut runtime, fes, 64, button, 1), 0);
        assert_eq!(send(&mut runtime, fes, 65, button, 1), 0);
        assert_eq!(send(&mut runtime, button, 4, 4, 1), 1);
    }
}

#[test]
fn button_load_creates_each_declaration_before_start_and_rebinding_keeps_the_tasks() {
    let source = b"#DEFINE\nBUTTON item[2]\nBUTTON item[3]\n#OBJECT SIZE PRI ENABLE\nitem 2,2 5 21\nitem[2] 2,2 7 21\n#START\n$0=$0+1\nwait 99\n#item.FOCUS\n$6=$6+@+1\n";
    for budget in [1, 4096] {
        let Setup {
            mut runtime, reads, ..
        } = setup(budget, source);
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        let buttons: Vec<_> = runtime.tree().children(fes).unwrap().collect();
        assert_eq!(buttons.len(), 5);
        for &id in &buttons {
            assert_eq!(runtime.tree().node(id).unwrap().kind, kind::BUTTON);
            assert!(runtime.input_state(1).unwrap().regions().get(id).is_none());
        }
        assert_eq!(runtime.numbers().shared[0], 0);
        let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, fes, 340, 0, list);
        send(&mut runtime, fes, 339, 0, 0);
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
        for (index, &id) in buttons.iter().enumerate() {
            let region = runtime.input_state(1).unwrap().regions().get(id);
            if index < 2 {
                assert!(region.is_none());
            } else {
                assert_eq!(region.unwrap().enabled(), 21);
            }
        }
        assert_eq!(runtime.tree().children(fes).unwrap().count(), 7);
        for button in buttons {
            assert_eq!(send(&mut runtime, fes, 64, button, 1), 0);
        }
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(runtime.numbers().shared[6], 9);
        runtime.destroy(fes).unwrap();
        assert!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .records()
                .is_empty()
        );
    }
}

fn start_button_scene(runtime: &mut TestRuntime) -> (u32, u32, Vec<u32>) {
    runtime.update(100, 1).unwrap();
    let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
    send(runtime, fes, 337, 0, 0);
    file(runtime, fes, b"complete.fes").unwrap();
    let buttons = runtime.tree().children(fes).unwrap().collect();
    let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
    send(runtime, list, 285, 8, 6);
    send(runtime, list, 289, 0, 0);
    send(runtime, fes, 340, 0, list);
    send(runtime, fes, 339, 0, 0);
    runtime.update(101, 1).unwrap();
    (fes, list, buttons)
}

#[test]
fn system_keys_scan_live_records_without_replacing_focus_or_release_targets() {
    let source = br"#DEFINE
BUTTON first
BUTTON skipped
BUTTON first
BUTTON last
BUTTON focus[2]
#OBJECT SIZE PRI ENABLE SYSKEY
first 2,2 5 21 12
skipped 2,2 6 1 12
last 2,2 7 0 12
focus[0] 2,2 8 1 -1
focus[1] 2,2 9 1 -2
#START
wait 1000
#first.PUSH_L
$0=$0*10+1
skipped enable 0
last enable 1
#skipped.PUSH_L
$0=$0*10+2
#last.PUSH_L
$0=$0*10+3
#focus.PUSH_L
$2=$2*10+@+1
#focus.RELEASE_L
$3=$3*10+@+1
";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        let (fes, _, buttons) = start_button_scene(&mut runtime);
        send(&mut runtime, fes, 64, buttons[4], 1);
        assert_eq!(send(&mut runtime, buttons[4], 4, 12, 1), 0);
        assert_eq!(runtime.numbers().shared[0], 113);
        assert_eq!(runtime.numbers().shared[2], 0);
        assert_eq!(send(&mut runtime, buttons[4], 4, 0, 0), 0);
        assert_eq!(runtime.numbers().shared[3], 1);
        assert_eq!(send(&mut runtime, buttons[4], 4, 0, 1), 0);
        assert_eq!(runtime.numbers().shared[2], 1);
        // 新焦点不改写已经保存的释放目标。
        send(&mut runtime, fes, 64, buttons[5], 1);
        assert_eq!(send(&mut runtime, buttons[4], 4, 0, 0), 0);
        assert_eq!(runtime.numbers().shared[3], 11);
        assert_eq!(send(&mut runtime, buttons[4], 4, 0, 1), 0);
        assert_eq!(runtime.numbers().shared[2], 12);
        assert_eq!(runtime.numbers().shared[0], 113);

        assert_eq!(send(&mut runtime, buttons[4], 4, u32::MAX - 1, 1), 0);
        assert_eq!(runtime.numbers().shared[2], 122);
        assert_eq!(send(&mut runtime, buttons[4], 4, u32::MAX, 1), 1);
        assert_eq!(runtime.numbers().shared[2], 122);
        assert_eq!(send(&mut runtime, buttons[4], 4, 12, 0), 1);
        assert_eq!(runtime.numbers().shared[0], 113);
    }
}

#[test]
fn system_key_retries_keep_completed_matches_and_default_only_responses() {
    for decode in ["", "decode\n$2=$2+1\n"] {
        let source = format!(
            "#DEFINE\nBUTTON clean\nBUTTON dirty\n#OBJECT SIZE DEFAULT PRI SYSKEY\nclean 2,2 DEFAULT 5 12\ndirty 2,2 DEFAULT 6 12\n#OBJECT FILE PATHMODE\ndirty absent 1\n#START\nwait 1000\n#DEFAULT.PUSH_L\n$0=$0*10+(_CLICK_L_==clean)+2*(_CLICK_L_==dirty)\n@ size 3 4\n{decode}#DEFAULT.RELEASE_L\n$1=$1+1\n"
        );
        for budget in [1, 4096] {
            let Setup {
                mut runtime,
                fail_media,
                ..
            } = setup(budget, source.as_bytes());
            let (fes, _, buttons) = start_button_scene(&mut runtime);
            send(&mut runtime, fes, 64, buttons[0], 1);
            fail_media.set(true);
            let press = command(buttons[0], 4, 12, 1);
            for _ in 0..2 {
                assert!(
                    runtime
                        .dispatch_command(0, press)
                        .unwrap_err()
                        .message
                        .contains("媒体读取失败")
                );
                assert_eq!(runtime.numbers().shared[0], 12);
                assert_eq!(runtime.numbers().shared[2], 0);
            }
            fail_media.set(false);
            assert_eq!(runtime.dispatch_command(0, press).unwrap(), 1);
            assert_eq!(runtime.numbers().shared[0], 12);
            assert_eq!(runtime.numbers().shared[2], 0);
            assert_eq!(send(&mut runtime, buttons[0], 4, 12, 0), 1);
            assert_eq!(runtime.numbers().shared[1], 0);
            assert_eq!(send(&mut runtime, buttons[0], 4, 0, 0), 1);
            assert_eq!(runtime.numbers().shared[1], 1);
        }
    }
}

#[test]
fn direction_events_precede_pointer_moves_and_preserve_the_previous_pressed_bank() {
    let source = b"#KEYBLOCK 1\n0 2,1\n#DEFINE\nBUTTON item[2]\n#OBJECT SIZE POS KEYBLOCK PRI\nitem[0] 32,16 0,0 0,0,0 5\nitem[1] 32,16 40,0 0,1,0 6\n#START\nitem[1] keypos 1 8 4\nwait 1000\n#item.TOUCH\n$0=(_PUSH_RIGHT_==item[1])\n$1=@\n$2=(_PUSH_RIGHT_!=item[0])\n$5=(_RELEASE_RIGHT_==item[0])\n#item.FOCUS\n$3=$3*10+@+1\n#item.UNFOCUS\n$4=$4*10+@+1\n";
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            pointer,
            ..
        } = setup(budget, source);
        runtime.input_coordinates_mut().logical_size = [64, 32];
        pointer.set(PointerSample {
            position: [1, 1],
            active: 1,
        });
        let (_, _, buttons) = start_button_scene(&mut runtime);
        runtime.update_input_selection(1, Some([1, 1])).unwrap();
        assert_eq!(runtime.numbers().shared[3], 1);
        assert_eq!(send(&mut runtime, buttons[0], 4, 7, 1), 0);
        send(&mut runtime, buttons[1], 5, 0, 0);
        assert_eq!(&runtime.numbers().shared[..5], &[1, 1, 1, 1, 0]);
        assert_eq!(pointer.get().position, [1, 1]);
        let movement = runtime.input_state(1).unwrap().pointer();
        assert_eq!(movement.move_target, [48, 4]);
        assert_eq!(movement.move_remaining, 1);
        assert_eq!(send(&mut runtime, buttons[0], 4, 7, 0), 1);
        send(&mut runtime, buttons[1], 5, 0, 0);
        assert_eq!(runtime.numbers().shared[5], 1);
        runtime.update(102, 1).unwrap();
        assert_eq!(pointer.get().position, [48, 4]);
        assert_eq!(&runtime.numbers().shared[3..5], &[12, 1]);
        assert_eq!(runtime.input_state(1).unwrap().pointer().move_remaining, 0);

        assert_eq!(send(&mut runtime, buttons[1], 4, 6, 1), 0);
        assert_eq!(&runtime.numbers().shared[3..5], &[12, 1]);
        assert_eq!(
            runtime.input_state(1).unwrap().pointer().move_target,
            [16, 8]
        );
        runtime.update(103, 1).unwrap();
        assert_eq!(pointer.get().position, [16, 8]);
        assert_eq!(&runtime.numbers().shared[3..5], &[121, 12]);
    }
}

#[test]
fn direction_fallback_distinguishes_enabled_focus_disabled_hits_and_preferred_targets() {
    for (focus, hit, source_enabled, preferred_enabled, response, moved) in [
        (false, false, 1, 21, 0, true),
        (false, false, 1, 0, 1, false),
        (true, false, 1, 1, 1, false),
        (true, false, 0, 1, 0, true),
        (false, true, 0, 1, 0, true),
        (false, true, 0, 0, 0, false),
    ] {
        let source = format!(
            "#DEFINE\nBUTTON item[2]\n#OBJECT SIZE POS ENABLE PRI\nitem[0] 32,16 0,0 {source_enabled} 5\nitem[1] 32,16 40,0 {preferred_enabled} 6\n#START\nitem[1] noact\nwait 1000\n#item.TOUCH\n$0=(_PUSH_RIGHT_==item[1])\n"
        );
        for budget in [1, 4096] {
            let Setup {
                mut runtime,
                pointer,
                ..
            } = setup(budget, source.as_bytes());
            runtime.input_coordinates_mut().logical_size = [64, 32];
            pointer.set(PointerSample {
                position: [60, 25],
                active: 1,
            });
            let (fes, _, buttons) = start_button_scene(&mut runtime);
            if focus {
                send(&mut runtime, fes, 64, buttons[0], 1);
            }
            if hit {
                pointer.set(PointerSample {
                    position: [1, 1],
                    active: 1,
                });
            }
            assert_eq!(send(&mut runtime, buttons[0], 4, 7, 1), response);
            send(&mut runtime, buttons[1], 5, 0, 0);
            assert_eq!(runtime.numbers().shared[0], i32::from(moved));
            let pointer = runtime.input_state(1).unwrap().pointer();
            assert_eq!(pointer.move_remaining, i32::from(moved));
            if moved {
                assert_eq!(pointer.move_target, [56, 8]);
            }
        }
    }
}

#[test]
fn direction_media_retry_keeps_a_newer_pointer_request() {
    let source = b"#KEYBLOCK 1\n0 2,1\n#DEFINE\nBUTTON item[2]\n#OBJECT SIZE POS KEYBLOCK PRI FILE PATHMODE\nitem[0] 32,16 0,0 0,0,0 5 absent 1\nitem[1] 32,16 40,0 0,1,0 6 absent 1\n#START\nwait 1000\n#item.TOUCH\n$0=$0+1\n@ size 33 16\n@ keypos 1 8 4\n";
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            fail_media,
            ..
        } = setup(budget, source);
        runtime.input_coordinates_mut().logical_size = [64, 32];
        let (fes, _, buttons) = start_button_scene(&mut runtime);
        send(&mut runtime, fes, 64, buttons[0], 1);
        send(&mut runtime, buttons[1], 5, 0, 0);
        fail_media.set(true);
        assert!(
            runtime
                .dispatch_command(0, command(buttons[0], 4, 7, 1))
                .unwrap_err()
                .message
                .contains("媒体读取失败")
        );
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(
            runtime.input_state(1).unwrap().pointer().move_target,
            [48, 4]
        );
        send(&mut runtime, 1, 139, 0, 12 | (13 << 16));
        fail_media.set(false);
        assert_eq!(send(&mut runtime, buttons[0], 4, 7, 1), 0);
        assert_eq!(runtime.numbers().shared[0], 1);
        let movement = runtime.input_state(1).unwrap().pointer();
        assert_eq!(movement.move_target, [12, 13]);
        assert_eq!(movement.move_remaining, 0);
    }
}

#[test]
fn focus_notifications_wait_for_the_body_and_lock_commands_end_local_handlers() {
    use katze_system2::{fes, runtime::fes::Scene};
    let source = b"#DEFINE\nBUTTON item[3]\nBASE other\n#START\nitem noact ($0+=100)\nother noact ($0+=1)\nitem[1] noact ($0+=1)\nlockactive ($0+=1)\n$5=$5+1\nwait 1000\n#_FES_SLEEP_\nitem[2] noact ($0+=1)\nlockactive -3\n$6=$6+1\n#_FES_WAKE_\nlockactive\n$6=$6+100\n#item.FOCUS\nitem[0] noact\nlockactive 7\n$6=$6+1000\n";
    for budget in [1, 4096] {
        let mut runtime = setup(budget, b"#START\nwait 1000\n").runtime;
        let mut scene = Scene::new(
            fes::Program::from_text(source).unwrap(),
            runtime.host().handle_allocator(),
        )
        .unwrap();
        let list = runtime.create(0, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        scene
            .objects_mut()
            .unwrap()
            .set_image_list(list, katze_system2::graphics::Rect::from_xywh(0, 0, 8, 6));
        let update = |runtime: &mut TestRuntime, scene: &mut Scene, now| {
            for _ in 0..128 {
                if runtime.update_fes_scene(0, scene, now).unwrap() {
                    return;
                }
            }
            panic!("合成焦点场景没有完成本轮");
        };
        update(&mut runtime, &mut scene, 100);
        assert_eq!(runtime.numbers().shared[0], 3);
        assert_eq!(runtime.numbers().shared[5], 1);
        assert_eq!(scene.focus_lock(), 3);
        assert_eq!(scene.preferred_focus().unwrap().bank, 1);
        for _ in 0..128 {
            if runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
            {
                break;
            }
        }
        assert!(!scene.is_pending());
        assert_eq!(runtime.numbers().shared[0], 4);
        assert_eq!(runtime.numbers().shared[6], 0);
        assert_eq!(scene.focus_lock(), 3);
        assert_eq!(scene.preferred_focus().unwrap().bank, 1);
        update(&mut runtime, &mut scene, 101);
        assert_eq!(scene.focus_lock(), -3);
        assert_eq!(scene.preferred_focus().unwrap().bank, 2);
        let target = fes::ObjectReference {
            name: "item".into(),
            bank: 1,
        };
        let mut handled = None;
        for _ in 0..128 {
            handled = runtime
                .dispatch_fes_event(0, &mut scene, Some(&target), 0)
                .unwrap();
            if handled.is_some() {
                break;
            }
        }
        assert_eq!(handled, Some(true));
        assert_eq!(scene.focus_lock(), -3);
        assert_eq!(scene.preferred_focus().unwrap().bank, 2);
        update(&mut runtime, &mut scene, 102);
        assert_eq!(scene.focus_lock(), 7);
        assert_eq!(scene.preferred_focus().unwrap().bank, 0);
        for _ in 0..128 {
            if runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_WAKE_")
                .unwrap()
            {
                break;
            }
        }
        assert!(!scene.is_pending());
        assert_eq!(scene.focus_lock(), 7);
        update(&mut runtime, &mut scene, 103);
        assert_eq!(scene.focus_lock(), 0);
        assert_eq!(runtime.numbers().shared[6], 0);
    }
}

#[test]
fn focus_lock_preserves_pressed_targets_after_unfocus_and_missing_selections() {
    let source = b"#DEFINE\nBUTTON item\n#OBJECT SIZE PRI\nitem 2,2 5\n#START\nlockactive -3\nwait 1000\n#item.PUSH_L\n$0=$0+1\n#item.RELEASE_L\n$1=$1+1\n#item.UNFOCUS\n$2=$2+1\n#_FES_SLEEP_\nlockactive 0\n";
    for budget in [1, 4096] {
        let mut runtime = setup(budget, source).runtime;
        let (fes, _, buttons) = start_button_scene(&mut runtime);
        let button = buttons[0];
        send(&mut runtime, fes, 64, button, 1);
        send(&mut runtime, button, 4, 0, 1);
        send(&mut runtime, fes, 64, u32::MAX, 1);
        send(&mut runtime, button, 4, 0, 0);
        assert_eq!(&runtime.numbers().shared[..2], &[1, 1]);
        send(&mut runtime, button, 4, 0, 1);
        send(&mut runtime, fes, 64, button, 0);
        send(&mut runtime, button, 4, 0, 0);
        assert_eq!(&runtime.numbers().shared[..3], &[2, 2, 1]);
        send(&mut runtime, button, 4, 0, 1);
        send(&mut runtime, button, 4, 0, 0);
        assert_eq!(&runtime.numbers().shared[..3], &[2, 2, 1]);

        send(&mut runtime, fes, 348, 0, 1);
        send(&mut runtime, fes, 348, 0, 0);
        runtime.update(102, 1).unwrap();
        send(&mut runtime, fes, 64, button, 1);
        send(&mut runtime, button, 4, 0, 1);
        send(&mut runtime, fes, 64, u32::MAX, 1);
        send(&mut runtime, button, 4, 0, 0);
        assert_eq!(&runtime.numbers().shared[..3], &[3, 2, 1]);
    }
}

#[test]
fn button_events_keep_pressed_banks_and_defer_touch_media_refresh() {
    let source = b"#DEFINE\nBUTTON item[2]\nCIMAGE fill\n#OBJECT SIZE PRI ENABLE DEFAULT POS\nitem 2,2 5 21 DEFAULT 0,0\nitem[1] 2,2 6 21 DEFAULT 4,0\n#OBJECT SIZE COLOR PRI POS DISP\nfill 2,2 0xff112233 30 0,3 1\n#START\nwait 1000\n$7=_CLICK_L_\nwait 1000\n#DEFAULT.PUSH_L\n$0=$0*10+1\n$6=1\ndecode\n$6=99\n#item.PUSH_L\n$0=$0*10+2\n$1=@\n$5=(_CLICK_L_==item[@])\n#item.RELEASE_L\n$2=@+10\n@ enable 0\n#item.FOCUS\n$3=$3*10+@+1\n#item.TOUCH\n$4=$4+1\nfill disp 0\n";
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            pointer,
            ..
        } = setup(budget, source);
        pointer.set(PointerSample {
            position: [1, 1],
            active: 1,
        });
        runtime.input_coordinates_mut().logical_size = [8, 6];
        let (fes, list, buttons) = start_button_scene(&mut runtime);
        assert_eq!(buttons.len(), 2);
        runtime.update_input_selection(1, Some([1, 1])).unwrap();
        assert_eq!(runtime.numbers().shared[3], 1);
        assert_eq!(send(&mut runtime, buttons[0], 4, 0, 1), 0);
        assert_eq!(&runtime.numbers().shared[..2], &[12, 0]);
        assert_eq!(runtime.numbers().shared[6], 1);
        assert_eq!(runtime.numbers().shared[5], 1);
        // 只发布新焦点；UNFOCUS 会另行清除保存的按下目标。
        send(&mut runtime, fes, 64, buttons[1], 1);
        assert_eq!(runtime.numbers().shared[3], 12);
        assert_eq!(send(&mut runtime, buttons[0], 4, 0, 0), 0);
        assert_eq!(runtime.numbers().shared[2], 10);
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(buttons[0])
                .unwrap()
                .enabled(),
            0
        );
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(buttons[1])
                .unwrap()
                .enabled(),
            21
        );
        send(&mut runtime, fes, 64, buttons[0], 1);
        assert_eq!(send(&mut runtime, buttons[0], 4, 0, 1), 1);
        assert_eq!(runtime.numbers().shared[0], 12);
        send(&mut runtime, fes, 64, buttons[1], 1);
        assert_eq!(send(&mut runtime, buttons[1], 4, 0, 1), 0);
        assert_eq!(&runtime.numbers().shared[..2], &[1212, 1]);
        assert_eq!(runtime.numbers().shared[5], 1);
        send(&mut runtime, fes, 64, buttons[1], 0);
        assert_eq!(send(&mut runtime, buttons[1], 4, 0, 0), 1);
        assert_eq!(runtime.numbers().shared[2], 10);
        send(&mut runtime, list, 298, 0, 0);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(0, 3),
            Some(0xff112233)
        );
        assert_eq!(send(&mut runtime, buttons[1], 5, 0, 0), 0);
        assert_eq!(runtime.numbers().shared[4], 1);
        send(&mut runtime, list, 298, 0, 0);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(0, 3),
            Some(0xff112233)
        );
        send(&mut runtime, fes, 64, u32::MAX, 0);
        send(&mut runtime, list, 298, 0, 0);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(0, 3),
            Some(0)
        );
        runtime.update(102, 1).unwrap();
        assert_eq!(runtime.numbers().shared[7], 0);
    }
}

#[test]
fn default_only_button_events_respond_and_retry_media_without_repeating_script_effects() {
    for decode in ["", "decode\n$2=$2+1\n"] {
        let source = format!(
            "#DEFINE\nBUTTON item\n#OBJECT FILE PATHMODE SIZE DEFAULT PRI\nitem absent 1 2,2 DEFAULT 5\n#START\nwait 1000\n#DEFAULT.PUSH_L\n$0=$0+1\n@ size 3 4\n{decode}#DEFAULT.RELEASE_L\n$1=$1+1\n"
        );
        for budget in [1, 4096] {
            let Setup {
                mut runtime,
                fail_media,
                reads,
                ..
            } = setup(budget, source.as_bytes());
            let (fes, _, buttons) = start_button_scene(&mut runtime);
            let button = buttons[0];
            send(&mut runtime, fes, 64, button, 1);
            assert_eq!(
                reads
                    .borrow()
                    .iter()
                    .filter(|name| name.as_slice() == b"absent")
                    .count(),
                1
            );
            fail_media.set(true);
            assert!(
                runtime
                    .dispatch_command(0, command(button, 4, 0, 1))
                    .unwrap_err()
                    .message
                    .contains("媒体读取失败")
            );
            assert_eq!(runtime.numbers().shared[0], 1);
            assert_eq!(runtime.numbers().shared[2], 0);
            assert!(
                runtime
                    .dispatch_command(0, command(fes, 64, button, 0))
                    .unwrap_err()
                    .message
                    .contains("上一次命令")
            );
            fail_media.set(false);
            assert_eq!(send(&mut runtime, button, 4, 0, 1), 1);
            assert_eq!(runtime.numbers().shared[0], 1);
            assert_eq!(runtime.numbers().shared[2], 0);
            assert_eq!(
                reads
                    .borrow()
                    .iter()
                    .filter(|name| name.as_slice() == b"absent")
                    .count(),
                3
            );
            assert_eq!(
                runtime
                    .input_state(1)
                    .unwrap()
                    .regions()
                    .get(button)
                    .unwrap()
                    .bounds(),
                katze_system2::graphics::Rect::from_xywh(0, 0, 3, 4)
            );
            assert_eq!(send(&mut runtime, button, 4, 0, 0), 1);
            assert_eq!(runtime.numbers().shared[1], 1);
        }
    }
}

#[test]
fn dedicated_pause_hides_refreshed_media_and_wakes_with_current_fes_display() {
    let source = b"#DEFINE\nCIMAGE sprite\nCPLANE panel\nPLANE view\nBUTTON control\n#OBJECT SIZE PRI ENABLE\ncontrol 2,2 8 21\n#OBJECT SIZE COLOR PRI POS DISP\nsprite 2,2 0xff123456 5 1,1 21\npanel 1,1 0xff654321 100 5,4 94\n#OBJECT POS SIZE VRAM PRI DISP\nview 0,0 8,6 0,0 500 57\n#START\n$0=$0+1\nwait 77\n$1=$1+1\n#_FES_SLEEP_\n$2=$2+1\nsprite size 3 3\nsprite pri ($4+=1)\n#_FES_WAKE_\n$3=$3+1\nsprite size 4 4\npanel pri 99\n";
    for budget in [1, 4096] {
        let Setup { mut runtime, .. } = setup(budget, source);
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, fes, 340, 0, list);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        let control = runtime
            .tree()
            .children(fes)
            .unwrap()
            .find(|id| runtime.tree().node(*id).unwrap().kind == kind::BUTTON)
            .unwrap();
        send(&mut runtime, fes, 339, 0, 0);
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(control)
                .unwrap()
                .enabled(),
            21
        );
        runtime.update(1, 1).unwrap();
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0xff123456)
        );
        let planes: Vec<_> = runtime
            .tree()
            .children(fes)
            .unwrap()
            .filter(|id| runtime.tree().node(*id).unwrap().kind == kind::PLANE)
            .collect();
        assert_eq!(planes.len(), 2);
        for (value, count) in [(0, 0), (1, 1), (1, 1), (2, 2)] {
            send(&mut runtime, fes, 348, 0, value);
            assert_eq!(runtime.numbers().shared[2], count);
            assert_eq!(runtime.numbers().shared[4], count);
            assert_eq!(
                runtime
                    .input_state(1)
                    .unwrap()
                    .regions()
                    .get(control)
                    .unwrap()
                    .enabled(),
                if value == 0 { 21 } else { 0 }
            );
        }
        runtime.update(123, 1).unwrap();
        assert_eq!(&runtime.numbers().shared[..4], &[1, 0, 2, 0]);
        assert_eq!(runtime.object(fes).unwrap().clock().time, 123);
        assert_eq!(runtime.object(list).unwrap().clock().time, 123);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0)
        );
        for &id in &planes {
            assert_eq!(runtime.drawables().get(id).unwrap().display(), 0);
            assert_eq!(runtime.object(id).unwrap().clock().time, 123);
        }
        send(&mut runtime, list, 310, 0, 0);
        send(&mut runtime, fes, 348, 0, 0);
        assert_eq!(
            runtime
                .input_state(1)
                .unwrap()
                .regions()
                .get(control)
                .unwrap()
                .enabled(),
            21
        );
        assert_eq!(runtime.numbers().shared[3], 1);
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(1, 1),
            Some(0xff123456)
        );
        assert_eq!(
            runtime.drawing().target.as_ref().unwrap().pixel(4, 4),
            Some(0xff123456)
        );
        assert_eq!(
            runtime
                .drawables()
                .get(planes[0])
                .unwrap()
                .transform()
                .base_point()[2],
            99.
        );
        assert_eq!(runtime.drawables().get(planes[0]).unwrap().display(), 94);
        assert_eq!(runtime.drawables().get(planes[1]).unwrap().display(), 57);
        send(&mut runtime, fes, 348, 0, 0);
        assert_eq!(runtime.numbers().shared[3], 1);
        runtime.update(124, 1).unwrap();
        assert_eq!(&runtime.numbers().shared[..4], &[1, 1, 2, 1]);
    }
}

#[test]
fn pause_propagates_to_nested_fes_and_skips_the_first_body_until_wake() {
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            button_attempts,
            ..
        } = setup(
            budget,
            b"#START\n$0=$0+1\nwait 100\n#_FES_SLEEP_\n$2=$2*10+1\n#_FES_WAKE_\n$3=$3*10+2\n",
        );
        let parent = runtime.create(0, kind::FES, b"parent", b"").unwrap();
        let child = runtime.create(parent, kind::FES, b"child", b"").unwrap();
        for id in [parent, child] {
            send(&mut runtime, id, 337, 0, 0);
            file(&mut runtime, id, b"complete.fes").unwrap();
        }
        runtime
            .dispatch_command(
                0,
                Command {
                    data: [0, 2, 1],
                    ..command(parent, 348, 0, 2)
                },
            )
            .unwrap();
        assert_eq!(runtime.numbers().shared[2], 11);
        runtime.update(100, 1).unwrap();
        assert_eq!(button_attempts.get(), 0);
        assert_eq!(runtime.object(parent).unwrap().clock().time, 100);
        assert_eq!(runtime.object(child).unwrap().clock().time, 100);
        send(&mut runtime, parent, 339, 0, 0);
        send(&mut runtime, child, 339, 0, 0);
        assert_eq!(runtime.numbers().shared[0], 0);
        runtime
            .dispatch_command(
                0,
                Command {
                    data: [0, 0, 1],
                    ..command(parent, 348, 0, 0)
                },
            )
            .unwrap();
        assert_eq!(runtime.numbers().shared[3], 22);
        runtime.update(101, 1).unwrap();
        assert_eq!(button_attempts.get(), 2);
        assert_eq!(runtime.numbers().shared[0], 2);
    }
}

#[test]
fn media_failure_retries_pause_without_repeating_the_system_block() {
    for budget in [1, 4096] {
        let Setup { mut runtime, fail_media, reads, .. } = setup(budget,
            b"#DEFINE\nIMAGE sprite\n#OBJECT FILE PATHMODE SIZE PRI DISP\nsprite absent 1 2,2 5 1\n#START\nwait 77\n#_FES_SLEEP_\n$2=$2+1\n");
        let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
        send(&mut runtime, fes, 337, 0, 0);
        let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
        send(&mut runtime, list, 285, 8, 6);
        send(&mut runtime, fes, 340, 0, list);
        file(&mut runtime, fes, b"complete.fes").unwrap();
        fail_media.set(true);
        for _ in 0..2 {
            assert!(
                runtime
                    .dispatch_command(0, command(fes, 348, 0, 1))
                    .unwrap_err()
                    .message
                    .contains("媒体读取失败")
            );
            assert_eq!(runtime.numbers().shared[2], 1);
        }
        assert!(
            runtime
                .dispatch_command(0, command(fes, 348, 0, 0))
                .is_err()
        );
        fail_media.set(false);
        send(&mut runtime, fes, 348, 0, 1);
        assert_eq!(runtime.numbers().shared[2], 1);
        assert_eq!(
            reads
                .borrow()
                .iter()
                .filter(|name| name.as_slice() == b"absent")
                .count(),
            3
        );
        runtime.update(100, 1).unwrap();
    }
}

#[test]
fn system_exit_notifications_wait_for_the_body_and_main_exits_continue_the_script() {
    let source = b"#START\n$0=$0+1\nwait 77\nexit 37\n$1=$1+1\nwait 5\n#_FES_SLEEP_\n$2=$2+1\nexit 9\n$2=999\n#_FES_WAKE_\nexit 11\n";
    for budget in [1, 4096] {
        for drain_while_waiting in [false, true] {
            let Setup { mut runtime, .. } = setup(budget, source);
            let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            file(&mut runtime, fes, b"complete.fes").unwrap();
            send(&mut runtime, fes, 339, 0, 0);
            send(&mut runtime, fes, 24, 0, 73);
            send(&mut runtime, fes, 348, 0, 1);
            runtime.update(20, 1).unwrap();
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), 73);
            send(&mut runtime, fes, 348, 0, 2);
            send(&mut runtime, fes, 348, 0, 0);
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), 73);
            assert_eq!(runtime.numbers().shared[2], 2);
            if drain_while_waiting {
                runtime.update(21, 1).unwrap();
                assert_eq!(send(&mut runtime, fes, 23, 0, 0), 11);
                send(&mut runtime, fes, 24, 0, 91);
                runtime.update(22, 1).unwrap();
                assert_eq!(send(&mut runtime, fes, 23, 0, 0), 91);
            }
            runtime.update(77, 1).unwrap();
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), 37);
            assert_eq!(&runtime.numbers().shared[..3], &[1, 1, 2]);
            send(&mut runtime, fes, 24, 0, 73);
            for now in [78, 82, 83] {
                runtime.update(now, 1).unwrap();
                assert_eq!(send(&mut runtime, fes, 23, 0, 0), 73);
            }
            assert_eq!(runtime.numbers().shared[0], 2);
            runtime.update(160, 1).unwrap();
            assert_eq!(send(&mut runtime, fes, 23, 0, 0), 37);
            assert_eq!(&runtime.numbers().shared[..3], &[2, 2, 2]);
        }
    }
}

#[test]
fn load_retry_keeps_a_record_wrapper_created_before_a_failed_button() {
    for budget in [1, 4096] {
        let mut snapshots = Vec::new();
        for fail in [false, true] {
            let Setup {
                mut runtime,
                fail_button,
                reads,
                ..
            } = setup(
                budget,
                b"#DEFINE\nBUTTON item\nBUTTON item\n#START\n$0=$0+1\nwait 99\n",
            );
            let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            fail_button.set(fail);
            if fail {
                assert!(
                    file(&mut runtime, fes, b"complete.fes")
                        .unwrap_err()
                        .message
                        .contains("按钮创建失败")
                );
                assert_eq!(runtime.tree().children(fes).unwrap().count(), 0);
                fail_button.set(false);
            }
            file(&mut runtime, fes, b"complete.fes").unwrap();
            let children: Vec<_> = runtime.tree().children(fes).unwrap().collect();
            assert_eq!(children.len(), 2);
            assert_eq!(runtime.numbers().shared[0], 0);
            assert_eq!(*reads.borrow(), [b"complete.fes".to_vec()]);
            snapshots.push((
                children,
                runtime.host().handle_allocator().allocate().unwrap(),
            ));
        }
        assert_eq!(snapshots[0], snapshots[1]);
    }
}

#[test]
fn slider_parent_callbacks_use_the_sender_and_keep_raw_positions_while_paused() {
    let source = b"#DEFINE\nSLIDER item[2]\nSLIDER item[3]\nBASE other\n#START\nwait 1000\n#item.SCRL_P\n@ getpos 0 1 2\n$3=$3*10+@+1\n$4=_MOUSE_X_\n$5=_MOUSE_Y_\n#item.FOCUS\n$6=$6*10+@+1\n#item.UNFOCUS\n$7=$7*10+@+1\n";
    for budget in [1, 4096] {
        for paused in [false, true] {
            let Setup {
                mut runtime,
                pointer,
                ..
            } = setup(budget, source);
            let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            file(&mut runtime, fes, b"complete.fes").unwrap();
            let sliders: Vec<_> = runtime
                .tree()
                .children(fes)
                .unwrap()
                .filter(|&id| runtime.tree().node(id).unwrap().kind == kind::SLIDER)
                .collect();
            assert_eq!(sliders.len(), 5);
            if paused {
                send(&mut runtime, fes, 348, 0, 1);
            }
            pointer.set(PointerSample {
                position: [12, -4],
                active: 1,
            });
            runtime.input_state_mut(1).unwrap().pointer_mut().active = 1;
            let mut notify = |sender: u32, code: u32, value: u32| {
                runtime
                    .dispatch_command(
                        0,
                        Command {
                            target: fes,
                            sender,
                            code,
                            data: [0xabcdef01, value, 1],
                        },
                    )
                    .unwrap()
            };
            assert_eq!(notify(sliders[0], 27, (-3i32) as u32), 0);
            assert_eq!(notify(sliders[4], 27, i32::MIN as u32), 0);
            assert_eq!(notify(sliders[4], 28, 0x80000000), 0);
            assert_eq!(notify(sliders[4], 28, 0), 0);
            assert_eq!(notify(99, 27, 55), 0);
            assert_eq!(runtime.numbers().shared, [i32::MIN, 0, 0, 13, 12, -4, 3, 3]);
            send(&mut runtime, fes, 3, 0, 0);
            runtime
                .dispatch_command(
                    0,
                    Command {
                        target: fes,
                        sender: sliders[4],
                        code: 27,
                        data: [0, 55, 0],
                    },
                )
                .unwrap();
            assert_eq!(runtime.numbers().shared[0], i32::MIN);
        }
    }
}

fn slider_sprite() -> Vec<u8> {
    let zip = |input: &[u8]| {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(input).unwrap();
        encoder.finish().unwrap()
    };
    let data = zip(&[0, 0, 0, 64]);
    let control = zip(&[9, 1]);
    let mut out = b"HG-3".to_vec();
    for n in [12u32, 0x300, 0, 0] {
        out.extend(n.to_le_bytes());
    }
    out.extend(b"stdinfo\0");
    for n in [56u32, 40, 1, 1, 32, 0, 0, 1, 1, 1, 0, 0] {
        out.extend(n.to_le_bytes());
    }
    out.extend(b"img0000\0");
    for n in [
        0,
        (24 + data.len() + control.len()) as u32,
        0,
        1,
        data.len() as u32,
        4,
        control.len() as u32,
        2,
    ] {
        out.extend(n.to_le_bytes());
    }
    out.extend(data);
    out.extend(control);
    out
}
struct SliderFiles(Files);
impl ResourceServices for SliderFiles {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        Ok(name.to_vec())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        self.0.read(name, mode)
    }
}

#[test]
fn sliding_controls_drag_reaches_fes_events_and_the_next_media_update_without_renotifying() {
    for (name, kind, page, expected) in [
        ("SLIDER", kind::SLIDER, "", 5),
        ("SCROLL2", kind::SCROLL2, " 1", 4),
    ] {
        let source = format!(
            "#DEFINE\n{name} bar\n#OBJECT FILE POS2 DISP ENABLE\nbar control bounds 1 7\n#START\nbar page 0 6{page}\nbar pos 2\nwait 1000\n#bar.SCRL_P\n@ getpos 0 1 2\n$3=$3+1\n#bar.FOCUS\n$4=$4+1\n#bar.UNFOCUS\n$5=$5+1\n"
        );
        let source = source.as_bytes();
        for budget in [1, 4096] {
            let Setup {
                mut runtime,
                pointer,
                ..
            } = setup(budget, source);
            runtime.input_coordinates_mut().logical_size = [8, 6];
            let sprite = slider_sprite();
            let mut bounds = sprite.clone();
            bounds[36..40].copy_from_slice(&7u32.to_le_bytes());
            runtime.set_resources(SliderFiles(Files {
                files: [
                    (b"complete.fes".to_vec(), packed(source)),
                    (b"control".to_vec(), sprite),
                    (b"bounds".to_vec(), bounds),
                ]
                .into(),
                reads: Rc::default(),
                fail_media: Rc::new(Cell::new(false)),
            }));
            let fes = runtime.create(0, kind::FES, b"scene", b"").unwrap();
            send(&mut runtime, fes, 337, 0, 0);
            file(&mut runtime, fes, b"complete.fes").unwrap();
            let slider = runtime
                .tree()
                .children(fes)
                .unwrap()
                .find(|&id| runtime.tree().node(id).unwrap().kind == kind)
                .unwrap();
            let list = runtime.create(fes, kind::IMAGE_LIST, b"list", b"").unwrap();
            send(&mut runtime, list, 285, 8, 6);
            send(&mut runtime, fes, 340, 0, list);
            send(&mut runtime, fes, 339, 0, 0);
            let buttons: Vec<_> = runtime.tree().children(slider).unwrap().collect();
            assert_eq!(buttons.len(), 2);
            let knob = buttons[1];
            pointer.set(PointerSample {
                position: [2, 0],
                active: 1,
            });
            runtime.input_state_mut(1).unwrap().pointer_mut().active = 1;
            runtime.update_input_selection(1, Some([2, 0])).unwrap();
            assert_eq!(runtime.numbers().shared[4], 1);
            send(&mut runtime, knob, 95, 0, 0);
            let event = |pressed| KeyEvent {
                kind: KeyEventKind::Change,
                code: 4,
                key: 0,
                pressed,
                mask: 2,
                point: [2, 0],
            };
            assert!(runtime.dispatch_input_key(1, event(1)).unwrap());
            pointer.set(PointerSample {
                position: [5, 0],
                active: 1,
            });
            runtime
                .dispatch_input_motion(
                    1,
                    katze_system2::input::PointerMotion {
                        code: 6,
                        previous: [2, 0],
                        current: [5, 0],
                    },
                )
                .unwrap();
            assert_eq!(runtime.numbers().shared[..4], [expected, 0, 0, 1]);
            assert!(runtime.dispatch_input_key(1, event(0)).unwrap());
            assert_eq!(runtime.numbers().shared[3], 1);
            runtime.update(1, 1).unwrap();
            assert_eq!(runtime.numbers().shared[..4], [expected, 0, 0, 1]);
            send(&mut runtime, list, 298, 0, 0);
            let target = runtime.drawing().target.as_ref().unwrap();
            assert_eq!(target.pixel(expected as u32, 0), Some(0xff000000));
            assert_eq!(target.pixel(2, 0), Some(0));
            for paused in [1, 2, 0] {
                send(&mut runtime, fes, 348, 0, paused);
                send(&mut runtime, list, 298, 0, 0);
                assert_eq!(
                    runtime
                        .drawing()
                        .target
                        .as_ref()
                        .unwrap()
                        .pixel(expected as u32, 0),
                    Some(if paused != 0 { 0 } else { 0xff000000 })
                );
                let area = runtime.input_state(1).unwrap().regions().get(knob).unwrap();
                assert_eq!(area.enabled(), if paused != 0 { 0 } else { 7 });
            }
        }
    }
}

#[test]
fn linked_slider_callbacks_finish_inside_media_updates_and_preserve_main_waits() {
    let source = b"#KEYBLOCK 1\n0 1,1\n#DEFINE\nBUTTON nav\nSLIDER bar\n#OBJECT SIZE POS KEYBLOCK PRI\nnav 2,2 0,3 0,0,0 5\n#OBJECT FILE POS2 DISP\nbar control bounds 1\n#START\nnav attach bar\nbar page 0 6\nbar pos 2\nwait 1000\n$6=$6+1\nbar getpos 0 1 2\nbar disp 1\nbar enable 1\nwait 9999\n#bar.SCRL_P\n@ getpos 0 1 2\n$3=$3+1\n$4=_MOUSE_X_\n$5=_MOUSE_Y_\n@ pos 4\n@ page 0 9\nif ($7)\n@ disp 0\n@ enable 0\nendif\n";
    for budget in [1, 4096] {
        for hide in [false, true] {
            let Setup {
                mut runtime,
                pointer,
                ..
            } = setup(budget, source);
            runtime.numbers_mut().shared[7] = i32::from(hide);
            runtime.input_coordinates_mut().logical_size = [8, 6];
            let sprite = slider_sprite();
            let mut bounds = sprite.clone();
            bounds[36..40].copy_from_slice(&7u32.to_le_bytes());
            runtime.set_resources(SliderFiles(Files {
                files: [
                    (b"complete.fes".to_vec(), packed(source)),
                    (b"control".to_vec(), sprite),
                    (b"bounds".to_vec(), bounds),
                ]
                .into(),
                reads: Rc::default(),
                fail_media: Rc::new(Cell::new(false)),
            }));
            let (_, list, children) = start_button_scene(&mut runtime);
            let nav = children
                .into_iter()
                .find(|&id| runtime.tree().node(id).unwrap().kind == kind::BUTTON)
                .unwrap();
            pointer.set(PointerSample {
                position: [1, 4],
                active: 1,
            });
            runtime.input_state_mut(1).unwrap().pointer_mut().active = 1;
            runtime.update_input_selection(1, Some([1, 4])).unwrap();
            assert_eq!(send(&mut runtime, nav, 4, 7, 1), 0);
            assert_eq!(&runtime.numbers().shared[..7], &[3, 0, 0, 1, 1, 4, 0]);
            send(&mut runtime, list, 298, 0, 0);
            let pixels = runtime.drawing().target.as_ref().unwrap();
            assert_eq!(pixels.pixel(3, 0), Some(if hide { 0 } else { 0xff000000 }));
            assert_eq!(pixels.pixel(4, 0), Some(0));
            runtime.update(102, 1).unwrap();
            assert_eq!(runtime.numbers().shared[3], 1);
            assert_eq!(runtime.numbers().shared[6], 0);
            runtime.update(1200, 1).unwrap();
            assert_eq!(&runtime.numbers().shared[..7], &[4, 0, 0, 1, 1, 4, 1]);
            send(&mut runtime, list, 298, 0, 0);
            let pixels = runtime.drawing().target.as_ref().unwrap();
            assert_eq!(pixels.pixel(3, 0), Some(0xff000000));
            assert_eq!(pixels.pixel(4, 0), Some(0));
        }
    }
}

struct ScrollTransport {
    id: u32,
    base: BaseObject,
    calls: ControlCalls,
}

impl TaskObject for ScrollTransport {
    fn control(&self) -> &Control {
        self.base.control()
    }
    fn clock(&self) -> &TaskClock {
        self.base.clock()
    }
    fn state(&mut self) -> (&mut Control, &mut TaskClock) {
        self.base.state()
    }
    fn interpreter(&self) -> Option<&Interpreter> {
        None
    }
    fn interpreter_mut(&mut self) -> Option<&mut Interpreter> {
        None
    }
    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        self.base.set_creation_context(context)
    }
    fn update_body(&mut self, _: &mut dyn Host, _: u32, _: usize) -> Result<bool> {
        Ok(true)
    }
    fn dispatch_command(
        &mut self,
        kind: u32,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        self.dispatch_command_with_buffers(
            kind,
            command,
            &mut CommandBuffers::default(),
            output,
            host,
            budget,
        )
    }
    fn dispatch_command_with_buffers(
        &mut self,
        kind: u32,
        command: &Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<CommandOutcome> {
        if !(374..=382).contains(&command.code) {
            return self
                .base
                .dispatch_command(kind, command, output, host, budget);
        }
        self.calls.borrow_mut().push((
            *command,
            buffers.data1.as_deref().unwrap_or_default().to_vec(),
        ));
        if command.code == 377 && command.data[0] != 0 {
            let mut value = 0;
            assert!(host.dispatch_task_command(
                Command {
                    target: command.sender,
                    sender: self.id,
                    code: 27,
                    data: [0, command.data[1], 1],
                },
                &mut CommandBuffers::default(),
                &mut value
            )?);
        }
        Ok(CommandOutcome::finished(false))
    }
    fn enqueue_message(&mut self, message: [u32; 4]) -> Result<()> {
        self.base.enqueue_message(message)
    }
    fn dispatch_message(
        &mut self,
        message: &mut [u32; 4],
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        self.base.dispatch_message(message, output, host, budget)
    }
    fn request_delete(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.base.request_delete(host, budget)
    }
}

#[test]
fn scroll2_media_routes_pages_and_synchronous_notifications_and_restores_sleep_state() {
    let source = b"#KEYBLOCK 1\n0 1,1\n#DEFINE\nBUTTON nav\nSCROLL2 bar\n#OBJECT SIZE POS KEYBLOCK PRI\nnav 2,2 0,3 0,0,0 5\n#OBJECT DISP ENABLE\nbar 1 7\n#START\nnav attach bar\nbar page 0 9 3\nbar pos 2\nwait 1000\n$6=$6+1\nbar getpos 0 1 2\nwait 9999\n#bar.SCRL_P\n@ getpos 0 1 2\n$3=$3+1\n@ pos 4\n@ page 0 12 5\n#bar.FOCUS\n$7=$7+1\n#bar.UNFOCUS\n$7=$7+10\n";
    for budget in [1, 4096] {
        let Setup {
            mut runtime,
            pointer,
            ..
        } = setup(budget, source);
        let calls = ControlCalls::default();
        runtime.factory_mut().scroll = Some(calls.clone());
        let (fes, _, children) = start_button_scene(&mut runtime);
        let nav = children
            .iter()
            .copied()
            .find(|&id| runtime.tree().node(id).unwrap().kind == kind::BUTTON)
            .unwrap();
        let scroll = children
            .into_iter()
            .find(|&id| runtime.tree().node(id).unwrap().kind == kind::SCROLL2)
            .unwrap();
        let range = calls
            .borrow()
            .iter()
            .find(|(command, _)| command.code == 376)
            .unwrap()
            .1
            .clone();
        assert_eq!(range, [0i32, 9, 3].map(i32::to_le_bytes).concat());
        calls.borrow_mut().clear();
        pointer.set(PointerSample {
            position: [1, 4],
            active: 1,
        });
        runtime.input_state_mut(1).unwrap().pointer_mut().active = 1;
        runtime.update_input_selection(1, Some([1, 4])).unwrap();
        assert_eq!(send(&mut runtime, nav, 4, 7, 1), 0);
        assert_eq!(&runtime.numbers().shared[..4], &[3, 0, 0, 1]);
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(calls.borrow()[0].0.code, 377);
        assert_eq!(calls.borrow()[0].0.data, [1, 3, 1]);
        calls.borrow_mut().clear();
        runtime.update(102, 1).unwrap();
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(calls.borrow()[0].0.code, 376);
        assert_eq!(
            calls.borrow()[0].1,
            [0i32, 12, 5].map(i32::to_le_bytes).concat()
        );
        runtime.update(1200, 1).unwrap();
        assert_eq!(&runtime.numbers().shared[..7], &[4, 0, 0, 1, 0, 0, 1]);
        calls.borrow_mut().clear();
        send(&mut runtime, fes, 348, 0, 1);
        for value in [7, 0] {
            runtime
                .dispatch_command(
                    0,
                    Command {
                        target: fes,
                        sender: scroll,
                        code: 28,
                        data: [0xabcdef01, value, 1],
                    },
                )
                .unwrap();
        }
        assert_eq!(runtime.numbers().shared[7], 11);
        send(&mut runtime, fes, 348, 0, 0);
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|(command, _)| (command.code, command.data))
                .collect::<Vec<_>>(),
            [
                (375, [0, 0, 1]),
                (379, [0, 0, 1]),
                (375, [0, 7, 1]),
                (379, [0, 1, 1]),
            ]
        );
        send(&mut runtime, fes, 3, 0, 0);
        runtime.update(1201, 1).unwrap();
        assert!(runtime.object(scroll).is_none());
    }
}
