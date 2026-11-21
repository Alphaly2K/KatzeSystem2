use super::{ObjectState, Objects, Scene};
use crate::graphics::RenderTarget;
use crate::runtime::{ObjectFactory, Store, create_object, kind, route_with_buffers};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, fes};

#[derive(Clone, Debug, PartialEq)]
enum Capture {
    Pending,
    Done(Option<RenderTarget>),
}

/// RDRAW 和 RWIPE 的捕获参数及媒体阶段。
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenTransition {
    wipe: bool,
    duration: u32,
    range: [f32; 2],
    rule: Vec<u8>,
    capture: Capture,
    mask: u32,
    phase: Option<i32>,
}

impl ScreenTransition {
    pub(super) fn new(kind: &str) -> Self {
        Self {
            wipe: kind == "RWIPE",
            duration: 30,
            range: [0., 60_000.],
            rule: Vec::new(),
            capture: Capture::Pending,
            mask: 0,
            phase: None,
        }
    }

    pub(super) fn kind(&self) -> &'static str {
        if self.wipe { "RWIPE" } else { "RDRAW" }
    }

    fn task_kind(&self) -> u32 {
        if self.wipe {
            kind::RWIPE
        } else {
            kind::RENDER_DRAW
        }
    }

    fn configure_code(&self) -> u32 {
        if self.wipe { 393 } else { 390 }
    }

    fn parameters(&self) -> [u8; 20] {
        let mut bytes = [0; 20];
        let [near, far] = self.range.map(f32::to_bits);
        for (slot, word) in bytes
            .chunks_exact_mut(4)
            .zip([near, far, self.duration, self.mask])
        {
            slot.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

impl Objects {
    fn screen_transition(&self, handle: u32) -> &ScreenTransition {
        let ObjectState::ScreenTransition(state) = &self.get(handle).unwrap().state else {
            unreachable!()
        };
        state
    }

    fn screen_transition_mut(&mut self, handle: u32) -> &mut ScreenTransition {
        let ObjectState::ScreenTransition(state) = &mut self.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        state
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn run_fes_screen_transition(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
    ) -> Result<bool> {
        let (kind, name) = match args.first().map(Vec::as_slice) {
            Some(b"rdraw") => ("RDRAW", "__rdrawobj__"),
            Some(b"rwipe") => ("RWIPE", "__rwipeobj__"),
            _ => return Ok(false),
        };
        if scene.objects.lookup(name, 0).is_some() {
            return Ok(true);
        }
        let mut state = ScreenTransition::new(kind);
        if let Some(value) = args.get(1) {
            let value = scene.interpreter.eval(value, self.numbers)?;
            state.duration = if state.wipe { value } else { value.max(1) } as u32;
        }
        if state.wipe {
            if let Some(name) = args.get(2) {
                state.rule = self.fes_transition_resource(b"image", name)?;
            }
        }
        for (limit, value) in state
            .range
            .iter_mut()
            .zip(args.iter().skip(2 + usize::from(state.wipe)))
        {
            *limit = scene.interpreter.eval(value, self.numbers)? as u32 as f32;
        }
        state.capture = Capture::Done(self.capture_fes_transition(state.range)?);
        let motion_name = self.fes_transition_resource(b"mot", b"d")?;
        let motion = self.resources.load_motion(&motion_name)?;
        let handle = scene.objects.define(&fes::ObjectDefinition {
            kind,
            name: name.to_owned(),
            banks: 1,
        })?[0];
        *scene.objects.screen_transition_mut(handle) = state;
        if let Some(program) = motion {
            scene.objects.start_motion(handle, program, &[], 0)?;
        }
        scene.objects.publish_bindings(&mut scene.interpreter);
        Ok(true)
    }

    fn fes_transition_resource(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.resources
            .services
            .as_deref_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置 FES 转场资源服务"))?
            .resolve(category, name)
    }

    fn capture_fes_transition(&mut self, range: [f32; 2]) -> Result<Option<RenderTarget>> {
        if self
            .host
            .platform_settings
            .as_ref()
            .is_some_and(|settings| {
                settings.d3d_backbuffer == 0
                    || (settings.d3d_tex_size as i32 > 0
                        && self
                            .host
                            .surface_size
                            .iter()
                            .any(|&side| side > settings.d3d_tex_size))
            })
        {
            return Ok(None);
        }
        self.resources
            .readback_depth_frame(self.host.surface_size, range)?
            .map(|frame| RenderTarget::from_tiles(frame.size(), [1, 1], vec![Some(frame)]))
            .transpose()
    }

    pub(super) fn prepare_fes_screen_transition(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let state = objects.screen_transition(handle);
        let task = objects.get(handle).unwrap().task_id;
        self.check_fes_screen_transition_binding(owner, task, state)?;
        if state.phase.is_some() {
            return Ok(());
        }
        if state.wipe {
            if state.mask == 0 && !state.rule.is_empty() {
                let services = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置 FES 转场资源服务"))?;
                if let Some(mask) =
                    self.resources
                        .images
                        .decode(&state.rule, Some(0), false, services)?
                {
                    let image = self.resources.images.insert(owner, mask, None)?;
                    let pixels = self.resources.images.pixel_handle(image)?;
                    objects.get_mut(handle).unwrap().images.push(image);
                    objects.screen_transition_mut(handle).mask = pixels;
                }
            }
            if objects.screen_transition(handle).mask == 0 {
                let object = objects.get_mut(handle).unwrap();
                object.motions.clear();
                object.motion_delay = 0;
                return Ok(());
            }
        }
        let state = objects.screen_transition_mut(handle);
        if matches!(state.capture, Capture::Pending) {
            state.capture = Capture::Done(self.capture_fes_transition(state.range)?);
        }
        if task.is_none() {
            let Capture::Done(Some(frame)) = &state.capture else {
                return Ok(());
            };
            // 下层任务构造从辅助目标零取得旧画面。
            let targets = &mut self.resources.compositor.auxiliary_targets;
            if targets.is_empty() {
                targets.push(frame.clone());
            } else {
                targets[0] = frame.clone();
            }
            let name = if state.wipe { b"FE RWipe" } else { b"FE RDraw" };
            let task = create_object(self.reborrow(), owner, state.task_kind(), name, b"")?;
            objects.get_mut(handle).unwrap().task_id = Some(task);
        }
        let task = objects.get(handle).unwrap().task_id.unwrap();
        let state = objects.screen_transition_mut(handle);
        self.configure_fes_screen_transition(owner, task, state)?;
        state.capture = Capture::Done(None);
        state.phase = Some(-2);
        Ok(())
    }

    fn configure_fes_screen_transition(
        &mut self,
        owner: u32,
        task: u32,
        state: &ScreenTransition,
    ) -> Result<()> {
        let mut parameters = state.parameters();
        let length = if state.wipe { 20 } else { 12 };
        self.fes_command(
            owner,
            Some(task),
            state.configure_code(),
            [0; 3],
            Some(&mut parameters[..length]),
        )
    }

    pub(super) fn update_fes_screen_transition(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let object = objects.get(handle).unwrap();
        let task = object.task_id;
        let state = objects.screen_transition(handle);
        self.check_fes_screen_transition_binding(owner, task, state)?;
        let Some(task) = task else {
            return self.remove_fes_object(owner, objects, handle);
        };
        let state_code = state.configure_code() + 1;
        match state.phase {
            Some(-2) => objects.screen_transition_mut(handle).phase = Some(-1),
            Some(-1) => {
                self.configure_fes_screen_transition(owner, task, state)?;
                objects.screen_transition_mut(handle).phase = Some(0);
            }
            Some(0) => {
                let now = self
                    .running
                    .get(&owner)
                    .map(|task| task.clock.elapsed())
                    .or_else(|| {
                        self.objects
                            .get(&owner)
                            .map(|object| object.clock().elapsed())
                    })
                    .ok_or_else(|| Error::new(0, "FES 转场所属任务时钟不可用"))?;
                self.fes_command(owner, Some(task), state_code, [0, 1, 0], None)?;
                objects.screen_transition_mut(handle).phase = Some(now as i32);
            }
            Some(_) => {
                let active = route_with_buffers(
                    self.reborrow(),
                    owner,
                    Command {
                        target: task,
                        sender: owner,
                        code: 383,
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                )?;
                if active == 0 {
                    self.remove_fes_object(owner, objects, handle)?;
                } else if !objects.motion_active(handle)? {
                    self.fes_command(owner, Some(task), state_code, [0; 3], None)?;
                }
            }
            None => (),
        }
        Ok(())
    }

    pub(super) fn check_fes_screen_transition_binding(
        &self,
        owner: u32,
        task: Option<u32>,
        state: &ScreenTransition,
    ) -> Result<()> {
        if task.is_some_and(|task| {
            !self
                .tree
                .node(task)
                .is_some_and(|node| node.parent == Some(owner) && node.kind == state.task_kind())
        }) {
            return Err(Error::new(0, "FES 下层转场任务绑定失效"));
        }
        Ok(())
    }
}
