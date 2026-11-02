use super::{NavigationOutcome, Scene};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, ObjectFactory, ObjectHost, TaskLifecycle, TaskObject, kind};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, fes};

/// FES 任务所需的共享表达式状态、媒体更新及启动时的子树调度。
pub trait Services {
    /// 只推进装载期事件；false 表示保留游标等待续跑。
    fn prepare(&mut self, scene: &mut Scene, budget: usize) -> Result<bool>;
    /// 推进当前任务的场景；false 后沿用同一时刻继续本轮。
    fn update(&mut self, scene: &mut Scene, now: u32, budget: usize) -> Result<bool>;
    /// 以父任务 elapsed 更新时钟和子任务，不执行当前任务主体。
    fn start(&mut self, clock: &mut TaskClock) -> Result<()>;
    /// 父任务命令 25 对应的原始正文；没有正文时返回空值。
    fn inherited_context(&self) -> Result<Vec<u8>>;
    /// 辅助按钮优先级；直接父级为根时返回一。
    fn button_priority(&self) -> Result<u32>;
    /// 分派睡眠／唤醒系统块；false 后用相同名称完成预算续跑。
    fn dispatch_system_block(
        &mut self,
        scene: &mut Scene,
        name: &str,
        budget: usize,
    ) -> Result<bool>;
    /// 刷新当前媒体属性后隐藏或恢复显示，唤醒时强制提交共享列表。
    fn set_media_sleep(&mut self, scene: &mut Scene, sleeping: bool) -> Result<()>;
    /// 推进输入事件；None 表示预算耗尽，完成值表示对象自身处理器存在。
    fn dispatch_event(
        &mut self,
        scene: &mut Scene,
        target: Option<&fes::ObjectReference>,
        event: u32,
        budget: usize,
    ) -> Result<Option<bool>>;
    /// 按声明顺序刷新媒体，成功后推进 cursor；失败保留当前位置。
    fn refresh_media(
        &mut self,
        scene: &mut Scene,
        cursor: &mut usize,
        budget: usize,
    ) -> Result<bool>;
    /// 网格选择的默认实现；关联控件可返回 Handled 消费方向输入。
    fn navigate_button(
        &mut self,
        scene: &mut Scene,
        source: &fes::ObjectReference,
        direction: usize,
    ) -> Result<NavigationOutcome> {
        Ok(scene
            .objects()
            .navigate_button(scene.keyblocks(), source, direction)?
            .map_or(NavigationOutcome::Unhandled, NavigationOutcome::Target))
    }
}

