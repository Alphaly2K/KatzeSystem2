use katze_system2::{
    Result,
    expression::State,
    kcs::Program,
    runtime::{
        BaseObject, KcsFactory, PlatformJob, PlatformJobRequest, PlatformJobServices,
        ProgramLoader, Runtime, kind,
    },
    task::Command,
};
use std::{cell::Cell, cell::RefCell, rc::Rc};

struct Loader(Program);

impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn write_invocation(code: &mut Vec<u8>, index: u32, args: &[u32]) {
    for arg in args {
        code.extend([0x25, 3]);
        code.extend(arg.to_le_bytes());
    }
    code.extend([0x2b, 3]);
    code.extend(index.to_le_bytes());
    code.push(3);
    code.extend((args.len() as u32 * 4).to_le_bytes());
}

fn runtime(calls: &[(u32, &[u32])]) -> Result<Runtime<KcsFactory<Loader>>> {
    let mut code = Vec::new();
    for &(index, args) in calls {
        write_invocation(&mut code, index, args);
    }
    code.push(0x3b);
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Loader(Program::from_code(
            &code,
            b"run%%\0second\0".to_vec(),
            64,
        )?)),
    );
    runtime.set_budget(1, 1024);
    assert_eq!(runtime.create(0, kind::PLATFORM_PLUGIN, b"", b"")?, 1);
    assert_eq!(runtime.create(0, kind::IMAGE_IMPORT, b"", b"")?, 2);
    Ok(runtime)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Request {
    Plugin(u32, Vec<u8>),
    Import(u32),
}

#[derive(Clone, Default)]
struct JobState {
    result: Rc<Cell<Option<u32>>>,
    dropped: Rc<Cell<bool>>,
}

impl PlatformJob for JobState {
    fn poll(&mut self) -> Option<u32> {
        self.result.get()
    }
}

impl Drop for JobState {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

struct Services {
    requests: Rc<RefCell<Vec<Request>>>,
    plugin: JobState,
    import: JobState,
}

impl PlatformJobServices for Services {
    fn start(
        &mut self,
        task_id: u32,
        request: PlatformJobRequest<'_>,
    ) -> Result<Option<Box<dyn PlatformJob>>> {
        let (event, job) = match request {
            PlatformJobRequest::Plugin(text) => {
                (Request::Plugin(task_id, text.to_vec()), &self.plugin)
            }
            PlatformJobRequest::ImageImport => (Request::Import(task_id), &self.import),
        };
        self.requests.borrow_mut().push(event);
        Ok(Some(Box::new(job.clone())))
    }
}

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[test]
fn platform_jobs_launch_once_return_pending_and_complete_without_window() -> Result<()> {
    let mut runtime = runtime(&[
        (1004, &[1]),
        (1005, &[1]),
        (1008, &[2]),
        (1003, &[1, u32::MAX]),
        (1004, &[1]),
        (1003, &[1, 0]),
        (1003, &[1, 6]),
        (1004, &[1]),
        (1005, &[1]),
        (1007, &[2]),
        (1007, &[2]),
        (1008, &[2]),
    ])?;
    let requests = Rc::new(RefCell::new(Vec::new()));
    let plugin = JobState::default();
    let import = JobState::default();
    runtime.set_platform_job_services(Services {
        requests: requests.clone(),
        plugin: plugin.clone(),
        import: import.clone(),
    });
    let caller = runtime.create(0, kind::ORDINARY, b"caller", b"")?;
    runtime.update(0, 1)?;
    let vm = runtime.object(caller).unwrap().interpreter().unwrap();
    let initial = (-2i32) as u32;
    assert_eq!(vm.stack_bytes(), words(&[1, initial, 1, 1, 0, initial, 0]));
    assert_eq!(
        *requests.borrow(),
        [Request::Plugin(1, b"run%".to_vec()), Request::Import(2)]
    );
    assert!(!plugin.dropped.get() && !import.dropped.get());

    plugin.result.set(Some(0x8000_0004));
    runtime.update(1, 1)?;
    let mut command = |target, code| {
        runtime.dispatch_command(
            0,
            Command {
                target,
                sender: caller,
                code,
                data: [0; 3],
            },
        )
    };
    assert_eq!(command(1, 574)?, 1);
    assert_eq!(command(1, 575)?, 0x8000_0004);
    command(1, 573)?;
    assert_eq!(command(2, 577)?, 0);
    assert!(plugin.dropped.get() && !import.dropped.get());
    runtime.destroy(2)?;
    assert!(import.dropped.get());
    assert_eq!(requests.borrow().len(), 2);
    Ok(())
}

#[test]
fn platform_job_fallbacks_distinguish_missing_service_and_unwritten_result() -> Result<()> {
    let mut no_service = runtime(&[
        (1003, &[1, 0]),
        (1007, &[2]),
        (1004, &[1]),
        (1005, &[1]),
        (1008, &[2]),
        (1004, &[99]),
        (1005, &[99]),
    ])?;
    let caller = no_service.create(0, kind::ORDINARY, b"caller", b"")?;
    no_service.update(0, 1)?;
    let vm = no_service.object(caller).unwrap().interpreter().unwrap();
    assert_eq!(vm.stack_bytes(), words(&[1, (-3i32) as u32, 1, 0, 0]));

    for target in [0, 1, 99] {
        let mut missing = runtime(&[(1008, &[target])])?;
        missing.create(0, kind::ORDINARY, b"caller", b"")?;
        assert!(
            missing
                .update(0, 1)
                .unwrap_err()
                .to_string()
                .contains("图片导入查询")
        );
    }
    Ok(())
}
