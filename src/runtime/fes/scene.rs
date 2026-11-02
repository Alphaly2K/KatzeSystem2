use super::Objects;
use crate::{
    Error, HandleAllocator, Result, Step, fes,
    runtime::{ObjectFactory, Runtime, Store},
};
use std::collections::VecDeque;

/// 一份预先装载的 FES 执行状态；调用方绑定共享列表并在所属任务的主体时点推进。
pub struct Scene {
    interpreter: fes::Interpreter,
    objects: Objects,
    owner: Option<u32>,
    frame_origin: u32,
    deadline: Option<u32>,
    exit_code: Option<i32>,
    exit_request: Option<i32>,
    queued_exits: VecDeque<i32>,
    invocation: Option<Invocation>,
    input: super::input::SystemInput,
    system_block: Option<String>,
    media_preparation: Option<usize>,
    media_prepared: usize,
}

struct Invocation {
    now: u32,
    updates: Option<(usize, usize)>,
    resume_script: bool,
}

impl Scene {
    pub fn new(program: fes::Program, handles: HandleAllocator) -> Result<Self> {
        let mut interpreter = fes::Interpreter::new(program)?;
        let objects = Objects::new(handles)?;
        objects.publish_bindings(&mut interpreter);
        for name in [
            "_CLICK_L_",
            "_CLICK_R_",
            "_SCRL_U_",
            "_SCRL_D_",
            "_PUSH_UP_",
            "_PUSH_DOWN_",
            "_PUSH_RIGHT_",
            "_PUSH_LEFT_",
        ] {
            interpreter
                .named
                .entry(name.to_owned())
                .or_default()
                .insert(0, 0);
        }
        Ok(Self {
            interpreter,
            objects,
            owner: None,
            frame_origin: 0,
            deadline: None,
            exit_code: None,
            exit_request: None,
            queued_exits: VecDeque::new(),
            invocation: None,
            input: super::input::SystemInput::default(),
            system_block: None,
            media_preparation: None,
            media_prepared: 0,
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
        if self.is_pending() {
            return Err(Error::new(0, "FES 本轮推进尚未完成"));
        }
        self.media_prepared = 0;
        Ok(&mut self.objects)
    }

    pub fn deadline(&self) -> Option<u32> {
        self.deadline
    }

    pub fn raw_input(&self) -> u32 {
        self.input.raw
    }

    pub(super) fn set_raw_input(&mut self, value: u32) {
        self.input.raw = value;
    }

    pub(super) fn record_input_event(&mut self, event: u32) {
        if let Some((name, value)) = self.input.record(event) {
            self.set_system_value(name, value);
        }
    }

    /// 最近已处理的退出通知；本轮最终对象更新完成后可用，不表示脚本停止。
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code.filter(|_| !self.is_pending())
    }

    pub(super) fn take_exit_request(&mut self) -> Option<i32> {
        self.exit_request.take()
    }

    fn publish_exit(&mut self, code: i32) {
        self.exit_code = Some(code);
        self.exit_request = Some(code);
    }

    fn publish_queued_exits(&mut self) {
        while let Some(code) = self.queued_exits.pop_front() {
            self.publish_exit(code);
        }
    }

    pub(super) fn set_system_value(&mut self, name: &str, value: i32) {
        self.interpreter
            .named
            .entry(name.to_owned())
            .or_default()
            .insert(0, value);
    }

    pub fn is_pending(&self) -> bool {
        self.invocation.is_some() || self.system_block.is_some() || self.media_preparation.is_some()
    }

    /// 只处理装载期声明和属性，不进入 START 或推进下层媒体对象。
    /// 返回 false 时保留初始化游标，下一次调用继续处理。
    pub fn prepare(
        &mut self,
        numbers: &mut crate::expression::State,
        budget: usize,
    ) -> Result<bool> {
        if self.is_pending() {
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

    fn finish_input(&mut self) {
        if self.input.finish() {
            for name in super::input::TRANSIENT_NAMES {
                self.set_system_value(name, 0);
            }
        }
    }

    fn begin_updates(&mut self, resume_script: bool) {
        let invocation = self.invocation.as_mut().unwrap();
        invocation.updates = Some((self.objects.update_order.len(), 0));
        invocation.resume_script = resume_script;
    }
}

impl<F: ObjectFactory> Runtime<F> {
    /// 完成装载期声明、属性和下层媒体创建；false 后继续调用，尚未执行 START。
    pub fn prepare_fes_scene(&mut self, owner: u32, scene: &mut Scene) -> Result<bool> {
        self.store().prepare_fes_scene(owner, scene)
    }

    /// 同步推进睡眠／唤醒系统块；false 后用同一名称继续预算续跑。
    pub fn dispatch_fes_system_block(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        name: &str,
    ) -> Result<bool> {
        self.store().dispatch_fes_system_block(owner, scene, name)
    }

    /// 推进脚本并在本轮边界更新 FES 对象；false 表示预算耗尽，续跑保留原时刻。
    pub fn update_fes_scene(&mut self, owner: u32, scene: &mut Scene, now: u32) -> Result<bool> {
        self.store().update_fes_scene(owner, scene, now)
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(in crate::runtime) fn prepare_fes_scene(
        &mut self,
        owner: u32,
        scene: &mut Scene,
    ) -> Result<bool> {
        if scene.invocation.is_some() || scene.system_block.is_some() {
            return Err(Error::new(0, "FES 本轮推进尚未完成"));
        }
        if self.tree.node(owner).is_none() || scene.owner.is_some_and(|bound| bound != owner) {
            return Err(Error::new(0, "FES 装载所属任务无效"));
        }
        scene.owner = Some(owner);
        if scene.media_preparation.is_none() && !scene.prepare(self.numbers, self.budget)? {
            return Ok(false);
        }
        self.prepare_fes_media(owner, scene)
    }

    fn prepare_fes_media(&mut self, owner: u32, scene: &mut Scene) -> Result<bool> {
        if scene.media_prepared == scene.objects.update_order.len() {
            return Ok(true);
        }
        let cursor = scene.media_preparation.get_or_insert(scene.media_prepared);
        for _ in 0..self.budget.max(1) {
            if *cursor < scene.objects.update_order.len() {
                self.prepare_fes_object(owner, &mut scene.objects, *cursor)?;
                *cursor += 1;
            } else {
                scene.media_prepared = *cursor;
                scene.media_preparation = None;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(in crate::runtime) fn set_fes_media_sleep(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        sleeping: bool,
    ) -> Result<()> {
        if scene.is_pending() {
            return Err(Error::new(0, "FES 推进尚未完成，不能切换媒体睡眠状态"));
        }
        let handles = scene.objects.update_order().collect::<Vec<_>>();
        for index in 0..scene.objects.update_order.len() {
            self.update_fes_record(owner, &mut scene.objects, index)?;
        }
        for handle in handles {
            let object = scene.objects.get(handle).unwrap();
            if let super::ObjectState::Button(button) = &object.state {
                self.fes_command(
                    owner,
                    object.task_id,
                    93,
                    [0, if sleeping { 0 } else { button.enabled as u32 }, 0],
                    None,
                )?;
            }
            if let Some(image) = object.state.image() {
                self.fes_command(
                    owner,
                    scene.objects.image_list,
                    293,
                    [
                        image.parent_priority as u32,
                        if sleeping { 0 } else { image.display as u32 },
                        0,
                    ],
                    None,
                )?;
            } else if let Some(plane) = object.state.plane() {
                self.fes_command(
                    owner,
                    object.task_id,
                    47,
                    [0, if sleeping { 0 } else { plane.display() as u32 }, 0],
                    None,
                )?;
            }
        }
        if !sleeping {
            self.fes_command(owner, scene.objects.image_list, 298, [0; 3], None)?;
        }
        Ok(())
    }

    pub(in crate::runtime) fn dispatch_fes_system_block(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        name: &str,
    ) -> Result<bool> {
        if !matches!(name, "_FES_SLEEP_" | "_FES_WAKE_") {
            return Err(Error::new(0, "FES 系统块名称尚未接入"));
        }
        if self.tree.node(owner).is_none() || scene.owner.is_some_and(|bound| bound != owner) {
            return Err(Error::new(0, "FES 系统块所属任务无效"));
        }
        if scene.invocation.is_some() || scene.media_preparation.is_some() {
            return Err(Error::new(0, "FES 本轮推进尚未完成"));
        }
        if let Some(pending) = &scene.system_block {
            if pending != name {
                return Err(Error::new(0, "FES 上一次系统块尚未完成"));
            }
        } else {
            scene.owner = Some(owner);
            if !scene.interpreter.dispatch_system_block(name)? {
                return Ok(true);
            }
            scene.system_block = Some(name.to_owned());
        }
        for _ in 0..self.budget.max(1) {
            match scene.interpreter.run(self.numbers, 1)? {
                Step::Yield => (),
                Step::Event(fes::Event::Wait(_)) => scene.interpreter.finish_system_block()?,
                Step::Event(fes::Event::Command(line)) => {
                    let args = scene.interpreter.parameters(&line.bytes, self.numbers)?;
                    if args.first().is_some_and(|name| name == b"decode") {
                        for index in 0..scene.objects.update_order.len() {
                            self.update_fes_record(owner, &mut scene.objects, index)?;
                        }
                    } else if !scene.objects.dispatch_image_animation(
                        &mut scene.interpreter,
                        &line,
                        self.numbers,
                    )? && !scene.objects.dispatch_object_command(
                        &mut scene.interpreter,
                        &line,
                        self.numbers,
                    )? {
                        return Err(Error::new(line.offset, "FES 系统块对象命令尚未接入"));
                    }
                    scene.interpreter.resume();
                }
                Step::Event(fes::Event::InitializeObject(initialization)) => {
                    scene
                        .objects
                        .initialize(&initialization, |name| Ok(name.to_vec()))?;
                    scene.interpreter.resume();
                }
                Step::Event(fes::Event::Exit(code)) => {
                    if scene.queued_exits.len() >= crate::read::MAX_ITEMS {
                        return Err(Error::new(0, "FES 系统块退出事件队列超过上限"));
                    }
                    scene.queued_exits.push_back(code);
                    scene.interpreter.finish_system_block()?;
                }
                Step::Event(fes::Event::Subcommand(call)) => {
                    return Err(Error::new(call.offset, "FES 系统块运动服务尚未接入"));
                }
                _ => return Err(Error::new(0, "FES 系统块出现未支持的执行状态")),
            }
            if let Some(transferred) = scene.interpreter.take_dispatch_result() {
                if transferred {
                    scene.deadline = None;
                }
                scene.system_block = None;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(in crate::runtime) fn update_fes_scene(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        now: u32,
    ) -> Result<bool> {
        if scene.system_block.is_some() {
            return Err(Error::new(0, "FES 系统块尚未完成"));
        }
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
            if scene
                .deadline
                .is_some_and(|deadline| now < deadline && !scene.input.left)
            {
                scene.publish_queued_exits();
                scene.finish_input();
                scene.begin_updates(false);
            } else {
                scene.deadline = None;
            }
        }
        for _ in 0..self.budget {
            if !scene.interpreter.initialization_pending()
                && scene.media_prepared != scene.objects.update_order.len()
                && !self.prepare_fes_media(owner, scene)?
            {
                return Ok(false);
            }
            if let Some((count, cursor)) = &mut scene.invocation.as_mut().unwrap().updates {
                if *cursor < *count {
                    self.update_fes_record(owner, &mut scene.objects, *cursor)?;
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
                Step::Paused | Step::Halted => {
                    return Err(Error::new(0, "FES 解释器出现未支持的停止状态"));
                }
                Step::Event(event) => {
                    scene.publish_queued_exits();
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
                            )? && !scene.objects.dispatch_object_command(
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
                            scene.finish_input();
                            scene.begin_updates(false);
                        }
                        fes::Event::Frame => {
                            scene.deadline = Some(now.wrapping_add(1));
                            scene.finish_input();
                            scene.begin_updates(false);
                        }
                        fes::Event::Exit(code) => {
                            scene.publish_exit(code);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{BaseObject, TaskObject, kind};

    struct NoObjects;
    impl ObjectFactory for NoObjects {
        fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
            Err(Error::new(0, "合成场景不创建任务"))
        }
    }

    #[test]
    fn exit_continues_to_the_round_boundary_and_clears_clicks_but_keeps_release_and_touch() {
        for budget in [1, 4096] {
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                crate::expression::State::new(0),
                NoObjects,
            );
            runtime.set_budget(budget, 10000);
            let mut scene = Scene::new(
                fes::Program::from_text(b"#START\nexit 5\n").unwrap(),
                runtime.host().handle_allocator(),
            )
            .unwrap();
            scene.set_raw_input(16);
            for event in [2, 15, 21] {
                scene.record_input_event(event);
            }
            for now in [100, 101] {
                let mut complete = false;
                for _ in 0..16 {
                    if runtime.update_fes_scene(0, &mut scene, now).unwrap() {
                        complete = true;
                        break;
                    }
                }
                assert!(complete);
                assert_eq!(scene.interpreter().named["_CLICK_L_"][&0], 0);
                assert_eq!(scene.interpreter().named["_RELEASE_UP_"][&0], -1);
                assert_eq!(scene.interpreter().named["_TOUCH_"][&0], -1);
                assert_eq!(scene.raw_input(), 16);
                assert_eq!(scene.exit_code(), Some(5));
            }
        }
    }
}