impl<F: ObjectFactory> Services for ObjectHost<'_, F> {
    fn prepare(&mut self, scene: &mut Scene, budget: usize) -> Result<bool> {
        let owner = self.host.task_id;
        let mut store = self.store();
        store.budget = budget.max(1);
        store.prepare_fes_scene(owner, scene)
    }

    fn update(&mut self, scene: &mut Scene, now: u32, budget: usize) -> Result<bool> {
        let owner = self.host.task_id;
        let mut store = self.store();
        store.budget = budget.max(1);
        store.update_fes_scene(owner, scene, now)
    }

    fn dispatch_system_block(
        &mut self,
        scene: &mut Scene,
        name: &str,
        budget: usize,
    ) -> Result<bool> {
        let owner = self.host.task_id;
        let mut store = self.store();
        store.budget = budget.max(1);
        store.dispatch_fes_system_block(owner, scene, name)
    }

    fn set_media_sleep(&mut self, scene: &mut Scene, sleeping: bool) -> Result<()> {
        let owner = self.host.task_id;
        self.store().set_fes_media_sleep(owner, scene, sleeping)
    }

    fn dispatch_event(
        &mut self,
        scene: &mut Scene,
        target: Option<&fes::ObjectReference>,
        event: u32,
        budget: usize,
    ) -> Result<Option<bool>> {
        let owner = self.host.task_id;
        let mut store = self.store();
        store.budget = budget.max(1);
        store.dispatch_fes_event(owner, scene, target, event)
    }

    fn refresh_media(
        &mut self,
        scene: &mut Scene,
        cursor: &mut usize,
        budget: usize,
    ) -> Result<bool> {
        let owner = self.host.task_id;
        let mut store = self.store();
        store.budget = budget.max(1);
        store.refresh_fes_media(owner, scene, cursor)
    }

    fn start(&mut self, clock: &mut TaskClock) -> Result<()> {
        let owner = self.host.task_id;
        let parent = self.tree.node(owner).and_then(|node| node.parent);
        let now = match parent {
            Some(parent) => self
                .objects
                .get(&parent)
                .map(|object| object.clock().elapsed())
                .or_else(|| self.running.get(&parent).map(|task| task.clock.elapsed()))
                .ok_or_else(|| Error::new(0, "FES 父任务时钟不可用"))?,
            None => 0,
        };
        clock.update(now);
        self.set_task_clock(clock);
        let mut lifecycle = TaskLifecycle {
            numbers: self.numbers,
            host: self.host,
            resources: self.resources,
            objects: self.objects,
            running: self.running,
            factory: self.factory,
            budget: self.budget,
            limit: self.limit,
        };
        self.tree.update_children(owner, now, 0, &mut lifecycle)
    }

    fn inherited_context(&self) -> Result<Vec<u8>> {
        let Some(parent) = self
            .tree
            .node(self.host.task_id)
            .and_then(|node| node.parent)
        else {
            return Ok(Vec::new());
        };
        if let Some(object) = self.objects.get(&parent) {
            return Ok(if object.control().deletion_requested != 0 {
                Vec::new()
            } else {
                object.creation_context().unwrap_or_default().to_vec()
            });
        }
        self.running
            .get(&parent)
            .map(|task| {
                if task.deleted {
                    Vec::new()
                } else {
                    task.context.clone().unwrap_or_default()
                }
            })
            .ok_or_else(|| Error::new(0, "FES 父任务上下文不可用"))
    }

    fn button_priority(&self) -> Result<u32> {
        let mut current = self
            .tree
            .node(self.host.task_id)
            .and_then(|node| node.parent);
        let mut depth = 1u32;
        while let Some(id) = current {
            let node = self
                .tree
                .node(id)
                .ok_or_else(|| Error::new(0, "FES 祖先任务不存在"))?;
            if node.kind == kind::ROOT {
                return Ok(depth);
            }
            depth = depth
                .checked_add(1)
                .ok_or_else(|| Error::new(0, "FES 祖先层数超过上限"))?;
            current = node.parent;
        }
        Err(Error::new(0, "FES 辅助按钮需要类型为根的祖先任务"))
    }
}

/// 类型 34 的 FES 任务；构造后通过命令 337、338、340、339 显式初始化并启动。
pub struct FesObject {
    id: u32,
    base: BaseObject,
    initialized: bool,
    started: bool,
    scene: Option<Scene>,
    pending: Option<Command>,
    name: Vec<u8>,
    context: Vec<u8>,
    input: u32,
    sound: u32,
    list: Option<u32>,
    clip: crate::graphics::Rect,
    offset: [i32; 2],
    first_update: bool,
    input_button: Option<u32>,
    input_mode: u32,
    last_input: u32,
    paused: u32,
    pause_script_done: bool,
    focus: Option<fes::ObjectReference>,
    pressed: Option<fes::ObjectReference>,
    input_event: Option<InputEvent>,
}

struct InputEvent {
    target: Option<fes::ObjectReference>,
    event: Option<u32>,
    handled: Option<bool>,
    response_done: bool,
    clear_selection: bool,
    refresh: bool,
    cursor: usize,
    preserve_output: bool,
    move_pointer: bool,
    response: InputResponse,
    system_key: Option<SystemKeyInput>,
}

struct SystemKeyInput {
    key: u32,
    cursor: usize,
    target: Option<fes::ObjectReference>,
    handled: bool,
}

#[derive(Clone, Copy)]
enum InputResponse {
    IfUnhandled,
    Always,
    None,
}

