use super::{ObjectState, Objects, Scene};
use crate::{
    Error, Result,
    runtime::{FesSceneLoan, ObjectFactory, Store, kind, route_with_buffers},
    task::{Command, CommandBuffers},
};

#[derive(Clone, Copy)]
pub(super) enum ControlKind {
    Slider,
    Scroll2,
}

impl ControlKind {
    pub(super) fn of(state: &ObjectState) -> Option<Self> {
        match state {
            ObjectState::Slider(_) => Some(Self::Slider),
            ObjectState::Scroll2(_) => Some(Self::Scroll2),
            _ => None,
        }
    }

    pub(super) fn task_kind(self) -> u32 {
        match self {
            Self::Slider => kind::SLIDER,
            Self::Scroll2 => kind::SCROLL2,
        }
    }

    pub(super) fn task_name(self) -> &'static [u8] {
        match self {
            Self::Slider => b"FE slider",
            Self::Scroll2 => b"FE scroll2",
        }
    }

    pub(super) fn command(self, code: u32) -> u32 {
        match self {
            Self::Slider => code,
            Self::Scroll2 => match code {
                365 => 374,
                366 => 379,
                367 => 375,
                368 => 376,
                369 => 377,
                370 => 378,
                371 => 380,
                372 => 381,
                373 => 382,
                _ => unreachable!(),
            },
        }
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn check_fes_control_binding(
        &self,
        owner: u32,
        task: Option<u32>,
        protocol: ControlKind,
    ) -> Result<()> {
        if task.is_some_and(|task| {
            !self
                .tree
                .node(task)
                .is_some_and(|node| node.parent == Some(owner) && node.kind == protocol.task_kind())
        }) {
            return Err(Error::new(0, "FES 下层滑动控件任务绑定失效"));
        }
        Ok(())
    }

    pub(super) fn update_fes_control(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let (task, protocol) = self.prepare_fes_control_update(owner, objects, handle)?;
        if let Some(data) = position_request(objects, handle) {
            self.control_command(owner, task, protocol, 369, data, None)?;
            finish_position(objects, handle);
        }
        self.finish_fes_control_update(owner, objects, handle, task, protocol)
    }

    pub(super) fn update_fes_scene_record(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        index: usize,
    ) -> Result<()> {
        if self.tree.node(owner).is_none() {
            return Err(Error::new(0, "FES 所属任务不存在"));
        }
        self.prepare_fes_object(owner, &mut scene.objects, index)?;
        let record = &scene.objects.update_order[index];
        let draw = record.kind == "DRAW";
        let Some(handle) = scene.objects.media_handle(record) else {
            if draw {
                scene.objects.publish_bindings(&mut scene.interpreter);
            }
            return Ok(());
        };
        let image = record.image;
        if scene.objects.get(handle).unwrap().state.control().is_none() {
            self.update_fes_object_image(owner, &mut scene.objects, handle, image)?;
            if draw {
                scene.objects.publish_bindings(&mut scene.interpreter);
            }
            return Ok(());
        }
        let (task, protocol) =
            self.prepare_fes_control_update(owner, &mut scene.objects, handle)?;
        if let Some(data) = position_request(&mut scene.objects, handle) {
            let loan = data[0] != 0
                && self.running.contains_key(&owner)
                && self
                    .tree
                    .node(owner)
                    .is_some_and(|node| node.kind == kind::FES);
            let mut store = self.reborrow();
            if loan {
                store.fes_scene = Some(FesSceneLoan { owner, scene });
            }
            store.control_command(owner, task, protocol, 369, data, None)?;
            finish_position(&mut scene.objects, handle);
        }
        self.finish_fes_control_update(owner, &mut scene.objects, handle, task, protocol)
    }

    fn prepare_fes_control_update(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<(u32, ControlKind)> {
        let cache = objects.preloaded_images.clone();
        let object = objects.get(handle).unwrap();
        let protocol = ControlKind::of(&object.state).unwrap();
        self.check_fes_control_binding(owner, object.task_id, protocol)?;
        let task = object
            .task_id
            .ok_or_else(|| Error::new(0, "FES 滑动控件尚未完成装载准备"))?;
        if object.state.control().unwrap().updates.file {
            let list = self.check_fes_image_list(owner, objects)?;
            let state = objects.get(handle).unwrap().state.control().unwrap();
            let mut offset = [0; 2];
            if let Some(plane) = objects.lookup(&state.plane.name, state.plane.bank) {
                if matches!(objects.get(plane).unwrap().state, ObjectState::Plane(_)) {
                    let position = objects.plane_position(plane);
                    offset = [position[0], position[1]].map(integer_position);
                }
            }
            if state.updates.metadata {
                let reference = state
                    .position_resource
                    .clone()
                    .ok_or_else(|| Error::new(0, "FES 滑动控件位置元数据请求缺少资源引用"))?;
                objects
                    .get_mut(handle)
                    .unwrap()
                    .state
                    .control_mut()
                    .unwrap()
                    .updates
                    .metadata = false;
                let position = self.fes_image_geometry(&cache, &reference, false)?;
                let size = self.fes_image_geometry(&cache, &reference, true)?;
                objects
                    .get_mut(handle)
                    .unwrap()
                    .state
                    .control_mut()
                    .unwrap()
                    .set_rectangle([
                        position[0],
                        position[1],
                        position[0].wrapping_add(size[0]),
                        position[1].wrapping_add(size[1]),
                    ]);
            }
            let state = objects.get(handle).unwrap().state.control().unwrap();
            let name = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(0, "运行时未配置 FES 滑动控件资源服务"))?
                .resolve(b"image", &state.file)?;
            if name.len() >= 260 || name.contains(&0) {
                return Err(Error::new(
                    0,
                    "FES 滑动控件完整资源名称超过缓冲区或包含零字节",
                ));
            }
            let mut config = [0; 0x124];
            config[..4].copy_from_slice(&list.to_le_bytes());
            config[4..4 + name.len()].copy_from_slice(&name);
            for (dst, value) in config[0x108..].chunks_exact_mut(4).zip([
                state.priority,
                state.rectangle[0],
                state.rectangle[1],
                state.rectangle[2],
                state.rectangle[3],
                offset[0],
                offset[1],
            ]) {
                dst.copy_from_slice(&value.to_le_bytes());
            }
            self.control_command(owner, task, protocol, 365, [0, 0, 1], Some(&mut config))?;
            let direction =
                self.control_command(owner, task, protocol, 370, [0, 0, 1], Some(&mut config))?;
            let state = objects
                .get_mut(handle)
                .unwrap()
                .state
                .control_mut()
                .unwrap();
            state.vertical = direction as i32;
            state.updates.file = false;
        }
        let page = match &objects.get(handle).unwrap().state {
            ObjectState::Scroll2(state) => Some(state.page),
            _ => None,
        };
        let state = objects
            .get_mut(handle)
            .unwrap()
            .state
            .control_mut()
            .unwrap();
        if state.updates.range {
            state.updates.range = false;
            let mut range = state.range.map(i32::to_le_bytes).concat();
            if let Some(page) = page {
                range.extend(page.to_le_bytes());
            }
            self.control_command(owner, task, protocol, 368, [0, 0, 1], Some(&mut range))?;
        }
        Ok((task, protocol))
    }

    fn finish_fes_control_update(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        task: u32,
        protocol: ControlKind,
    ) -> Result<()> {
        let state = objects
            .get_mut(handle)
            .unwrap()
            .state
            .control_mut()
            .unwrap();
        if state.updates.alpha {
            state.updates.alpha = false;
            self.control_command(owner, task, protocol, 371, [0, state.alpha, 1], None)?;
        }
        let state = objects.get(handle).unwrap().state.control().unwrap();
        if state.updates.dirty {
            self.control_command(
                owner,
                task,
                protocol,
                367,
                [0, state.enabled as u32, 1],
                None,
            )?;
            let display = objects
                .get(handle)
                .unwrap()
                .state
                .control()
                .unwrap()
                .display;
            self.control_command(owner, task, protocol, 366, [0, display as u32, 1], None)?;
            objects
                .get_mut(handle)
                .unwrap()
                .state
                .control_mut()
                .unwrap()
                .updates
                .dirty = false;
        }
        let state = objects
            .get_mut(handle)
            .unwrap()
            .state
            .control_mut()
            .unwrap();
        if state.updates.frames {
            state.updates.frames = false;
            for index in 0..4 {
                let frame = objects.get(handle).unwrap().state.control().unwrap().frames[index];
                self.control_command(owner, task, protocol, 372, [index as u32, frame, 1], None)?;
            }
            self.control_command(owner, task, protocol, 373, [0, 0, 1], None)?;
        }
        Ok(())
    }

    fn control_command(
        &mut self,
        owner: u32,
        task: u32,
        protocol: ControlKind,
        code: u32,
        data: [u32; 3],
        bytes: Option<&mut [u8]>,
    ) -> Result<u32> {
        route_with_buffers(
            self.reborrow(),
            owner,
            Command {
                target: task,
                sender: owner,
                code: protocol.command(code),
                data,
            },
            &mut CommandBuffers {
                data0: None,
                data1: bytes,
            },
        )
    }
}

fn integer_position(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn position_request(objects: &mut Objects, handle: u32) -> Option<[u32; 3]> {
    let state = objects
        .get_mut(handle)
        .unwrap()
        .state
        .control_mut()
        .unwrap();
    state.updates.position.then(|| {
        [
            std::mem::take(&mut state.notification) as u32,
            state.position as u32,
            1,
        ]
    })
}

fn finish_position(objects: &mut Objects, handle: u32) {
    objects
        .get_mut(handle)
        .unwrap()
        .state
        .control_mut()
        .unwrap()
        .updates
        .position = false;
}
