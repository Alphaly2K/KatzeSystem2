use katze_system2::{
    Result, Step,
    expression::State,
    kcs::{CachedResources, CoreHost, Interpreter, Program, ResourceHost, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
struct Files(Rc<RefCell<Vec<Vec<u8>>>>);
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        Ok([category, b"/", name].concat())
    }
    fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
        let mut requests = self.0.borrow_mut();
        requests.push(name.to_vec());
        Ok(match name {
            b"missing" => None,
            b"empty" => Some(Vec::new()),
            _ => Some(vec![requests.len() as u8; 4]),
        })
    }
}

#[test]
fn source_opening_forwards_host_overrides_and_keeps_cached_sources_after_eviction() {
    #[derive(Default)]
    struct Opens {
        reads: usize,
        opens: usize,
    }
    impl ResourceServices for Opens {
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            self.reads += 1;
            Ok(Some(vec![1; 4]))
        }
        fn open(
            &mut self,
            _: &[u8],
            _: ResourceMode,
        ) -> Result<Option<katze_system2::io::SharedSource>> {
            self.opens += 1;
            Ok(Some(std::sync::Arc::new(vec![9; 4])))
        }
    }
    let mut inner = Opens::default();
    let mut files = CachedResources::new(&mut inner);
    let direct = files.open(b"x", ResourceMode::File).unwrap().unwrap();
    let mut bytes = [0; 4];
    direct.read_exact_at(0, &mut bytes).unwrap();
    assert_eq!(bytes, [9; 4]);
    assert_eq!((files.opens, files.reads), (1, 0));
    let cache = files.resource_cache().unwrap();
    cache.enqueue(b"x", 1).unwrap();
    cache.start();
    assert!(cache.advance(&mut files).unwrap());
    let cached = files.open(b"X", ResourceMode::Archive).unwrap().unwrap();
    assert_eq!((files.opens, files.reads), (1, 1));
    cache.clear();
    cached.read_exact_at(0, &mut bytes).unwrap();
    assert_eq!(bytes, [1; 4]);
    direct.read_exact_at(0, &mut bytes).unwrap();
    assert_eq!(bytes, [9; 4]);
}

#[test]
fn prefetch_groups_renew_life_without_reordering_and_keep_copies_after_eviction() {
    let source = Files::default();
    let mut files = CachedResources::new(source.clone());
    let cache = files.resource_cache().unwrap();
    cache.enqueue(b"A", 1).unwrap();
    cache.enqueue(b"B", 0).unwrap();
    cache.enqueue(b"a", 5).unwrap();
    cache.start();
    assert!(!cache.advance(&mut files).unwrap());
    assert_eq!(*source.0.borrow(), [b"B".to_vec()]);
    assert!(cache.is_ready(0));
    assert!(!cache.is_ready(1));
    assert!(cache.advance(&mut files).unwrap());
    assert_eq!(*source.0.borrow(), [b"B".to_vec(), b"A".to_vec()]);
    let mut clone = files.clone();
    let copy = clone.read(b"a", ResourceMode::Archive).unwrap().unwrap();
    assert_eq!(copy, [2; 4]);
    cache.age();
    assert_eq!(cache.len(), 1);
    assert!(!cache.is_ready(i32::MAX));
    assert_eq!(cache.read_cached(b"A").unwrap(), Some(copy.clone()));
    cache.clear();
    assert_eq!(
        clone.read(b"A", ResourceMode::File).unwrap(),
        Some(vec![3; 4])
    );
    assert_eq!(copy, [2; 4]);

    cache.enqueue(b"missing", 2).unwrap();
    cache.start();
    assert!(cache.advance(&mut files).unwrap());
    cache.start();
    assert!(cache.advance(&mut files).unwrap());
    assert_eq!(source.0.borrow().len(), 4);
    assert_eq!(
        files.read(b"missing", ResourceMode::LooseFirst).unwrap(),
        None
    );
    assert_eq!(source.0.borrow().len(), 5);
    cache.clear();
    cache.enqueue(b"empty", 1).unwrap();
    cache.start();
    assert!(cache.advance(&mut files).unwrap());
    assert_eq!(
        files.read(b"empty", ResourceMode::LooseFirst).unwrap(),
        Some(vec![])
    );
    assert_eq!(source.0.borrow().len(), 6);
    cache.clear();
    cache.enqueue(b"wrap", i32::MIN).unwrap();
    cache.enqueue(b"zero", 0).unwrap();
    cache.age();
    assert_eq!(cache.len(), 1);
    cache.start();
    assert!(cache.advance(&mut files).unwrap());
    assert_eq!(cache.read_cached(b"wrap").unwrap(), Some(vec![7; 4]));
}