impl FesObject {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            base: BaseObject::new(),
            initialized: false,
            started: false,
            scene: None,
            pending: None,
            name: Vec::new(),
            context: Vec::new(),
            input: 0,
            sound: 0,
            list: None,
            clip: crate::graphics::Rect::default(),
            offset: [0; 2],
            first_update: true,
            input_button: None,
            input_mode: 0,
            last_input: 0,
            paused: 0,
            pause_script_done: false,
            focus: None,
            pressed: None,
            input_event: None,
        }
    }

    pub fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }

    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn input_task(&self) -> u32 {
        self.input
    }

    pub fn sound_task(&self) -> u32 {
        self.sound
    }

    pub fn input_button(&self) -> Option<u32> {
        self.input_button
    }

    pub fn last_input(&self) -> u32 {
        self.last_input
    }

    pub fn paused(&self) -> u32 {
        self.paused
    }

    fn pointer_position(&self, host: &mut dyn Host, propagation: u32) -> Result<[i32; 2]> {
        let mut x = [0; 4];
        let mut y = [0; 4];
        self.send(
            host,
            self.input,
            126,
            [0, 0, propagation],
            &mut CommandBuffers {
                data0: Some(&mut x),
                data1: Some(&mut y),
            },
        )?;
        Ok([i32::from_le_bytes(x), i32::from_le_bytes(y)])
    }

    fn update_pointer_values(&mut self, host: &mut dyn Host) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }
        let outside = self.send(
            host,
            self.input,
            153,
            [0, 0, 1],
            &mut CommandBuffers::default(),
        )?;
        let point = if outside == 0 {
            self.pointer_position(host, 1)?
        } else {
            [i32::MIN; 2]
        };
        if let Some(scene) = &mut self.scene {
            scene.set_system_value("_MOUSE_X_", point[0]);
            scene.set_system_value("_MOUSE_Y_", point[1]);
        }
        Ok(())
    }

    fn input_event(
        &mut self,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
        budget: usize,
    ) -> Result<bool> {
        if self.input_event.is_none() {
            if !self.begin_input_event(command, output, host)? {
                return Ok(true);
            }
            self.pending = Some(*command);
        }
        if matches!(command.code, 9 | 10) {
            *output = 0;
        }
        if self.input_event.as_ref().unwrap().system_key.is_some()
            && self.input_event.as_ref().unwrap().handled.is_none()
            && !self.system_key_events(host, budget)?
        {
            return Ok(false);
        }
        if self.input_event.as_ref().unwrap().handled.is_none() {
            let event = self.input_event.as_ref().unwrap();
            let handled = if let Some(code) = event.event {
                let target = event.target.clone();
                let Some(handled) = services(host)?.dispatch_event(
                    self.scene.as_mut().unwrap(),
                    target.as_ref(),
                    code,
                    budget,
                )?
                else {
                    return Ok(false);
                };
                handled
            } else {
                false
            };
            self.input_event.as_mut().unwrap().handled = Some(handled);
            if self.input_event.as_ref().unwrap().clear_selection
                && self
                    .scene
                    .as_ref()
                    .is_none_or(|scene| scene.focus_lock() == 0)
            {
                self.focus = None;
                self.pressed = None;
            }
        }
        if self.input_event.as_ref().unwrap().move_pointer {
            let target = self.input_event.as_ref().unwrap().target.as_ref().unwrap();
            let point = self
                .scene
                .as_ref()
                .unwrap()
                .objects()
                .navigation_pointer(target);
            if let Some([x, y]) = point {
                let packed = (x as u16 as u32) | ((y as u16 as u32) << 16);
                self.send(
                    host,
                    self.input,
                    139,
                    [1, packed, 0],
                    &mut CommandBuffers::default(),
                )?;
            } else {
                self.input_event.as_mut().unwrap().response = InputResponse::Always;
            }
            self.input_event.as_mut().unwrap().move_pointer = false;
        }
        let event = self.input_event.as_ref().unwrap();
        if !event.response_done {
            let handled = event.handled.unwrap();
            if command.code == 10 && handled && command.data[1] < 2 {
                self.send(
                    host,
                    self.input,
                    146,
                    [0; 3],
                    &mut CommandBuffers::default(),
                )?;
            } else if match event.response {
                InputResponse::IfUnhandled => !handled,
                InputResponse::Always => true,
                InputResponse::None => false,
            } {
                self.send(
                    host,
                    command.sender,
                    95,
                    [0, 1, 1],
                    &mut CommandBuffers::default(),
                )?;
            }
            self.input_event.as_mut().unwrap().response_done = true;
        }
        if self.input_event.as_ref().unwrap().refresh {
            if let Some(scene) = &mut self.scene {
                let cursor = &mut self.input_event.as_mut().unwrap().cursor;
                if !services(host)?.refresh_media(scene, cursor, budget)? {
                    return Ok(false);
                }
            }
        }
        if !self.input_event.as_ref().unwrap().preserve_output {
            *output = 0;
        }
        self.input_event = None;
        self.pending = None;
        self.effects(host);
        Ok(true)
    }

    fn begin_input_event(
        &mut self,
        command: &Command,
        output: &mut u32,
        host: &mut dyn Host,
    ) -> Result<bool> {
        if self.scene.is_none() && matches!(command.code, 9 | 10) {
            return Err(Error::new(0, "FES 按下与释放事件需要已装载脚本"));
        }
        if command.code == 9 {
            let raw = self.send(
                host,
                command.sender,
                96,
                [0, 1, 1],
                &mut CommandBuffers::default(),
            )?;
            self.last_input = raw;
            self.scene.as_mut().unwrap().set_raw_input(raw);
        } else if command.code == 10 {
            self.last_input = 0;
        }
        if self.paused != 0 {
            if matches!(command.code, 9 | 10) {
                self.send(
                    host,
                    command.sender,
                    95,
                    [0, 1, 1],
                    &mut CommandBuffers::default(),
                )?;
            } else {
                *output = 0;
            }
            return Ok(false);
        }
        self.update_pointer_values(host)?;
        let find = || {
            self.scene
                .as_ref()?
                .objects()
                .media_records()
                .iter()
                .find(|record| record.task_id() == Some(command.data[0]))
                .map(|record| record.target().clone())
        };
        let key = command.data[1];
        let mut clear_selection = false;
        let mut move_pointer = false;
        let mut response = InputResponse::IfUnhandled;
        let mut system_key = None;
        let target = match command.code {
            64 => {
                let target = find();
                if target.is_some() {
                    self.focus = if key != 0 { target.clone() } else { None };
                }
                clear_selection = target.is_none() || key == 0;
                target
            }
            65 => find(),
            9 => {
                let objects = self.scene.as_ref().unwrap().objects();
                let target = self.focus.clone().filter(|target| {
                    !objects
                        .lookup(&target.name, target.bank)
                        .and_then(|handle| objects.get(handle))
                        .is_some_and(|object| {
                            matches!(&object.state,
                            super::ObjectState::Button(button) if button.enabled == 0)
                        })
                });
                self.pressed = target.clone();
                if (4..=7).contains(&key) {
                    match self.navigate_input(key, host)? {
                        NavigationOutcome::Target(target) => {
                            move_pointer = true;
                            response = InputResponse::None;
                            Some(target)
                        }
                        NavigationOutcome::Handled => {
                            response = InputResponse::None;
                            None
                        }
                        NavigationOutcome::Unhandled => {
                            response = InputResponse::Always;
                            None
                        }
                    }
                } else if !matches!(key, 0 | 1 | 8..=11) {
                    system_key = Some(SystemKeyInput {
                        key,
                        cursor: 0,
                        target: None,
                        handled: false,
                    });
                    None
                } else {
                    target
                }
            }
            10 => self.pressed.clone(),
            _ => unreachable!(),
        };
        let event = match (command.code, key) {
            (9, 0) => Some(2),
            (9, 1) => Some(3),
            (9, 4..=7) if move_pointer => Some(key + 7),
            (9, 8) => Some(6),
            (9, 9) => Some(7),
            (9, 10) => Some(9),
            (9, 11) => Some(10),
            (10, 0) => Some(4),
            (10, 1) => Some(5),
            (10, 4..=7) => Some(key + 11),
            (64, _) if target.is_some() => Some(u32::from(key == 0)),
            (65, _) if target.is_some() => Some(21),
            _ => None,
        };
        let touch = command.code == 65 && target.is_some();
        if command.code == 64 || command.code == 65 && !touch {
            response = InputResponse::None;
        }
        self.input_event = Some(InputEvent {
            target,
            event,
            handled: None,
            response_done: false,
            clear_selection,
            refresh: !touch,
            cursor: 0,
            preserve_output: touch,
            move_pointer,
            response,
            system_key,
        });
        Ok(true)
    }

    fn system_key_events(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        let event = self.input_event.as_mut().unwrap();
        let input = event.system_key.as_mut().unwrap();
        let scene = self.scene.as_mut().unwrap();
        for _ in 0..budget.max(1) {
            if input.target.is_none() {
                let objects = scene.objects();
                let Some(record) = objects.media_records().get(input.cursor) else {
                    event.handled = Some(input.handled);
                    return Ok(true);
                };
                let target = record.target();
                let matched = objects
                    .lookup(&target.name, target.bank)
                    .and_then(|handle| objects.get(handle))
                    .is_some_and(|object| {
                        matches!(&object.state, super::ObjectState::Button(button)
                            if button.enabled != 0 && button.system_key != -1
                                && button.system_key as u32 == input.key)
                    });
                input.target = matched.then(|| target.clone());
                if input.target.is_none() {
                    input.cursor += 1;
                    continue;
                }
            }
            let Some(handled) =
                services(host)?.dispatch_event(scene, input.target.as_ref(), 2, budget)?
            else {
                return Ok(false);
            };
            input.handled |= handled;
            input.target = None;
            input.cursor += 1;
        }
        Ok(false)
    }

    fn navigate_input(&mut self, key: u32, host: &mut dyn Host) -> Result<NavigationOutcome> {
        let objects = self.scene.as_ref().unwrap().objects();
        let current_exists = self
            .focus
            .as_ref()
            .is_some_and(|target| objects.lookup(&target.name, target.bank).is_some());
        if !current_exists {
            let point = self.pointer_position(host, 0)?;
            if let Some(target) = objects.navigation_hit(point, host)? {
                self.focus = Some(target);
            } else {
                return Ok(self.preferred_navigation());
            }
        }
        let target = self.focus.as_ref().unwrap().clone();
        let result = services(host)?.navigate_button(
            self.scene.as_mut().unwrap(),
            &target,
            (key - 4) as usize,
        )?;
        match result {
            NavigationOutcome::Target(target) => {
                self.focus = Some(target.clone());
                Ok(NavigationOutcome::Target(target))
            }
            NavigationOutcome::Unhandled => {
                if self
                    .scene
                    .as_ref()
                    .unwrap()
                    .objects()
                    .enabled_button(&target)
                {
                    Ok(NavigationOutcome::Unhandled)
                } else {
                    Ok(match self.preferred_navigation() {
                        NavigationOutcome::Unhandled => NavigationOutcome::Handled,
                        result => result,
                    })
                }
            }
            result => Ok(result),
        }
    }

    fn preferred_navigation(&mut self) -> NavigationOutcome {
        let scene = self.scene.as_ref().unwrap();
        if let Some(target) = scene
            .preferred_focus()
            .filter(|target| scene.objects().enabled_button(target))
        {
            self.focus = Some(target.clone());
            NavigationOutcome::Target(target.clone())
        } else {
            NavigationOutcome::Unhandled
        }
    }

    fn prepare_input_button(&mut self, host: &mut dyn Host) -> Result<()> {
        if !self.first_update {
            return Ok(());
        }
        self.first_update = false;
        let button = host.create_task_child(kind::BUTTON, b"low button", b"")?;
        self.input_button = Some(button);
        let [width, height] = host.surface_size();
        let priority = services(host)?.button_priority()?;
        let mut rectangle: Vec<u8> = [0, 0, width, height, priority, button]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        self.send(
            host,
            button,
            90,
            [0, 0, 1],
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut rectangle),
            },
        )?;
        self.send(host, button, 93, [0, 1, 1], &mut CommandBuffers::default())?;
        Ok(())
    }

    fn synchronize(&self, host: &mut dyn Host) {
        host.set_task_id(self.id);
        host.set_task_clock(&self.base.clock);
        host.set_task_context(&self.context);
    }

    fn effects(&mut self, host: &mut dyn Host) {
        if host.take_task_effects(0).delete {
            self.base.control.deletion_requested = 1;
            if let Some(scene) = &mut self.scene {
                scene.set_system_value("_EXITCODE_", -1);
            }
        }
    }

    fn send(
        &self,
        host: &mut dyn Host,
        target: u32,
        code: u32,
        data: [u32; 3],
        buffers: &mut CommandBuffers<'_>,
    ) -> Result<u32> {
        let mut output = 0;
        if !host.dispatch_task_command(
            Command {
                target,
                sender: self.id,
                code,
                data,
            },
            buffers,
            &mut output,
        )? {
            return Err(Error::new(0, "FES 服务任务不可达"));
        }
        Ok(output)
    }

    fn bind(&mut self) -> Result<()> {
        if let Some(scene) = &mut self.scene {
            let objects = scene.objects_mut()?;
            if let Some(list) = self.list {
                objects.set_image_list(list, self.clip);
            }
            objects.set_canvas_offset(self.offset);
            objects.set_input_task(self.input);
        }
        Ok(())
    }

    fn run_body(&mut self, host: &mut dyn Host, budget: usize) -> Result<bool> {
        self.synchronize(host);
        self.effects(host);
        if self.base.control.deletion_requested != 0 || self.paused != 0 {
            return Ok(true);
        }
        self.prepare_input_button(host)?;
        if !self.started {
            return Ok(true);
        }
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| Error::new(0, "FES 尚未装载"))?;
        if !scene.is_pending() {
            self.update_pointer_values(host)?;
            let mode = self.send(
                host,
                self.input,
                145,
                [12, 0, 1],
                &mut CommandBuffers::default(),
            )?;
            if mode != self.input_mode {
                self.scene
                    .as_mut()
                    .unwrap()
                    .record_input_event(if mode != 0 { 19 } else { 20 });
                self.input_mode = mode;
            }
        }
        let result = services(host)?.update(
            self.scene.as_mut().unwrap(),
            self.base.clock.previous_time,
            budget,
        );
        if let Some(code) = self.scene.as_mut().unwrap().take_exit_request() {
            self.base.control.property = code as u32;
        }
        self.effects(host);
        result
    }
}

