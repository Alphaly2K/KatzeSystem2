use super::Objects;
use crate::{
    Error, HandleAllocator, Result, Step, fes,
    runtime::{ObjectFactory, Runtime, Store},
};

/// 一份预先装载的 FES 执行状态；调用方绑定共享列表并在所属任务的主体时点推进。
pub struct Scene {
    interpreter: fes::Interpreter,
    objects: Objects,
    owner: Option<u32>,
    frame_origin: u32,
    deadline: Option<u32>,
    exit_code: Option<i32>,
    invocation: Option<Invocation>,
}

struct Invocation {
    now: u32,
    updates: Option<(Vec<u32>, usize)>,
    resume_script: bool,
}

impl Scene {
    pub fn new(program: fes::Program, handles: HandleAllocator) -> Result<Self> {
        let mut interpreter = fes::Interpreter::new(program)?;
        let objects = Objects::new(handles)?;
        objects.publish_bindings(&mut interpreter);
        Ok(Self {
            interpreter,
            objects,
            owner: None,
            frame_origin: 0,
            deadline: None,
            exit_code: None,
            invocation: None,
        })
    }

    pub fn interpreter(&self) -> &fes::Interpreter {
        &self.interpreter
    }

    pub fn objects(&self) -> &Objects {
        &self.objects
    }

    /// 同一次预算续跑期间保持对象绑定和属性稳定。
    pub fn objects_mut(&mut self) -> Result<&mut Objects> {
        if self.invocation.is_some() {
            return Err(Error::new(0, "FES 本轮推进尚未完成"));
        }
        Ok(&mut self.objects)
    }

    pub fn deadline(&self) -> Option<u32> {
        self.deadline
    }

