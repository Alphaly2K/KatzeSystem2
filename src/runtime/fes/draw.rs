use super::{ObjectState, Objects, Scene};
use crate::graphics::{Rect, Surface};
use crate::runtime::{ObjectFactory, Store, create_object, kind, route_with_buffers};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result, fes};

const NAME: &str = "__drawobj__";

/// DRAW 的持续时间和媒体阶段；下层任务与截图由所属 FES 持有。
#[derive(Clone, Debug, PartialEq)]
pub struct DrawState {
    duration: u32,
    phase: Option<i32>,
    parameters: Option<[u8; 44]>,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            duration: 30,
            phase: None,
            parameters: None,
        }
    }
}

impl DrawState {
    pub fn duration(&self) -> u32 {
        self.duration
    }

    /// 尚未登记时为 None，准备后依次为 -2、-1、0 和所属任务 elapsed。
    pub fn phase(&self) -> Option<i32> {
        self.phase
    }
}

pub(super) struct DrawCall {
    handle: u32,
    motion_name: Option<Vec<u8>>,
}

impl Objects {
    fn draw(&self, handle: u32) -> &DrawState {
        let ObjectState::Draw(state) = &self.get(handle).unwrap().state else {
            unreachable!()
        };
        state
    }

    fn draw_mut(&mut self, handle: u32) -> &mut DrawState {
        let ObjectState::Draw(state) = &mut self.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        state
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn run_fes_draw(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<bool> {
        if args.get(1).is_none_or(|name| name != b"draw") {
            return Ok(false);
        }
        if scene.draw_call.is_none() {
            let mut target = if args[0] == b"@" {
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
            let parent = scene
                .objects
                .lookup(
                    &target.name,
                    if target.bank == -1 { 0 } else { target.bank },
                )
                .ok_or_else(|| Error::new(offset, "FES 对象目标不存在，整行表达式回退尚未接入"))?;
            if scene.objects.count(&target.name) == 1 {
                target.bank = 0;
            }
            if target.bank == -1
                || !matches!(
                    scene.objects.get(parent).unwrap().state,
                    ObjectState::Plane(_)
                )
                || scene.objects.lookup(NAME, 0).is_some()
            {
                return Ok(true);
            }
            let duration = match args.get(2) {
                Some(value) => scene.interpreter.eval(value, self.numbers)?.max(1) as u32,
                None => 30,
            };
            let handle = scene.objects.define(&fes::ObjectDefinition {
                kind: "DRAW",
                name: NAME.to_owned(),
                banks: 1,
            })?[0];
            scene.objects.set_parent(handle, Some(parent))?;
            scene.objects.draw_mut(handle).duration = duration;
            scene.objects.publish_bindings(&mut scene.interpreter);
            scene.draw_call = Some(DrawCall {
                handle,
                motion_name: None,
            });
        }
        let pending = scene.draw_call.as_mut().unwrap();
        if pending.motion_name.is_none() {
            let services = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(offset, "运行时未配置 FES 运动资源服务"))?;
            pending.motion_name = Some(services.resolve(b"mot", b"d")?);
        }
        if let Some(program) = self
            .resources
            .load_motion(pending.motion_name.as_ref().unwrap())?
        {
            scene
                .objects
                .start_motion(pending.handle, program, &[], 0)?;
        }
        scene.draw_call = None;
        Ok(true)
    }

    pub(super) fn prepare_fes_draw(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let parent = objects.get(handle).unwrap().parent;
        let Some(ObjectState::Plane(plane)) =
            parent.and_then(|parent| objects.get(parent).map(|object| &object.state))
        else {
            let object = objects.get_mut(handle).unwrap();
            object.motions.clear();
            object.motion_delay = 0;
            return Ok(());
        };
        let [x, y, _, _] = plane.vram();
        let position = [x, y].map(super::plane::integer_position);
        let size = plane.base.size();
        let list = self.check_fes_image_list(owner, objects)?;
        self.check_fes_draw_binding(owner, objects.get(handle).unwrap().task_id)?;
        if objects.get(handle).unwrap().task_id.is_none() {
            objects.draw_mut(handle).phase = Some(-2);
            let task = create_object(self.reborrow(), owner, kind::FADE, b"FE Draw", b"")?;
            objects.get_mut(handle).unwrap().task_id = Some(task);
        }
        if objects.draw(handle).parameters.is_none() {
            let mut image = [0; 4];
            self.fes_command(owner, Some(list), 291, [0; 3], Some(&mut image))?;
            let source = u32::from_le_bytes(image);
            let mut x = [0; 4];
            let mut y = [0; 4];
            route_with_buffers(
                self.reborrow(),
                owner,
                Command {
                    target: list,
                    sender: owner,
                    code: 290,
                    data: [0; 3],
                },
                &mut CommandBuffers {
                    data0: Some(&mut x),
                    data1: Some(&mut y),
                },
            )?;
            let offset = [i32::from_le_bytes(x), i32::from_le_bytes(y)];
            if size.iter().any(|&value| value < 0) {
                return Err(Error::new(0, "FES DRAW 截图尺寸不能为负数"));
            }
            let mut snapshot = Surface::new(size.map(|value| value as u32))?;
            let image = self.resources.images.get(source)?;
            snapshot.copy_from(
                image.surface(),
                Rect::from_xywh(
                    position[0],
                    (image.info().size[1] as i32)
                        .wrapping_sub(position[1])
                        .wrapping_sub(size[1]),
                    size[0],
                    size[1],
                ),
                [0; 2],
            );
            let old = self
                .resources
                .images
                .create(owner, size.map(|value| value as u32))?;
            self.resources
                .images
                .get_mut(old)?
                .pixels_mut()
                .copy_from_slice(snapshot.pixels());
            let object = objects.get_mut(handle).unwrap();
            object.image = Some(old);
            object.images.push(old);
            let words = [
                objects.draw(handle).duration,
                old,
                0,
                0,
                source,
                position[0] as u32,
                position[1] as u32,
                offset[0].wrapping_add(position[0]) as u32,
                offset[1].wrapping_add(position[1]) as u32,
                size[0] as u32,
                size[1] as u32,
            ];
            let mut parameters = [0; 44];
            for (slot, value) in parameters.chunks_exact_mut(4).zip(words) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            objects.draw_mut(handle).parameters = Some(parameters);
        }
        self.fes_command(owner, Some(list), 292, [0; 3], None)
    }

    pub(super) fn update_fes_draw(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        self.restore_fes_draw_submission(owner, objects)?;
        let state = objects.draw(handle);
        let task = objects.get(handle).unwrap().task_id;
        self.check_fes_draw_binding(owner, task)?;
        if state.phase.is_some() && task.is_none() {
            return Err(Error::new(0, "FES DRAW 下层任务尚未创建"));
        }
        match state.phase {
            None => (),
            Some(-2) => objects.draw_mut(handle).phase = Some(-1),
            Some(-1) => {
                let mut parameters = state
                    .parameters
                    .ok_or_else(|| Error::new(0, "FES DRAW 来源尚未准备"))?;
                self.fes_command(owner, task, 281, [0; 3], Some(&mut parameters))?;
                objects.draw_mut(handle).phase = Some(0);
            }
            Some(0) => {
                let elapsed = self
                    .running
                    .get(&owner)
                    .map(|task| task.clock.elapsed())
                    .or_else(|| {
                        self.objects
                            .get(&owner)
                            .map(|object| object.clock().elapsed())
                    })
                    .ok_or_else(|| Error::new(0, "FES DRAW 所属任务时钟不可用"))?;
                self.fes_command(owner, task, 263, [0; 3], None)?;
                objects.draw_mut(handle).phase = Some(elapsed as i32);
            }
            Some(_) => {
                let active = route_with_buffers(
                    self.reborrow(),
                    owner,
                    Command {
                        target: task.unwrap(),
                        sender: owner,
                        code: 265,
                        data: [0; 3],
                    },
                    &mut CommandBuffers::default(),
                )?;
                if active == 0 {
                    let list = self.check_fes_image_list(owner, objects)?;
                    self.remove_fes_object(owner, objects, handle)?;
                    objects.draw_restore = Some(list);
                    self.restore_fes_draw_submission(owner, objects)?;
                } else if !objects.motion_active(handle)? {
                    self.fes_command(owner, task, 264, [0; 3], None)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn restore_fes_draw_submission(
        &mut self,
        owner: u32,
        objects: &mut Objects,
    ) -> Result<()> {
        if let Some(list) = objects.draw_restore {
            if !self
                .tree
                .find_reachable(owner, list)
                .is_some_and(|node| node.kind == kind::IMAGE_LIST)
            {
                return Err(Error::new(0, "FES DRAW 待恢复的共享列表绑定失效"));
            }
            self.fes_command(owner, Some(list), 292, [0, 1, 0], None)?;
            objects.draw_restore = None;
        }
        Ok(())
    }

    pub(super) fn check_fes_draw_binding(&self, owner: u32, task: Option<u32>) -> Result<()> {
        if task.is_some_and(|task| {
            !self
                .tree
                .node(task)
                .is_some_and(|node| node.parent == Some(owner) && node.kind == kind::FADE)
        }) {
            return Err(Error::new(0, "FES 下层淡入淡出任务绑定失效"));
        }
        Ok(())
    }
}