fn instruction(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for value in args {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}
fn program() -> Program {
    let mut code = Vec::new();
    instruction(&mut code, 0x25, &[0x12345678]);
    for (native, args, output) in [
        (434, vec![], None),
        (435, vec![64, 96, 2], None),
        (436, vec![], None),
        (437, vec![i32::MAX as u32], Some(16)),
        (365, vec![64], Some(0)),
        (438, vec![], None),
        (365, vec![128], Some(4)),
        (434, vec![], None),
        (365, vec![64], Some(8)),
    ] {
        for &value in &args {
            instruction(&mut code, 0x25, &[value]);
        }
        instruction(&mut code, 0x2b, &[native, args.len() as u32 * 4]);
        if let Some(output) = output {
            instruction(&mut code, 0x26, &[output]);
        }
    }
    instruction(&mut code, 0x26, &[12]);
    instruction(&mut code, 0x3b, &[]);
    let mut data = vec![0; 256];
    data[64..68].copy_from_slice(b"art\0");
    data[96..102].copy_from_slice(b"image\0");
    data[128..132].copy_from_slice(b"ART\0");
    Program::from_code(&code, data, 128).unwrap()
}
struct Single(Program);
impl ProgramLoader for Single {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}
fn outputs(vm: &Interpreter) -> [u32; 3] {
    assert_eq!(
        vm.read(12, 8).unwrap(),
        [0x12345678u32, 1].map(u32::to_le_bytes).concat()
    );
    std::array::from_fn(|i| {
        u32::from_le_bytes(vm.read(i as u32 * 4, 4).unwrap().try_into().unwrap())
    })
}

#[test]
fn prefetch_natives_share_cached_bytes_with_raw_images_in_both_hosts() {
    for budget in [1, 4096] {
        let mut files = Files::default();
        let requests = files.0.clone();
        let mut numbers = State::new(0);
        let mut core = CoreHost::new(&mut numbers);
        let mut host = ResourceHost::new(&mut core, &mut files);
        let mut vm = Interpreter::new(program(), vec![]).unwrap();
        let mut done = false;
        for _ in 0..256 {
            match vm.run(&mut host, budget).unwrap() {
                Step::Yield => (),
                Step::Halted => {
                    done = true;
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(done);
        let handles = outputs(&vm);
        assert_ne!(handles[0], handles[1]);
        for (handle, value) in handles.into_iter().zip([1, 1, 2]) {
            assert_eq!(host.image_data(handle).unwrap(), [value; 4]);
        }
        assert_eq!(requests.borrow().len(), 2);

        let files = Files::default();
        let requests = files.0.clone();
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Single(program())),
        );
        runtime.set_resources(files);
        runtime.set_budget(budget, 1 << 20);
        let task = runtime.create(0, kind::ORDINARY, b"prefetch", b"").unwrap();
        runtime.update(0, 1).unwrap();
        let handles = outputs(runtime.object(task).unwrap().interpreter().unwrap());
        runtime.destroy(task).unwrap();
        for (handle, value) in handles.into_iter().zip([1, 1, 2]) {
            assert_eq!(runtime.image_data(handle).unwrap(), [value; 4]);
        }
        assert_eq!(requests.borrow().len(), 2);
    }
}
