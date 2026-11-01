use super::{ObjectState, Objects};
use crate::fes::ImageReference;
use crate::runtime::{ObjectFactory, Runtime, kind};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result};

impl<F: ObjectFactory> Runtime<F> {
    /// 显式推进一个 FES 对象的下层更新；平面创建的任务直属 owner。
    /// 调用方按 FES 更新顺序调用，并将对象存储与 owner 保持同一生命周期。
    pub fn update_fes_object(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        if self.tree.node(owner).is_none() {
            return Err(Error::new(0, "FES 所属任务不存在"));
        }
        let object = objects
            .get(handle)
            .ok_or_else(|| Error::new(0, "FES 对象标识无效"))?;
        let canvas = matches!(object.state, ObjectState::Plane(_));
        if matches!(object.state, ObjectState::CImage(_)) {
            return self.update_fes_cimage(owner, objects, handle);
        }
        if object.state.plane().is_none() {
            return match object.state {
                ObjectState::Base => Ok(()),
                _ => Err(Error::new(0, "FES 图像对象更新尚未接入共享绘制服务")),
            };
        }
        let creation_position = objects.plane_position(handle);
        let object = objects.get_mut(handle).unwrap();
        let state = object.state.plane_mut().unwrap();
        let mut task = object.task_id;
        self.check_fes_plane_binding(owner, task)?;
        if state.rebuild {
            if !canvas {
                if let Some(resource) = &state.size_resource {
                    state.size = self.fes_image_geometry(resource, true)?;
                }
            }
            if let Some(old) = task {
                self.fes_command(owner, Some(old), 3, [0; 3], None)?;
            }
            task = match self.create(owner, kind::PLANE, b"FE Plane", b"") {
                Ok(task) => Some(task),
                Err(error) => {
                    state.rebuild = false;
                    object.task_id = None;
                    return Err(error);
                }
            };
            object.task_id = task;
            self.fes_command(owner, task, 84, [0, state.creation_type, 0], None)?;
            if canvas {
                if let Some(resource) = &state.size_resource {
                    state.size = self.fes_image_geometry(resource, true)?;
                }
            }
            let mut rectangle = [0; 32];
            for (i, value) in [
                0,
                0,
                0,
                (state.size[0] as f32).to_bits(),
                (state.size[1] as f32).to_bits(),
                if canvas { 4 } else { 32 },
                1,
                if canvas { u32::MAX } else { state.color },
            ]
            .into_iter()
            .enumerate()
            {
                rectangle[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            self.fes_command(
                owner,
                task,
                if canvas { 41 } else { 42 },
                [0; 3],
                Some(&mut rectangle),
            )?;
            if canvas {
                let mut bytes = creation_position[..3]
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>();
                self.fes_command(owner, task, 51, [0, 0, 1], Some(&mut bytes))?;
            }
            state.rebuild = false;
        }
        let viewport = state.viewport.clone();
        if let Some(viewport) = objects.lookup(&viewport.name, viewport.bank) {
            let target_dirty = objects
                .get(viewport)
                .unwrap()
                .state
                .plane()
                .is_some_and(|state| state.dirty);
            let state = objects.get(handle).unwrap().state.plane().unwrap();
            if state.viewport_dirty || target_dirty {
                return Err(Error::new(0, "FES VIEWPORT 更新需要二维对象命令 89"));
            }
        }
        let state = objects.get_mut(handle).unwrap().state.plane_mut().unwrap();
        if !state.dirty {
            return Ok(());
        }
        if let Some(resource) = state.position_resource.take() {
            let offset = self.fes_image_geometry(&resource, false)?;
            let next = [offset[0] as f32, offset[1] as f32];
            if state.position[..2] != next {
                state.position[..2].copy_from_slice(&next);
                objects.invalidate(handle)?;
            }
        }
        let position = objects.plane_position(handle);
        let base = objects.plane_base_point(handle);
        let object = objects.get(handle).unwrap();
        let state = object.state.plane().unwrap();
        let mut vectors = vec![
            (51, 0, [position[0], position[1], position[2]]),
            (49, 0, [base[0], base[1], state.priority as f32]),
        ];
        if let ObjectState::Plane(plane) = &object.state {
            let vram = plane.vram();
            let offset = objects.canvas_offset;
            vectors.push((
                51,
                1,
                [
                    offset[0].wrapping_add(integer_position(vram[0])) as f32,
                    offset[1].wrapping_add(integer_position(vram[1])) as f32,
                    vram[2],
                ],
            ));
        }
        vectors.push((66, 0, [1.0; 3]));
        for (code, source, value) in vectors {
            let mut bytes = value.map(f32::to_le_bytes).concat();
            self.fes_command(owner, task, code, [source, 0, 1], Some(&mut bytes))?;
        }
        let mut matrix = crate::graphics::Transform::default()
            .matrix()
            .map(f32::to_le_bytes)
            .concat();
        self.fes_command(owner, task, 57, [0, 0, 1], Some(&mut matrix))?;
        for (code, data) in [
            (59, [1, state.color, 1]),
            (47, [0, state.display as u32, 1]),
            (81, [0, 0, 1]),
        ] {
            self.fes_command(owner, task, code, data, None)?;
        }
        objects
            .get_mut(handle)
            .unwrap()
            .state
            .plane_mut()
            .unwrap()
            .dirty = false;
        Ok(())
    }

    /// 先标记下层任务删除，再移出 FES 子树；任务实际释放仍发生在树更新中。
    pub fn remove_fes_object(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        if self.tree.node(owner).is_none() {
            return Err(Error::new(0, "FES 所属任务不存在"));
        }
        if handle == objects.top() {
            return Err(Error::new(0, "FES 根对象随整个对象存储释放"));
        }
        let mut pending = vec![handle];
        while let Some(id) = pending.pop() {
            let object = objects
                .get(id)
                .ok_or_else(|| Error::new(0, "FES 对象标识无效"))?;
            pending.extend(object.children());
            self.check_fes_plane_binding(owner, object.task_id)?;
            if object.task_id.is_some() {
                self.fes_command(owner, object.task_id, 3, [0; 3], None)?;
            }
            if let Some(image) = object.image {
                let list = self.check_fes_image_list(owner, objects)?;
                let slot = object
                    .state
                    .image()
                    .ok_or_else(|| Error::new(0, "FES 图像来源与对象类型不匹配"))?
                    .parent_priority;
                self.fes_command(owner, Some(list), 287, [slot as u32, 0, 0], None)?;
                self.images_mut().release(image)?;
                objects.get_mut(id).unwrap().image = None;
            }
        }
        objects.remove(handle)?;
        Ok(())
    }

    fn check_fes_plane_binding(&self, owner: u32, task: Option<u32>) -> Result<()> {
        if task.is_some_and(|task| {
            !self
                .tree
                .node(task)
                .is_some_and(|node| node.parent == Some(owner) && node.kind == kind::PLANE)
        }) {
            return Err(Error::new(0, "FES 下层二维任务绑定失效"));
        }
        Ok(())
    }

    pub(super) fn fes_command(
        &mut self,
        owner: u32,
        task: Option<u32>,
        code: u32,
        data: [u32; 3],
        bytes: Option<&mut [u8]>,
    ) -> Result<()> {
        if let Some(task) = task {
            self.dispatch_command_with_buffers(
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
            )?;
        }
        Ok(())
    }

    pub(super) fn fes_image_geometry(
        &mut self,
        reference: &ImageReference,
        size: bool,
    ) -> Result<[i32; 2]> {
        if reference.name.is_empty() {
            return Ok([0; 2]);
        }
        let services = self
            .resources
            .services
            .as_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置 FES 图像元数据服务"))?;
        let name = services.resolve(b"image", &reference.name)?;
        let Some(data) = services.read(&name, crate::resource::ResourceMode::LooseFirst)? else {
            return Ok([0; 2]);
        };
        let metadata = crate::hg3::Metadata::parse(&data)?;
        let Some(info) = metadata.frame(reference.frame).and_then(|frame| frame.info) else {
            return Ok([0; 2]);
        };
        Ok(if size {
            info.size.map(|v| v as i32)
        } else {
            info.offset
        })
    }
}

impl Objects {
    fn plane_position(&self, handle: u32) -> [f32; 4] {
        let mut chain = Vec::new();
        let mut next = Some(handle);
        while let Some(handle) = next {
            let object = self.get(handle).unwrap();
            let Some(state) = object.state.plane() else {
                break;
            };
            chain.push(state.position);
            next = object.parent;
        }
        chain.into_iter().rev().fold([0.0; 4], |parent, local| {
            std::array::from_fn(|i| local[i] + parent[i])
        })
    }

    fn plane_base_point(&self, handle: u32) -> [f32; 4] {
        let mut chain = Vec::new();
        let mut next = Some(handle);
        while let Some(handle) = next {
            let object = self.get(handle).unwrap();
            chain.push(object.state.plane().unwrap().base_point);
            next = object
                .parent
                .filter(|&parent| matches!(self.get(parent).unwrap().state, ObjectState::Plane(_)));
        }
        chain.into_iter().rev().fold([0.0; 4], |parent, local| {
            std::array::from_fn(|i| local[i] + parent[i])
        })
    }
}

fn integer_position(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
