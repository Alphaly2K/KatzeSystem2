use super::Objects;
use crate::{
    Error, HandleAllocator, Result, Step, fes,
    runtime::{ObjectFactory, Runtime, Store},
};
use std::collections::{BTreeMap, VecDeque};

mod scripts;
mod wait;

use scripts::{ChildScripts, ScriptDisposition, ScriptRequest};

/// 一份预先装载的 FES 执行状态；调用方绑定共享列表并在所属任务的主体时点推进。
pub struct Scene {
    interpreter: fes::Interpreter,
    pub(super) objects: Objects,
    owner: Option<u32>,
    frame_origin: u32,
    deadline: Option<u32>,
    frame_wait: u32,
    wait_target: Option<fes::ObjectReference>,
    exit_code: Option<i32>,
    exit_request: Option<i32>,
    notifications: VecDeque<Notification>,
    scripts: ChildScripts,
    preferred_focus: Option<fes::ObjectReference>,
    focus_lock: i32,
    invocation: Option<Invocation>,
    input: super::input::SystemInput,
    system_block: Option<String>,
    object_event: Option<ObjectEvent>,
    deferred_decodes: usize,
    keyblocks: Vec<fes::KeyBlock>,
    media_preparation: Option<usize>,
    media_prepared: usize,
    motion_names: BTreeMap<Vec<u8>, Vec<u8>>,
    motion_call: Option<MotionCall>,
    motion_stop: Option<MotionStop>,
}

struct Invocation {
    now: u32,
    updates: Option<(usize, usize)>,
    resume_script: bool,
    motions_done: bool,
    media_passes: usize,
    flush_scripts: bool,
    notifications_done: bool,
}

struct ObjectEvent {
    target: Option<fes::ObjectReference>,
    event: u32,
    handled: bool,
}

struct MotionCall {
    name: Vec<u8>,
    bank: i64,
    end: i64,
    prepared: Option<(u32, Vec<f32>)>,
}

struct MotionStop {
    target: fes::ObjectReference,
    bank: i64,
    end: i64,
}