    /// 退出码在最终对象更新完成后可用；退出后仍可逐轮更新媒体对象。
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code.filter(|_| self.invocation.is_none())
    }

    pub(super) fn exit_request(&self) -> Option<i32> {
        self.exit_code
    }

    pub(super) fn set_system_value(&mut self, name: &str, value: i32) {
        self.interpreter
            .named
            .entry(name.to_owned())
            .or_default()
            .insert(0, value);
    }

    pub fn is_pending(&self) -> bool {
        self.invocation.is_some()
    }

    /// 只处理装载期声明和属性，不进入 START 或推进下层媒体对象。
    /// 返回 false 时保留初始化游标，下一次调用继续处理。
    pub fn prepare(
        &mut self,
        numbers: &mut crate::expression::State,
        budget: usize,
    ) -> Result<bool> {
        if self.invocation.is_some() {
            return Err(Error::new(0, "FES 本轮推进尚未完成"));
        }
        for _ in 0..budget.max(1) {
            if !self.interpreter.initialization_pending() {
                return Ok(true);
            }
            match self.interpreter.run(numbers, 1)? {
                Step::Yield => (),
                Step::Event(event) if self.apply_initialization(&event)? => {
                    self.interpreter.resume();
                }
                _ => return Err(Error::new(0, "FES 装载期出现执行事件")),
            }
        }
        Ok(!self.interpreter.initialization_pending())
    }

    fn apply_initialization(&mut self, event: &fes::Event) -> Result<bool> {
        match event {
            fes::Event::CreateObject(definition) => {
                self.objects.define(definition)?;
                self.objects.publish_bindings(&mut self.interpreter);
            }
            fes::Event::InitializeObject(initialization) => {
                self.objects
                    .initialize(initialization, |name| Ok(name.to_vec()))?;
            }
            fes::Event::Declarations(declarations) => {
                if let Some(declaration) = declarations
                    .iter()
                    .find(|declaration| declaration.header.as_slice() != b"SUBCOMMAND")
                {
                    return Err(Error::new(
                        declaration.lines.first().map_or(0, |line| line.offset),
                        "FES 声明需要对应的运行时服务",
                    ));
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn begin_updates(&mut self, resume_script: bool) {
        let invocation = self.invocation.as_mut().unwrap();
        invocation.updates = Some((self.objects.update_order().collect(), 0));
        invocation.resume_script = resume_script;
    }
}

impl<F: ObjectFactory> Runtime<F> {
    /// 推进脚本并在本轮边界更新 FES 对象；false 表示预算耗尽，续跑保留原时刻。
    pub fn update_fes_scene(&mut self, owner: u32, scene: &mut Scene, now: u32) -> Result<bool> {
        self.store().update_fes_scene(owner, scene, now)
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(in crate::runtime) fn update_fes_scene(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        now: u32,
    ) -> Result<bool> {
        if self.tree.node(owner).is_none() {
            return Err(Error::new(0, "FES 所属任务不存在"));
        }
        if scene.owner.is_some_and(|bound| bound != owner) {
            return Err(Error::new(0, "FES 执行状态已绑定其他所属任务"));
        }
        scene.owner = Some(owner);
        if scene.invocation.is_none() {
            if scene
                .deadline
                .is_some_and(|deadline| deadline.wrapping_add(7) < 7)
            {
                return Err(Error::new(0, "FES 特殊等待期限需要对象完成状态服务"));
            }
            if scene.frame_origin == 0 {
                scene.frame_origin = now.wrapping_sub(1);
            }
            scene
                .interpreter
                .named
                .entry("_FRAME_".to_owned())
                .or_default()
                .insert(0, now.wrapping_sub(scene.frame_origin) as i32);
            scene.invocation = Some(Invocation {
                now,
                updates: None,
                resume_script: false,
            });
            if scene.exit_code.is_some() || scene.deadline.is_some_and(|deadline| now < deadline) {
                scene.begin_updates(false);
            } else {
                scene.deadline = None;
            }
        }
        for _ in 0..self.budget {
            if let Some((handles, cursor)) = &mut scene.invocation.as_mut().unwrap().updates {
                if let Some(&handle) = handles.get(*cursor) {
                    self.update_fes_object(owner, &mut scene.objects, handle)?;
                    *cursor += 1;
                } else {
                    if scene.invocation.as_ref().unwrap().resume_script {
                        scene.interpreter.resume();
                        scene.invocation.as_mut().unwrap().updates = None;
                        continue;
                    }
                    scene.invocation = None;
                    return Ok(true);
                }
                continue;
            }
            match scene.interpreter.run(self.numbers, 1)? {
                Step::Yield => (),
                Step::Paused => return Err(Error::new(0, "FES 解释器出现未支持的暂停状态")),
                Step::Halted => {
                    scene.exit_code = Some(scene.interpreter.return_value);
                    scene.begin_updates(false);
                }
                Step::Event(event) => {
                    let now = scene.invocation.as_ref().unwrap().now;
                    if scene.apply_initialization(&event)? {
                        scene.interpreter.resume();
                        continue;
                    }
                    match event {
                        fes::Event::Command(line) => {
                            let args = scene.interpreter.parameters(&line.bytes, self.numbers)?;
                            if args.first().is_some_and(|name| name == b"decode") {
                                scene.begin_updates(true);
                                continue;
                            }
                            if !scene.objects.dispatch_image_animation(
                                &mut scene.interpreter,
                                &line,
                                self.numbers,
                            )? {
                                return Err(Error::new(line.offset, "FES 对象命令尚未接入"));
                            }
                        }
                        fes::Event::Subcommand(call) => {
                            return Err(Error::new(call.offset, "FES 运动服务尚未接入"));
                        }
                        fes::Event::Wait(duration) => {
                            if duration < 0 {
                                return Err(Error::new(0, "FES 特殊等待尚未接入"));
                            }
                            scene.deadline = Some(now.wrapping_add(duration.max(1) as u32));
                            scene.begin_updates(false);
                        }
                        fes::Event::Frame => {
                            scene.deadline = Some(now.wrapping_add(1));
                            scene.begin_updates(false);
                        }
                        fes::Event::Exit(code) => {
                            scene.exit_code = Some(code);
                            scene.begin_updates(false);
                        }
                        fes::Event::CreateObject(_)
                        | fes::Event::InitializeObject(_)
                        | fes::Event::Declarations(_) => unreachable!(),
                    }
                    scene.interpreter.resume();
                }
            }
        }
        Ok(false)
    }
}
