use super::{ObjectState, Objects, slider::ControlKind};
use crate::{
    Error, Result,
    graphics::Rect,
    runtime::{ObjectFactory, Store, create_object, kind, route_with_buffers},
    task::{Command, CommandBuffers},
};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn prepare_fes_object(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        index: usize,
    ) -> Result<()> {
        self.restore_fes_draw_submission(owner, objects)?;
        let record = objects
            .update_order
            .get(index)
            .ok_or_else(|| Error::new(0, "FES 媒体记录编号无效"))?;
        if record.prepared {
            return Ok(());
        }
        let Some(handle) = objects.media_handle(record) else {
            return Ok(());
        };
        if matches!(objects.get(handle).unwrap().state, ObjectState::Draw(_)) {
            self.prepare_fes_draw(owner, objects, handle)?;
            let object = objects.get(handle).unwrap();
            let (task, image) = (object.task_id, object.image);
            objects.update_order[index].task_id = task;
            objects.update_order[index].image = image;
            objects.update_order[index].prepared = true;
            return Ok(());
        }
        let object = objects.get(handle).unwrap();
        let button = matches!(object.state, ObjectState::Button(_));
        let control = ControlKind::of(&object.state);
        let string = match &object.state {
            ObjectState::String(state) => Some(state.task_kind()),
            _ => None,
        };
        if object.state.image().is_some()
            && !matches!(object.state, ObjectState::DifImage(_))
            && record.image.is_none()
        {
            let wrapper = self.resources.images.create_empty(owner)?;
            let object = objects.get_mut(handle).unwrap();
            if let Some(reference) = object.reference_image {
                self.resources.images.release_reference(reference)?;
                object.reference_image = None;
            }
            object.image = Some(wrapper);
            object.images.push(wrapper);
            objects.update_order[index].image = Some(wrapper);
        }
        if (button || control.is_some() || string.is_some())
            && objects.update_order[index].task_id.is_none()
        {
            let (kind, name) = if button {
                (kind::BUTTON, b"FE Button".as_slice())
            } else if let Some(kind) = string {
                (
                    kind,
                    if kind == kind::STRING_PLANE_UTF8 {
                        b"FE string UTF8".as_slice()
                    } else {
                        b"FE string".as_slice()
                    },
                )
            } else {
                let protocol = control.unwrap();
                (protocol.task_kind(), protocol.task_name())
            };
            let task = create_object(self.reborrow(), owner, kind, name, b"")?;
            let object = objects.get_mut(handle).unwrap();
            object.task_id = Some(task);
            object.input_tasks.push(task);
            objects.update_order[index].task_id = Some(task);
        }
        objects.update_order[index].prepared = true;
        Ok(())
    }

    pub(super) fn check_fes_button_binding(&self, owner: u32, task: Option<u32>) -> Result<()> {
        if task.is_some_and(|task| {
            !self
                .tree
                .node(task)
                .is_some_and(|node| node.parent == Some(owner) && node.kind == kind::BUTTON)
        }) {
            return Err(Error::new(0, "FES 下层按钮任务绑定失效"));
        }
        Ok(())
    }

    pub(super) fn update_fes_button(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        image: Option<u32>,
    ) -> Result<()> {
        let object = objects.get(handle).unwrap();
        self.check_fes_button_binding(owner, object.task_id)?;
        let task = object
            .task_id
            .ok_or_else(|| Error::new(0, "FES BUTTON 尚未完成装载准备"))?;
        if object.button_update.is_none() {
            let flags = object.state.image().unwrap().updates.clone();
            objects.get_mut(handle).unwrap().button_update = Some(flags);
        }
        let wrapper = match image {
            Some(image) => image,
            None => self.fes_image_wrapper(owner, objects, handle)?,
        };
        self.update_fes_image(owner, objects, handle, Some(wrapper))?;
        let flags = objects.get(handle).unwrap().button_update.clone().unwrap();
        if !flags.needs_rebuild() && !flags.dirty && !flags.priority_changed {
            objects.get_mut(handle).unwrap().button_update = None;
            return Ok(());
        }
        let input = match objects.input_task {
            Some(task) => task,
            None => {
                let task = self.button_command(owner, 0, 34, [0; 3], None)?;
                objects.input_task = Some(task);
                task
            }
        };
        if !self
            .tree
            .node(input)
            .is_some_and(|node| node.kind == kind::INPUT)
        {
            return Err(Error::new(0, "FES 输入任务绑定失效"));
        }
        if flags.needs_rebuild() || flags.dirty {
            let rectangle = self.fes_button_rectangle(objects, handle, wrapper)?;
            let ObjectState::Button(button) = &objects.get(handle).unwrap().state else {
                unreachable!()
            };
            let priority = button.image.base.priority.wrapping_add(100) as u32;
            let enabled = button.enabled as u32;
            let mask = button.mask_type != 0;
            let locked = button.position_lock != 0;
            let mut bytes = [
                rectangle.left,
                rectangle.top,
                rectangle.right,
                rectangle.bottom,
            ]
            .map(i32::to_le_bytes)
            .concat();
            if flags.needs_rebuild() {
                let mut registration = bytes.clone();
                registration.extend(priority.to_le_bytes());
                registration.extend(0u32.to_le_bytes());
                if std::env::var_os("KATZE_TRACE_NATIVE").is_some() {
                    let name = objects
                        .get(handle)
                        .map(|object| format!("{}[{}]", object.name(), object.bank()))
                        .unwrap_or_default();
                    eprintln!(
                        "[fes-button] owner={} handle={} name={} task={} rect={:?}",
                        owner, handle, name, task, rectangle
                    );
                }
                if self.button_command(owner, task, 90, [0; 3], Some(&mut registration))? == 0
                    && !locked
                {
                    self.button_command(owner, input, 130, [task, 0, 0], Some(&mut bytes))?;
                }
            } else {
                self.button_command(owner, task, 101, [0; 3], Some(&mut bytes))?;
            }
            self.button_command(owner, task, 93, [0, enabled, 0], None)?;
            if flags.needs_rebuild() && mask && self.resources.images.pixel_handle(wrapper)? != 0 {
                self.button_command(owner, task, 98, [0, wrapper, 0], None)?;
            }
            let updates = &mut objects
                .get_mut(handle)
                .unwrap()
                .state
                .image_mut()
                .unwrap()
                .updates;
            updates.rebuild = false;
            updates.source_rebuild = false;
            updates.dirty = false;
        }
        if !flags.needs_rebuild() && flags.priority_changed {
            let priority = objects
                .get(handle)
                .unwrap()
                .state
                .image()
                .unwrap()
                .priority
                .wrapping_add(100);
            self.button_command(owner, input, 129, [task, priority as u32, 0], None)?;
        }
        objects.get_mut(handle).unwrap().button_update = None;
        Ok(())
    }

    fn fes_button_rectangle(
        &mut self,
        objects: &mut Objects,
        handle: u32,
        wrapper: u32,
    ) -> Result<Rect> {
        let cache = objects.preloaded_images.clone();
        let reference = objects
            .get(handle)
            .unwrap()
            .state
            .image()
            .unwrap()
            .size_resource
            .clone();
        if let Some(reference) = reference {
            let size = self.fes_image_geometry(&cache, &reference, true)?;
            objects
                .get_mut(handle)
                .unwrap()
                .state
                .image_mut()
                .unwrap()
                .set_size(size);
        }
        let state = objects.get(handle).unwrap().state.image().unwrap();
        let mut size = state.size;
        if std::env::var_os("KATZE_TRACE_NATIVE").is_some() {
            eprintln!(
                "[btnrect] handle={handle} size={:?} resource={:?} wrapper={wrapper}",
                size,
                state.size_resource.as_ref().map(|r| {
                    String::from_utf8_lossy(&r.name).into_owned() + ":" + &r.frame.to_string()
                })
            );
        }
        if size.iter().any(|&axis| axis < 0) && self.resources.images.pixel_handle(wrapper)? != 0 {
            size = self
                .resources
                .images
                .get(wrapper)?
                .info()
                .size
                .map(|value| value as i32);
        }
        let mut position = objects.button_position(handle).map(integer_position);
        if let Some(plane) = objects.lookup(&state.plane.name, state.plane.bank) {
            if matches!(objects.get(plane).unwrap().state, ObjectState::Plane(_)) {
                let offset = objects.plane_position(plane).map(integer_position);
                let base = objects.plane_base_point(plane).map(integer_position);
                for i in 0..2 {
                    position[i] = position[i].wrapping_add(offset[i]).wrapping_sub(base[i]);
                }
            }
        }
        Ok(Rect::from_xywh(position[0], position[1], size[0], size[1]))
    }

    fn button_command(
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
                external: None,
            },
        )
    }
}

impl Objects {
    pub(super) fn lock_button_position(&mut self, handle: u32, value: i32) {
        let capture = matches!(&self.get(handle).unwrap().state,
            ObjectState::Button(button) if button.position_lock == 0 && value != 0);
        let position = capture.then(|| self.image_position(handle));
        if let ObjectState::Button(button) = &mut self.get_mut(handle).unwrap().state {
            if let Some(position) = position {
                button.locked_position = position;
            }
            button.position_lock = value;
        }
    }

    pub(super) fn button_position(&self, handle: u32) -> [f32; 2] {
        let object = self.get(handle).unwrap();
        if let ObjectState::Button(button) = &object.state {
            if button.position_lock != 0 {
                let parent = object.parent.filter(|&parent| {
                    matches!(
                        self.get(parent).unwrap().state,
                        ObjectState::CImage(_) | ObjectState::Image(_)
                    )
                });
                let parent = parent.map_or([0.; 2], |parent| self.image_position(parent));
                // 捕获值已经包含原父级位置，查询仍累加当前父级位置。
                return std::array::from_fn(|i| button.locked_position[i] + parent[i]);
            }
        }
        self.image_position(handle)
    }
}

fn integer_position(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}