enum Notification {
    Exit(i32),
    PreferredFocus(fes::ObjectReference),
    FocusLock(i32),
    Script(ScriptRequest),
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
            frame_wait: 0,
            wait_target: None,
            exit_code: None,
            exit_request: None,
            notifications: VecDeque::new(),
            scripts: ChildScripts::default(),
            preferred_focus: None,
            focus_lock: 0,
            invocation: None,
            input: super::input::SystemInput::default(),
            system_block: None,
            object_event: None,
            deferred_decodes: 0,
            keyblocks: Vec::new(),
            media_preparation: None,
            media_prepared: 0,
            motion_names: BTreeMap::new(),
            motion_call: None,
            motion_stop: None,
        })
    }

    pub fn interpreter(&self) -> &fes::Interpreter {
        &self.interpreter
    }

    pub(super) fn parameter_values(&self) -> &[i32] {
        self.interpreter.local_values()
    }

    pub(super) fn parameter_values_mut(&mut self) -> &mut [i32] {
        self.interpreter.local_values_mut()
    }

    /// 当前由 execfes 启动的直属子任务；完成通知不自动清除此编号。
    pub fn child_fes(&self) -> Option<u32> {
        self.scripts.fes
    }

    pub fn objects(&self) -> &Objects {
        &self.objects
    }

    pub(super) fn notify_slider_position(
        &mut self,
        target: &fes::ObjectReference,
        value: i32,
    ) -> Result<bool> {
        let Some(handle) = self.objects.lookup(&target.name, target.bank) else {
            return Ok(false);
        };
        let state = self
            .objects
            .get_mut(handle)
            .unwrap()
            .state
            .control_mut()
            .ok_or_else(|| Error::new(0, "FES 滑块通知的名称绑定类型不匹配"))?;
        state.set_position(value, 0);
        Ok(true)
    }

    /// 装载时解码的第一段 KEYBLOCK；后续同名声明不覆盖此表。
    pub fn keyblocks(&self) -> &[fes::KeyBlock] {
        &self.keyblocks
    }

    pub(super) fn navigate_button(
        &mut self,
        source: &fes::ObjectReference,
        direction: usize,
    ) -> Result<super::NavigationOutcome> {
        self.objects
            .navigate_button_with_controls(&self.keyblocks, source, direction)
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

    pub fn forced_key_skip(&self) -> bool {
        self.input.forced
    }

    pub fn raw_input(&self) -> u32 {
        self.input.raw
    }

    pub fn preferred_focus(&self) -> Option<&fes::ObjectReference> {
        self.preferred_focus.as_ref()
    }

    pub fn focus_lock(&self) -> i32 {
        self.focus_lock
    }

    pub(super) fn set_raw_input(&mut self, value: u32) {
        self.input.raw = value;
    }

    pub(super) fn record_input_event(&mut self, event: u32) {
        self.record_object_input(event, None);
    }

    fn record_object_input(&mut self, event: u32, object: Option<u32>) {
        if let Some((name, value)) = self.input.record(event) {
            let value = if value == -1 {
                object.map_or(-1, |handle| handle as i32)
            } else {
                value
            };
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

    fn publish_notifications(&mut self) {
        if self.scripts.active.is_some()
            || matches!(self.notifications.front(), Some(Notification::Script(_)))
        {
            return;
        }
        while let Some(notification) = self.notifications.pop_front() {
            match notification {
                Notification::Exit(code) => self.publish_exit(code),
                Notification::PreferredFocus(target) => self.preferred_focus = Some(target),
                Notification::FocusLock(value) => self.focus_lock = value,
                Notification::Script(request) => {
                    self.notifications.push_front(Notification::Script(request));
                    break;
                }
            }
        }
    }

    fn notify(&mut self, notification: Notification) -> Result<()> {
        if self.notifications.len() >= crate::read::MAX_ITEMS {
            return Err(Error::new(0, "FES 任务通知队列超过上限"));
        }
        self.notifications.push_back(notification);
        Ok(())
    }

    /// 完成值区分普通对象命令与会结束本次控制器执行的全局命令。
    fn control_command(
        &mut self,
        args: &[Vec<u8>],
        offset: usize,
        numbers: &mut crate::expression::State,
    ) -> Result<Option<bool>> {
        if self.key_skip_command(args, numbers)? {
            return Ok(Some(false));
        }
        if args.first().is_some_and(|name| name == b"lockactive") {
            let value = args
                .get(1)
                .map_or(Ok(0), |bytes| self.interpreter.eval(bytes, numbers))?;
            self.notify(Notification::FocusLock(value))?;
            return Ok(Some(true));
        }
        if !args.get(1).is_some_and(|name| name == b"noact") {
            return Ok(None);
        }
        let mut target = if args[0] == b"@" {
            let context = self
                .interpreter
                .event_context()
                .ok_or_else(|| Error::new(offset, "FES 当前没有事件对象"))?;
            fes::ObjectReference {
                name: context.object.clone(),
                bank: context.bank,
            }
        } else {
            self.interpreter.object_reference(&args[0], numbers)?
        };
        let count = self.objects.count(&target.name);
        if count == 0 {
            return Err(Error::new(
                offset,
                "FES 默认焦点目标不存在，表达式回退尚未接入",
            ));
        }
        if target.bank == -1 {
            if count != 1 {
                return Ok(Some(false));
            }
            target.bank = 0;
        }
        if let Some(bytes) = args.get(2) {
            self.interpreter.eval(bytes, numbers)?;
        }
        if self
            .objects
            .lookup(&target.name, target.bank)
            .and_then(|handle| self.objects.get(handle))
            .is_some_and(|object| matches!(object.state, super::ObjectState::Button(_)))
        {
            self.notify(Notification::PreferredFocus(target))?;
        }
        Ok(Some(false))
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
            || self.system_block.is_some()
            || self.object_event.is_some()
            || self.media_preparation.is_some()
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
                Step::Event(event) if self.apply_initialization(&event, numbers)? => {
                    self.interpreter.resume();
                }
                _ => return Err(Error::new(0, "FES 装载期出现执行事件")),
            }
        }
        Ok(!self.interpreter.initialization_pending())
    }

    fn apply_initialization(
        &mut self,
        event: &fes::Event,
        numbers: &mut crate::expression::State,
    ) -> Result<bool> {
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
                if let Some(declaration) = declarations.iter().find(|declaration| {
                    !matches!(
                        declaration.header.split(u8::is_ascii_whitespace).next(),
                        Some(b"SUBCOMMAND" | b"KEYBLOCK")
                    )
                }) {
                    return Err(Error::new(
                        declaration.lines.first().map_or(0, |line| line.offset),
                        "FES 声明需要对应的运行时服务",
                    ));
                }
                if declarations.iter().any(|declaration| {
                    declaration.header.split(u8::is_ascii_whitespace).next() == Some(b"KEYBLOCK")
                }) {
                    self.keyblocks = self.interpreter.keyblocks(numbers)?;
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
        invocation.media_passes = 1 + std::mem::take(&mut self.deferred_decodes);
        invocation.notifications_done = false;
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

    /// 分派输入事件；None 表示预算耗尽，续跑必须保持目标和事件不变。
    /// 完成值表示对象自身是否具有处理器；默认事件组单独执行时为 false。
    pub fn dispatch_fes_event(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        target: Option<&fes::ObjectReference>,
        event: u32,
    ) -> Result<Option<bool>> {
        self.store().dispatch_fes_event(owner, scene, target, event)
    }

    /// 推进脚本并在本轮边界更新 FES 对象；false 表示预算耗尽，续跑保留原时刻。
    pub fn update_fes_scene(&mut self, owner: u32, scene: &mut Scene, now: u32) -> Result<bool> {
        self.store().update_fes_scene(owner, scene, now)
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(in crate::runtime) fn refresh_fes_media(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        cursor: &mut usize,
    ) -> Result<bool> {
        if scene.is_pending() {
            return Err(Error::new(0, "FES 事件尚未完成，不能刷新媒体"));
        }
        for _ in 0..self.budget.max(1) {
            if *cursor == scene.objects.update_order.len() {
                return Ok(true);
            }
            self.update_fes_scene_record(owner, scene, *cursor)?;
            *cursor += 1;
        }
        Ok(*cursor == scene.objects.update_order.len())
    }

    pub(in crate::runtime) fn prepare_fes_scene(
        &mut self,
        owner: u32,
        scene: &mut Scene,
    ) -> Result<bool> {
        if scene.invocation.is_some()
            || scene.system_block.is_some()
            || scene.object_event.is_some()
        {
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
        for index in 0..scene.objects.update_order.len() {
            self.update_fes_scene_record(owner, scene, index)?;
        }
        for record in &scene.objects.update_order {
            let Some(handle) = scene
                .objects
                .lookup(&record.target.name, record.target.bank)
            else {
                continue;
            };
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
            if let Some(slider) = object.state.control() {
                let protocol = super::slider::ControlKind::of(&object.state).unwrap();
                if record.task_id.is_some() {
                    self.fes_command(
                        owner,
                        object.task_id,
                        protocol.command(367),
                        [0, if sleeping { 0 } else { slider.enabled as u32 }, 1],
                        None,
                    )?;
                    self.fes_command(
                        owner,
                        object.task_id,
                        protocol.command(366),
                        [0, if sleeping { 0 } else { slider.display as u32 }, 1],
                        None,
                    )?;
                }
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
        if scene.invocation.is_some()
            || scene.media_preparation.is_some()
            || scene.object_event.is_some()
        {
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
        if !self.run_fes_handler(scene)? {
            return Ok(false);
        }
        scene.system_block = None;
        Ok(true)
    }

    pub(in crate::runtime) fn dispatch_fes_event(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        target: Option<&fes::ObjectReference>,
        event: u32,
    ) -> Result<Option<bool>> {
        self.dispatch_fes_event_inner(owner, scene, target, event, false)
    }

    pub(in crate::runtime) fn dispatch_fes_control_event(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        target: &fes::ObjectReference,
        event: u32,
    ) -> Result<Option<bool>> {
        self.dispatch_fes_event_inner(owner, scene, Some(target), event, true)
    }

    fn dispatch_fes_event_inner(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        target: Option<&fes::ObjectReference>,
        event: u32,
        host_boundary: bool,
    ) -> Result<Option<bool>> {
        if event > 21 {
            return Err(Error::new(0, "FES 输入事件编号无效"));
        }
        if self.tree.node(owner).is_none() || scene.owner.is_some_and(|bound| bound != owner) {
            return Err(Error::new(0, "FES 事件所属任务无效"));
        }
        if (scene.invocation.is_some() && !host_boundary)
            || scene.system_block.is_some()
            || scene.media_preparation.is_some()
            || scene.interpreter.initialization_pending()
        {
            return Err(Error::new(0, "FES 本轮推进尚未完成"));
        }
        if let Some(pending) = &scene.object_event {
            if pending.target.as_ref() != target || pending.event != event {
                return Err(Error::new(0, "FES 上一次对象事件尚未完成"));
            }
        } else {
            scene.owner = Some(owner);
            let handle = match target {
                Some(target) => match scene.objects.lookup(&target.name, target.bank) {
                    Some(handle) => Some(handle),
                    None => return Ok(Some(false)),
                },
                None => None,
            };
            let default =
                handle.and_then(|handle| match &scene.objects.get(handle).unwrap().state {
                    super::ObjectState::Button(button) => Some(button.default_events.clone()),
                    _ => None,
                });
            scene.record_object_input(event, handle);
            let name = match event {
                0 => "FOCUS",
                1 => "UNFOCUS",
                2 => "PUSH_L",
                3 => "PUSH_R",
                4 => "RELEASE_L",
                5 => "RELEASE_R",
                8 => "SCRL_P",
                21 => "TOUCH",
                _ => return Ok(Some(false)),
            };
            let Some(target) = target else {
                return Ok(Some(false));
            };
            let default = default.as_deref().map(crate::text::ascii).transpose()?;
            let dispatched = if host_boundary {
                scene.interpreter.dispatch_event_at_host_boundary(
                    &target.name,
                    target.bank,
                    name,
                    default,
                )?
            } else {
                scene
                    .interpreter
                    .dispatch_event(&target.name, target.bank, name, default)?
            };
            if !dispatched {
                return Ok(Some(false));
            }
            scene.object_event = Some(ObjectEvent {
                target: Some(target.clone()),
                event,
                handled: scene.interpreter.object_handler_exists(),
            });
        }
        if !self.run_fes_handler(scene)? {
            return Ok(None);
        }
        Ok(Some(scene.object_event.take().unwrap().handled))
    }

    fn run_fes_handler(&mut self, scene: &mut Scene) -> Result<bool> {
        for _ in 0..self.budget.max(1) {
            match scene.interpreter.run(self.numbers, 1)? {
                Step::Yield => (),
                Step::Event(fes::Event::Wait(_)) => scene.interpreter.finish_event_handler()?,
                Step::Event(fes::Event::Command(line)) => {
                    let args = scene.interpreter.parameters(&line.bytes, self.numbers)?;
                    if let Some(disposition) =
                        scene.script_command(&args, line.offset, self.numbers)?
                    {
                        if matches!(disposition, ScriptDisposition::Continue) {
                            scene.interpreter.resume();
                        } else {
                            scene.interpreter.finish_event_handler()?;
                        }
                    } else if self.fes_font_command(&args, line.offset)? {
                        scene.interpreter.resume();
                    } else if args.first().is_some_and(|name| name == b"decode") {
                        if scene.deferred_decodes == crate::read::MAX_ITEMS {
                            return Err(Error::new(line.offset, "FES 提交请求数量超过上限"));
                        }
                        scene.deferred_decodes += 1;
                        scene.interpreter.finish_event_handler()?;
                    } else if scene
                        .wait_command(&args, line.offset, 0, self.numbers)?
                        .is_some()
                    {
                        scene.interpreter.finish_event_handler()?;
                    } else if let Some(stop) =
                        scene.control_command(&args, line.offset, self.numbers)?
                    {
                        if stop {
                            scene.interpreter.finish_event_handler()?;
                        } else {
                            scene.interpreter.resume();
                        }
                    } else if let Some(complete) = self.run_fes_stop(scene, &args, line.offset)? {
                        if !complete {
                            return Ok(false);
                        }
                        scene.interpreter.resume();
                    } else {
                        if !scene.objects.dispatch_image_animation(
                            &mut scene.interpreter,
                            &line,
                            self.numbers,
                        )? && !scene.objects.dispatch_object_command(
                            &mut scene.interpreter,
                            &line,
                            self.numbers,
                        )? {
                            return Err(Error::new(line.offset, "FES 事件对象命令尚未接入"));
                        }
                        scene.interpreter.resume();
                    }
                }
                Step::Event(fes::Event::InitializeObject(initialization)) => {
                    scene
                        .objects
                        .initialize(&initialization, |name| Ok(name.to_vec()))?;
                    scene.interpreter.resume();
                }
                Step::Event(fes::Event::Exit(code)) => {
                    scene.notify(Notification::Exit(code))?;
                    scene.interpreter.finish_event_handler()?;
                }
                Step::Event(fes::Event::Subcommand(call)) => {
                    if !self.run_fes_subcommand(scene, &call)? {
                        return Ok(false);
                    }
                    scene.interpreter.resume();
                }
                _ => return Err(Error::new(0, "FES 事件出现未支持的执行状态")),
            }
            if let Some(transferred) = scene.interpreter.take_dispatch_result() {
                if transferred {
                    scene.deadline = None;
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn run_fes_subcommand(
        &mut self,
        scene: &mut Scene,
        call: &fes::SubcommandCall,
    ) -> Result<bool> {
        let result = (|| {
            if scene.motion_call.is_none() {
                if !scene.motion_names.contains_key(&call.binding.name) {
                    let services = self
                        .resources
                        .services
                        .as_deref_mut()
                        .ok_or_else(|| Error::new(0, "运行时未配置 FES 运动资源服务"))?;
                    let name = services.resolve(b"mot", &call.binding.target)?;
                    scene.motion_names.insert(call.binding.name.clone(), name);
                }
                let (bank, end) = if call.target.bank == -1 {
                    (0, i64::from(scene.objects.count(&call.target.name)))
                } else {
                    (i64::from(call.target.bank), i64::from(call.target.bank) + 1)
                };
                scene.motion_call = Some(MotionCall {
                    name: scene.motion_names[&call.binding.name].clone(),
                    bank,
                    end,
                    prepared: None,
                });
            }
            for _ in 0..self.budget.max(1) {
                let pending = scene.motion_call.as_ref().unwrap();
                if pending.bank >= pending.end {
                    scene.motion_call = None;
                    return Ok(true);
                }
                if pending.prepared.is_none() {
                    let Some(handle) = scene.objects.lookup(&call.target.name, pending.bank as i32)
                    else {
                        scene.motion_call = None;
                        return Ok(true);
                    };
                    let object = scene.objects.get(handle).unwrap();
                    let context = match &object.state {
                        super::ObjectState::Base
                        | super::ObjectState::Slider(_)
                        | super::ObjectState::Scroll2(_) => fes::SubcommandContext::default(),
                        state => state
                            .image()
                            .ok_or_else(|| Error::new(0, "FES 此对象类型的运动属性尚未接入"))?
                            .subcommand_context(),
                    };
                    let parameters = scene.interpreter.subcommand_arguments(
                        &call.binding,
                        &call.arguments,
                        &context,
                        self.numbers,
                    )?;
                    scene.objects.capture_motion_base(handle)?;
                    scene.motion_call.as_mut().unwrap().prepared = Some((handle, parameters));
                }
                let pending = scene.motion_call.as_ref().unwrap();
                if let Some(program) = self.resources.load_motion(&pending.name)? {
                    let (handle, parameters) = pending.prepared.as_ref().unwrap();
                    scene.objects.insert_motion(
                        *handle,
                        program,
                        crate::kcs::MotionState::with_parameters(parameters)?,
                        scene.objects.get(*handle).unwrap().motion_delay(),
                    )?;
                }
                let pending = scene.motion_call.as_mut().unwrap();
                pending.bank += 1;
                pending.prepared = None;
                if pending.bank == pending.end {
                    scene.motion_call = None;
                    return Ok(true);
                }
            }
            Ok(false)
        })();
        result.map_err(|error: Error| Error::new(call.offset + error.offset, error.message))
    }

    fn run_fes_stop(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<Option<bool>> {
        if args.get(1).is_none_or(|name| name != b"stop") {
            return Ok(None);
        }
        if scene.motion_stop.is_none() {
            let target = if args[0] == b"@" {
                let context = scene
                    .interpreter
                    .event_context()
                    .ok_or_else(|| Error::new(offset, "FES 当前没有事件对象"))?;
                fes::ObjectReference {
                    name: context.object.clone(),
                    bank: context.bank,
                }
            } else {
                scene.interpreter.object_reference(&args[0], self.numbers)?
            };
            if scene
                .objects
                .lookup(
                    &target.name,
                    if target.bank == -1 { 0 } else { target.bank },
                )
                .is_none()
            {
                return Err(Error::new(
                    offset,
                    "FES 对象目标不存在，整行表达式回退尚未接入",
                ));
            }
            let (bank, end) = if target.bank == -1 {
                (0, i64::from(scene.objects.count(&target.name)))
            } else {
                (i64::from(target.bank), i64::from(target.bank) + 1)
            };
            scene.motion_stop = Some(MotionStop { target, bank, end });
        }
        for _ in 0..self.budget.max(1) {
            let pending = scene.motion_stop.as_ref().unwrap();
            if pending.bank == pending.end {
                scene.motion_stop = None;
                return Ok(Some(true));
            }
            if let Some(handle) = scene
                .objects
                .lookup(&pending.target.name, pending.bank as i32)
            {
                if !self.advance_fes_motions(&mut scene.objects, 0, Some(handle))? {
                    return Ok(Some(false));
                }
            }
            scene.motion_stop.as_mut().unwrap().bank += 1;
        }
        Ok(Some(false))
    }

    fn advance_fes_motions(
        &mut self,
        objects: &mut Objects,
        now: u32,
        stop: Option<u32>,
    ) -> Result<bool> {
        std::mem::swap(
            &mut self.host.random,
            &mut self.resources.motion_host.random,
        );
        let result = if let Some(handle) = stop {
            objects.stop_motions(handle, &mut self.resources.motion_host, self.budget)
        } else {
            objects.update_motions(now, &mut self.resources.motion_host, self.budget)
        };
        std::mem::swap(
            &mut self.host.random,
            &mut self.resources.motion_host.random,
        );
        self.host
            .debug_messages
            .append(&mut self.resources.motion_host.debug_messages);
        result
    }

    pub(in crate::runtime) fn update_fes_scene(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        now: u32,
    ) -> Result<bool> {
        if scene.system_block.is_some() || scene.object_event.is_some() {
            return Err(Error::new(0, "FES 事件尚未完成"));
        }
        if self.tree.node(owner).is_none() {
            return Err(Error::new(0, "FES 所属任务不存在"));
        }
        if scene.owner.is_some_and(|bound| bound != owner) {
            return Err(Error::new(0, "FES 执行状态已绑定其他所属任务"));
        }
        scene.owner = Some(owner);
        if scene.invocation.is_none() {
            if scene.frame_origin == 0 {
                scene.frame_origin = now.wrapping_sub(1);
            }
            scene
                .interpreter
                .named
                .entry("_FRAME_".to_owned())
                .or_default()
                .insert(0, now.wrapping_sub(scene.frame_origin) as i32);
            let waiting = scene.waiting_at_start(now)?;
            scene.invocation = Some(Invocation {
                now,
                updates: None,
                resume_script: false,
                motions_done: false,
                media_passes: 0,
                flush_scripts: false,
                notifications_done: false,
            });
            if waiting {
                scene.publish_notifications();
                scene.finish_input();
                scene.begin_updates(false);
            } else {
                scene.deadline = None;
            }
        }
        for _ in 0..self.budget {
            if scene.invocation.as_ref().unwrap().flush_scripts {
                if !self.process_fes_script_requests(owner, scene)? {
                    return Ok(false);
                }
                scene.invocation.as_mut().unwrap().flush_scripts = false;
            }
            if !scene.interpreter.initialization_pending()
                && scene.media_prepared != scene.objects.update_order.len()
                && !self.prepare_fes_media(owner, scene)?
            {
                return Ok(false);
            }
            let invocation = scene.invocation.as_ref().unwrap();
            if invocation.updates.is_some() && !invocation.resume_script && !invocation.motions_done
            {
                if !self.advance_fes_motions(&mut scene.objects, invocation.now, None)? {
                    return Ok(false);
                }
                scene.invocation.as_mut().unwrap().motions_done = true;
            }
            if let Some((count, cursor)) = scene.invocation.as_ref().unwrap().updates {
                if !scene.invocation.as_ref().unwrap().notifications_done {
                    if !self.process_fes_script_requests(owner, scene)? {
                        return Ok(false);
                    }
                    if !scene.invocation.as_ref().unwrap().resume_script {
                        self.poll_fes_child_status(owner, scene)?;
                    }
                    scene.invocation.as_mut().unwrap().notifications_done = true;
                }
                if cursor < count {
                    self.update_fes_scene_record(owner, scene, cursor)?;
                    scene
                        .invocation
                        .as_mut()
                        .unwrap()
                        .updates
                        .as_mut()
                        .unwrap()
                        .1 += 1;
                } else {
                    let invocation = scene.invocation.as_mut().unwrap();
                    if invocation.media_passes > 1 {
                        invocation.media_passes -= 1;
                        invocation.updates.as_mut().unwrap().1 = 0;
                        continue;
                    }
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
                    let now = scene.invocation.as_ref().unwrap().now;
                    if scene.apply_initialization(&event, self.numbers)? {
                        scene.interpreter.resume();
                        continue;
                    }
                    match event {
                        fes::Event::Command(line) => {
                            let args = scene.interpreter.parameters(&line.bytes, self.numbers)?;
                            if let Some(disposition) =
                                scene.script_command(&args, line.offset, self.numbers)?
                            {
                                scene.interpreter.resume();
                                match disposition {
                                    ScriptDisposition::Continue => (),
                                    ScriptDisposition::Flush => {
                                        scene.invocation.as_mut().unwrap().flush_scripts = true
                                    }
                                    ScriptDisposition::NextTick => {
                                        scene.deadline = Some(now.wrapping_add(1));
                                        scene.finish_input();
                                        scene.begin_updates(false);
                                    }
                                }
                                continue;
                            }
                            if self.fes_font_command(&args, line.offset)? {
                                scene.interpreter.resume();
                                continue;
                            }
                            if args.first().is_some_and(|name| name == b"decode") {
                                scene.publish_notifications();
                                scene.begin_updates(true);
                                continue;
                            }
                            if let Some(deadline) =
                                scene.wait_command(&args, line.offset, now, self.numbers)?
                            {
                                scene.deadline = Some(deadline);
                                scene.publish_notifications();
                                scene.finish_input();
                                scene.begin_updates(false);
                                scene.interpreter.resume();
                                continue;
                            }
                            if let Some(stop) =
                                scene.control_command(&args, line.offset, self.numbers)?
                            {
                                if stop {
                                    scene.publish_notifications();
                                }
                            } else if let Some(complete) =
                                self.run_fes_stop(scene, &args, line.offset)?
                            {
                                if !complete {
                                    return Ok(false);
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
                                return Err(Error::new(line.offset, "FES 对象命令尚未接入"));
                            }
                        }
                        fes::Event::Subcommand(call) => {
                            if !self.run_fes_subcommand(scene, &call)? {
                                return Ok(false);
                            }
                        }
                        fes::Event::Wait(duration) => {
                            scene.publish_notifications();
                            scene.deadline = Some(now.wrapping_add(duration.max(1) as u32));
                            scene.finish_input();
                            scene.begin_updates(false);
                        }
                        fes::Event::Frame => {
                            scene.publish_notifications();
                            scene.deadline = Some(now.wrapping_add(1));
                            scene.finish_input();
                            scene.begin_updates(false);
                        }
                        fes::Event::Exit(code) => {
                            scene.notify(Notification::Exit(code))?;
                            scene.invocation.as_mut().unwrap().flush_scripts = true;
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