impl TaskObject for FesObject {
    fn accepts_deferred_delete(&self) -> bool {
        true
    }

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
        self.context = name(context, 32)?.to_vec();
        Ok(())
    }

    fn update_body(&mut self, host: &mut dyn Host, _increment: u32, budget: usize) -> Result<bool> {
        if self.pending.is_some() {
            return Err(Error::new(0, "FES 上一次命令尚未完成"));
        }
        self.run_body(host, budget)
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
        if self.base.control.deletion_requested != 0 {
            return Ok(CommandOutcome::finished(false));
        }
        if command.code != 3 && self.pending.is_some_and(|pending| pending != *command) {
            return Err(Error::new(0, "FES 上一次命令尚未完成"));
        }
        self.synchronize(host);
        let b = command.data[1];
        match command.code {
            9 | 10 | 64 | 65 => {
                if !self.input_event(command, output, host, budget)? {
                    return Ok(CommandOutcome::RESUME);
                }
            }
            3 => {
                self.request_delete(host, budget)?;
            }
            25 => {
                if let Some(buffer) = &mut buffers.data1 {
                    if buffer.len() <= self.context.len() {
                        return Err(Error::new(0, "FES 上下文输出容量不足"));
                    }
                    buffer[..self.context.len()].copy_from_slice(&self.context);
                    buffer[self.context.len()] = 0;
                } else if b != 0 {
                    return Err(Error::new(0, "FES 上下文输出缓冲区缺失"));
                }
            }
            26 => {
                if let Some(buffer) = &buffers.data1 {
                    self.set_creation_context(buffer)?;
                } else if b != 0 {
                    return Err(Error::new(0, "FES 上下文输入缓冲区缺失"));
                }
            }
            337 => {
                if self.initialized || b != 0 {
                    return Err(Error::new(
                        0,
                        "FES 初始化需要未初始化对象及运行时隐式共享状态",
                    ));
                }
                self.sound = self.send(host, 0, 35, [0; 3], &mut CommandBuffers::default())?;
                self.input = self.send(host, 0, 34, [0; 3], &mut CommandBuffers::default())?;
                let inherited = services(host)?.inherited_context()?;
                self.set_creation_context(&inherited)?;
                self.initialized = true;
            }
            338 => {
                if self.pending.is_none() {
                    if !self.initialized || self.scene.is_some() {
                        return Err(Error::new(0, "FES 装载需要已初始化且尚未装载的对象"));
                    }
                    let file = name(
                        buffers
                            .data1
                            .as_deref()
                            .ok_or_else(|| Error::new(0, "FES 文件名缓冲区缺失"))?,
                        260,
                    )?;
                    let bytes = host
                        .resource_services()
                        .ok_or_else(|| Error::new(0, "宿主未提供 FES 资源读取"))?
                        .read(file, crate::resource::ResourceMode::LooseFirst)?;
                    let Some(bytes) = bytes else {
                        self.base.control.property = (-2i32) as u32;
                        return Ok(CommandOutcome::finished(false));
                    };
                    let handles = host
                        .handle_allocator()
                        .ok_or_else(|| Error::new(0, "宿主未提供 FES 共享编号源"))?;
                    let mut scene = Scene::new(fes::Program::parse(&bytes)?, handles)?;
                    scene.set_system_value("_EXITCODE_", -1);
                    scene.set_system_value("_SAVEOBJ_", 0);
                    scene.set_system_value("_FRAME_", 0);
                    self.name = file.to_vec();
                    self.scene = Some(scene);
                    self.bind()?;
                    self.pending = Some(*command);
                }
                if !services(host)?.prepare(self.scene.as_mut().unwrap(), budget)? {
                    return Ok(CommandOutcome::RESUME);
                }
                self.pending = None;
            }
            339 => {
                if self.scene.is_none() {
                    return Err(Error::new(0, "FES 启动前必须装载脚本"));
                }
                if self.pending.is_none() {
                    self.started = true;
                    self.pending = Some(*command);
                    services(host)?.start(&mut self.base.clock)?;
                }
                if !self.run_body(host, budget)? {
                    return Ok(CommandOutcome::RESUME);
                }
                self.pending = None;
            }
            340 => {
                if self.scene.as_ref().is_some_and(Scene::is_pending) {
                    return Err(Error::new(0, "FES 本轮推进尚未完成"));
                }
                self.list = Some(b);
                let mut image = [0; 4];
                self.send(
                    host,
                    b,
                    291,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: None,
                        data1: Some(&mut image),
                    },
                )?;
                let info = host.image(u32::from_le_bytes(image))?.info();
                self.clip = crate::graphics::Rect::from_xywh(
                    0,
                    0,
                    info.size[0] as i32,
                    info.size[1] as i32,
                );
                let mut x = [0; 4];
                let mut y = [0; 4];
                self.send(
                    host,
                    b,
                    290,
                    [0; 3],
                    &mut CommandBuffers {
                        data0: Some(&mut x),
                        data1: Some(&mut y),
                    },
                )?;
                self.offset = [i32::from_le_bytes(x), i32::from_le_bytes(y)];
                self.bind()?;
            }
            348 => {
                if self.pending.is_none() {
                    if self.paused == b {
                        return Ok(CommandOutcome::finished(true));
                    }
                    self.paused = b;
                    self.pause_script_done = false;
                    self.pending = Some(*command);
                }
                if let Some(scene) = &mut self.scene {
                    if !self.pause_script_done {
                        let name = if b != 0 { "_FES_SLEEP_" } else { "_FES_WAKE_" };
                        if !services(host)?.dispatch_system_block(scene, name, budget)? {
                            return Ok(CommandOutcome::RESUME);
                        }
                        self.pause_script_done = true;
                    }
                    services(host)?.set_media_sleep(scene, b != 0)?;
                }
                self.pending = None;
                self.pause_script_done = false;
                self.effects(host);
                return Ok(CommandOutcome::finished(true));
            }
            19 | 27 | 28 | 177 | 178 | 341..=347 | 349..=358 => {
                return Err(Error::new(0, "FES 专属命令尚未接入"));
            }
            _ => {
                return self
                    .base
                    .dispatch_command(kind, command, output, host, budget);
            }
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
        if let Some(scene) = &mut self.scene {
            scene.set_system_value("_EXITCODE_", -1);
        }
        self.base.request_delete(host, budget)
    }
}

