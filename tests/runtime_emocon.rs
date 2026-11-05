use katze_system2::{
    Result,
    expression::State,
    kcs::{Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::Cell, rc::Rc};

const CONFIG: &str = r#"<?xml version="1.0" encoding="UTF-8"?><document>
<size><texscale>125</texscale><s01><id>7</id><prefix>M</prefix><scale>80</scale></s01></size>
<ch><c01><id>9</id><p01><id>2</id><table>offsets.xml</table></p01></c01></ch>
<emocon><stop><label>停止</label></stop>
<em01><id>5</id><name>喜</name><label>喜ラベル</label><pos><x>-33</x><y>87</y></pos>
<scale>40</scale><tex><half>1</half><l>large</l><s>small</s></tex></em01>
<em02><id>8</id><name>plain</name></em02></emocon></document>"#;
const OFFSETS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><document>
<em01><id>5</id><offset><x>-7</x><y>12</y></offset></em01></document>"#;

struct Files(Rc<Cell<bool>>);
impl ResourceServices for Files {
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok(match name {
            b"config.int/emodef.xml" if self.0.get() => Some(CONFIG.as_bytes().to_vec()),
            b"config.int/offsets.xml" => Some(OFFSETS.as_bytes().to_vec()),
            _ => None,
        })
    }
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for value in args {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}
fn call(code: &mut Vec<u8>, index: u32, args: &[u32], output: Option<u32>) {
    for arg in args {
        ins(code, 0x25, &[*arg]);
    }
    ins(code, 0x2b, &[index, args.len() as u32 * 4]);
    if let Some(address) = output {
        ins(code, 0x26, &[address]);
    }
}
struct Loader;
impl ProgramLoader for Loader {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        let mut data = vec![0x5a; 256];
        let text = encoding_rs::SHIFT_JIS.encode("喜\0").0;
        data[128..128 + text.len()].copy_from_slice(&text);
        let mut code = Vec::new();
        ins(&mut code, 0x25, &[0x12345678]);
        if name == b"loaded" {
            call(&mut code, 1055, &[u32::MAX], None);
            call(&mut code, 1057, &[u32::MAX], Some(0));
            call(&mut code, 1058, &[0, 0x80000040], None);
            call(&mut code, 1059, &[0, 64], None);
            call(&mut code, 1060, &[96], None);
            call(&mut code, 1056, &[128, 7, 10, 20, 8, 12, 16, 160], None);
            call(&mut code, 1061, &[9, 2, 5, 20, 24], None);
            call(&mut code, 1058, &[u32::MAX, u32::MAX], None);
            ins(&mut code, 0x26, &[28]);
            call(&mut code, 65, &[], None);
            call(&mut code, 1055, &[], None);
            call(&mut code, 1057, &[], Some(4));
        } else {
            call(&mut code, 1057, &[], Some(4));
        }
        call(&mut code, 1060, &[192], None);
        call(&mut code, 65, &[], None);
        code.push(0x3b);
        Ok(Some(Program::from_code(&code, data, 64)?))
    }
}

#[test]
fn native_family_scales_metadata_and_clears_only_its_task_on_reload_and_release() {
    for budget in [1, 4096] {
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory {
                programs: Loader,
                external: katze_system2::memory::SharedMemory::new(vec![0; 256]).unwrap(),
            },
        );
        rt.set_budget(budget, 1 << 20);
        let available = Rc::new(Cell::new(true));
        rt.set_resources(Files(available.clone()));
        let loaded = rt.create(0, kind::ORDINARY, b"loaded", b"").unwrap();
        rt.update(0, 1).unwrap();
        let vm = rt.object(loaded).unwrap().interpreter().unwrap();
        let dword =
            |address| u32::from_le_bytes(vm.read(address, 4).unwrap()[..].try_into().unwrap());
        assert_eq!(dword(0), 2);
        assert_eq!(
            [dword(8) as i32, dword(12) as i32, dword(16) as i32],
            [-18, 85, 80]
        );
        assert_eq!([dword(20) as i32, dword(24) as i32], [-7, 12]);
        assert_eq!(dword(28), 0x12345678);
        assert_eq!(vm.read(160, 6).unwrap().as_ref(), b"small\0");
        for (address, value) in [(64, "喜ラベル\0"), (96, "停止\0"), (0x80000040, "喜\0")] {
            let bytes = encoding_rs::SHIFT_JIS.encode(value).0;
            assert_eq!(
                vm.read(address, bytes.len()).unwrap().as_ref(),
                bytes.as_ref()
            );
        }
        available.set(false);
        let empty = rt.create(0, kind::ORDINARY, b"empty", b"").unwrap();
        rt.update(1, 1).unwrap();
        let vm = rt.object(loaded).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(4, 4).unwrap().as_ref(), &[0; 4]);
        let stop = encoding_rs::SHIFT_JIS.encode("停止\0").0;
        assert_eq!(vm.read(192, stop.len()).unwrap().as_ref(), stop.as_ref());
        let vm = rt.object(empty).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(4, 4).unwrap().as_ref(), &[0; 4]);
        assert_eq!(vm.read(192, 1).unwrap().as_ref(), &[0]);
        rt.destroy(loaded).unwrap();
        rt.update(2, 1).unwrap();
        let reused = rt.create(0, kind::ORDINARY, b"empty", b"").unwrap();
        assert_eq!(loaded, reused);
        rt.update(3, 1).unwrap();
        let vm = rt.object(reused).unwrap().interpreter().unwrap();
        assert_eq!(vm.read(192, 1).unwrap().as_ref(), &[0]);
    }
}
