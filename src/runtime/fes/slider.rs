use super::{ObjectState, Objects, Scene};
use crate::{
    Error, Result,
    runtime::{FesSceneLoan, ObjectFactory, Store, kind, route_with_buffers},
    task::{Command, CommandBuffers},
};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn check_fes_slider_binding(&self, owner: u32, task: Option<u32>) -> Result<()> {
        if task.is_some_and(|task| {
            !self
                .tree
                .node(task)
                .is_some_and(|node| node.parent == Some(owner) && node.kind == kind::SLIDER)
        }) {
            return Err(Error::new(0, "FES 下层滑块任务绑定失效"));
        }
        Ok(())
    }

    pub(super) fn update_fes_slider(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let task = self.prepare_fes_slider_update(owner, objects, handle)?;
        if let Some(data) = position_request(objects, handle) {
            self.slider_command(owner, task, 369, data, None)?;
            finish_position(objects, handle);
        }
        self.finish_fes_slider_update(owner, objects, handle, task)
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
        let Some(handle) = scene
            .objects
            .lookup(&record.target.name, record.target.bank)
        else {
            return Ok(());
        };
        let image = record.image;
        if !matches!(
            scene.objects.get(handle).unwrap().state,
            ObjectState::Slider(_)
        ) {
            return self.update_fes_object_image(owner, &mut scene.objects, handle, image);
        }
        let task = self.prepare_fes_slider_update(owner, &mut scene.objects, handle)?;
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
            store.slider_command(owner, task, 369, data, None)?;
            finish_position(&mut scene.objects, handle);
        }
        self.finish_fes_slider_update(owner, &mut scene.objects, handle, task)
    }

    fn prepare_fes_slider_update(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<u32> {
        let object = objects.get(handle).unwrap();
        self.check_fes_slider_binding(owner, object.task_id)?;
        let task = object
            .task_id
            .ok_or_else(|| Error::new(0, "FES SLIDER 尚未完成装载准备"))?;
        if object.state.slider().unwrap().updates.file {
            let list = self.check_fes_image_list(owner, objects)?;
            let state = objects.get(handle).unwrap().state.slider().unwrap();
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
                    .ok_or_else(|| Error::new(0, "FES SLIDER 位置元数据请求缺少资源引用"))?;
                objects
                    .get_mut(handle)
                    .unwrap()
                    .state
                    .slider_mut()
                    .unwrap()
                    .updates
                    .metadata = false;
                let position = self.fes_image_geometry(&reference, false)?;
                let size = self.fes_image_geometry(&reference, true)?;
                objects
                    .get_mut(handle)
                    .unwrap()
                    .state
                    .slider_mut()
                    .unwrap()
                    .set_rectangle([
                        position[0],
                        position[1],
                        position[0].wrapping_add(size[0]),
                        position[1].wrapping_add(size[1]),
                    ]);
            }
            let state = objects.get(handle).unwrap().state.slider().unwrap();
            let name = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(0, "运行时未配置 FES 滑块资源服务"))?
                .resolve(b"image", &state.file)?;
            if name.len() >= 260 || name.contains(&0) {
                return Err(Error::new(
                    0,
                    "FES SLIDER 完整资源名称超过缓冲区或包含零字节",
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
            self.slider_command(owner, task, 365, [0, 0, 1], Some(&mut config))?;
            let direction = self.slider_command(owner, task, 370, [0, 0, 1], Some(&mut config))?;
            let state = objects.get_mut(handle).unwrap().state.slider_mut().unwrap();
            state.vertical = direction as i32;
            state.updates.file = false;
        }
        let state = objects.get_mut(handle).unwrap().state.slider_mut().unwrap();
        if state.updates.range {
            state.updates.range = false;
            let mut range = state.range.map(i32::to_le_bytes).concat();
            self.slider_command(owner, task, 368, [0, 0, 1], Some(&mut range))?;
        }
        Ok(task)
    }

    fn finish_fes_slider_update(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        task: u32,
    ) -> Result<()> {
        let state = objects.get_mut(handle).unwrap().state.slider_mut().unwrap();
        if state.updates.alpha {
            state.updates.alpha = false;
            self.slider_command(owner, task, 371, [0, state.alpha, 1], None)?;
        }
        let state = objects.get(handle).unwrap().state.slider().unwrap();
        if state.updates.dirty {
            self.slider_command(owner, task, 367, [0, state.enabled as u32, 1], None)?;
            let display = objects.get(handle).unwrap().state.slider().unwrap().display;
            self.slider_command(owner, task, 366, [0, display as u32, 1], None)?;
            objects
                .get_mut(handle)
                .unwrap()
                .state
                .slider_mut()
                .unwrap()
                .updates
                .dirty = false;
        }
        let state = objects.get_mut(handle).unwrap().state.slider_mut().unwrap();
        if state.updates.frames {
            state.updates.frames = false;
            for index in 0..4 {
                let frame = objects.get(handle).unwrap().state.slider().unwrap().frames[index];
                self.slider_command(owner, task, 372, [index as u32, frame, 1], None)?;
            }
            self.slider_command(owner, task, 373, [0, 0, 1], None)?;
        }
        Ok(())
    }

    fn slider_command(
        &mut self,
        owner: u32,
        task: u32,
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
                code,
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
    let state = objects.get_mut(handle).unwrap().state.slider_mut().unwrap();
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
        .slider_mut()
        .unwrap()
        .updates
        .position = false;
}