/// 创建未装载的类型 34 对象，其余类型委托已有工厂。
pub struct FesFactory<F> {
    pub fallback: F,
}

impl<F: ObjectFactory> ObjectFactory for FesFactory<F> {
    fn create(&mut self, id: u32, object_kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        if object_kind == kind::FES {
            Ok(Box::new(FesObject::new(id)))
        } else {
            self.fallback.create(id, object_kind, name)
        }
    }
}

fn services(host: &mut dyn Host) -> Result<&mut dyn Services> {
    host.fes_services()
        .ok_or_else(|| Error::new(0, "宿主未提供 FES 运行时服务"))
}

fn name(bytes: &[u8], capacity: usize) -> Result<&[u8]> {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    if end >= capacity {
        return Err(Error::new(0, "FES 名称或上下文超过容量"));
    }
    Ok(&bytes[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    struct InputHost(Vec<Command>);
    impl Host for InputHost {
        fn call(
            &mut self,
            _: u32,
            _: &[u8],
            _: &mut Interpreter,
        ) -> Result<crate::kcs::NativeReturn> {
            Err(Error::new(0, "合成宿主不执行 native"))
        }
        fn dispatch_task_command(
            &mut self,
            command: Command,
            _: &mut CommandBuffers<'_>,
            output: &mut u32,
        ) -> Result<bool> {
            assert!(matches!(command.code, 95 | 96));
            if command.code == 96 {
                *output = 0x80000002;
            }
            self.0.push(command);
            Ok(true)
        }
    }

    #[test]
    fn paused_events_acknowledge_without_pointer_or_script_input_changes() {
        let mut task = FesObject::new(9);
        task.paused = 2;
        let mut scene = Scene::new(
            fes::Program::from_text(b"#START\nwait 77\n").unwrap(),
            crate::HandleAllocator::default(),
        )
        .unwrap();
        scene.set_system_value("_CLICK_L_", 73);
        task.scene = Some(scene);
        let mut host = InputHost(Vec::new());
        for code in [9, 10, 64, 65] {
            let mut output = 0x13579bdf;
            assert_eq!(
                task.dispatch_command(
                    kind::FES,
                    &Command {
                        target: 9,
                        sender: 5,
                        code,
                        data: [5, 0, 0],
                    },
                    &mut output,
                    &mut host,
                    1
                )
                .unwrap(),
                CommandOutcome::finished(false)
            );
            assert_eq!(output, if code < 64 { 0x13579bdf } else { 0 });
            assert_eq!(task.last_input(), if code == 9 { 0x80000002 } else { 0 });
            let scene = task.scene().unwrap();
            assert_eq!(scene.raw_input(), 0x80000002);
            assert_eq!(scene.interpreter().named["_CLICK_L_"][&0], 73);
            assert!(!scene.interpreter().named.contains_key("_MOUSE_X_"));
        }
        assert_eq!(
            host.0,
            [96, 95, 95].map(|code| Command {
                target: 5,
                sender: 9,
                code,
                data: [0, 1, 1],
            })
        );
    }
}
