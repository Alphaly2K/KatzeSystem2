use super::{
    DiagnosticTrace, ObjectFactory, ObjectHost, create_object, fes, route_command,
    route_command_mode,
};
use crate::host::{effect::EffectServices, platform::PlatformJobServices, storage::ExportServices};
use crate::kcs::{Host, Interpreter, NativeReturn, Program, TaskEffects};
use crate::task::{Command, CommandBuffers, TaskClock};
use crate::{Error, Result, read};

impl<F: ObjectFactory> Host for ObjectHost<'_, F> {
    fn diagnostic_trace(&self) -> Option<&DiagnosticTrace> {
        self.resources.diagnostic_trace.as_ref()
    }

    fn register_file_source(&mut self, source: crate::host::storage::SharedSource) -> Result<u32> {
        self.host.register_file_source(source)
    }
    fn update_particle(
        &mut self,
        task: &mut crate::kcs::ParticleTask,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        self.update_particle_task(task, now, budget)
    }

    fn language_selection(&self) -> [u32; 3] {
        self.host.languages.selection()
    }

    fn load_sequence_data(&mut self, handle: u32, bytes: Vec<u8>) -> Result<()> {
        self.host.load_sequence_data(handle, bytes)
    }
    fn create_database(&mut self, database: crate::kcs::Database) -> Result<u32> {
        self.host.create_database(database)
    }
    fn create_quest_table(&mut self, bytes: &[u8]) -> Result<u32> {
        self.host.create_quest_table(bytes)
    }
    fn set_task_shared_numbers(&mut self, available: bool) {
        if let Some(task) = self.running.get_mut(&self.host.task_id) {
            task.shared_numbers = Some(available);
            if available {
                if let Some(values) = &task.local_numbers {
                    self.numbers
                        .register_local(self.host.task_id, values.clone());
                }
            }
        }
    }
    fn set_task_local_numbers(&mut self, values: crate::expression::LocalNumbers) {
        if let Some(task) = self.running.get_mut(&self.host.task_id) {
            if task.shared_numbers != Some(true) {
                return;
            }
            task.local_numbers = Some(values.clone());
        }
        self.numbers.bind_local(self.host.task_id, values);
    }
    fn set_task_named_values(&mut self, values: crate::expression::NamedValues) {
        if self
            .running
            .get(&self.host.task_id)
            .is_some_and(|task| task.shared_numbers != Some(true))
        {
            return;
        }
        self.numbers.bind_named(self.host.task_id, values);
    }
    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<Program>> {
        self.resources.images.release_resources(self.host.task_id)?;
        self.resources
            .images
            .release_shared_image_owner(self.host.task_id)?;
        self.resources.images.release_owner(self.host.task_id);
        self.resources.raw.release_owner(self.host.task_id);
        if self.resources.resource_programs {
            self.resources.read_ordinary_program(name)
        } else {
            self.factory.load_kcs_program(name)
        }
    }

    fn fes_services(&mut self) -> Option<&mut dyn fes::Services> {
        Some(self)
    }

    fn random(&mut self) -> Option<&mut crate::kcs::ScriptRandom> {
        Some(&mut self.host.random)
    }

    fn surface_size(&self) -> [u32; 2] {
        self.host.surface_size
    }

    fn parent_task_id(&self) -> Result<u32> {
        self.tree
            .node(self.host.task_id)
            .and_then(|node| node.parent)
            .ok_or_else(|| Error::new(0, "当前任务没有可查询的父级"))
    }

    fn root_index_contains(&self, id: u32) -> Result<bool> {
        Ok(self.tree.root_index_contains(id))
    }

    fn handle_allocator(&self) -> Option<crate::HandleAllocator> {
        Some(self.host.handle_allocator())
    }

    fn image(&self, handle: u32) -> Result<&crate::graphics::Image> {
        self.resources.images.get(handle)
    }

    fn pixel_image(&self, pixels: u32) -> Result<&crate::graphics::Image> {
        self.resources.images.pixel_image(pixels)
    }

    fn image_data(&self, handle: u32) -> Result<&[u8]> {
        self.resources.raw.image_data(handle)
    }

    fn images_mut(&mut self) -> Option<&mut crate::graphics::Images> {
        Some(&mut self.resources.images)
    }

    fn fonts(&mut self) -> Option<&mut dyn crate::host::font::FontServices> {
        self.resources
            .fonts
            .as_mut()
            .map(|fonts| fonts.as_mut() as _)
    }

    fn frame_rate(&self) -> u32 {
        self.host
            .platform_settings
            .as_ref()
            .map_or(60, |s| s.window_fps)
    }

    fn message_time_scale(&self) -> Option<u32> {
        self.host.message_time_scale
    }

    fn unicode_text(&self) -> bool {
        self.host.unicode_text != 0
    }

    fn sound_stereo(&self) -> u32 {
        self.host.sound_stereo_value()
    }

    fn sound_config_enabled(&self) -> u32 {
        self.host.sound_config_enabled
    }

    fn jtalk_enabled(&self) -> u32 {
        self.host
            .system_settings
            .as_ref()
            .map_or(0, |settings| settings.jtalk_enabled)
    }

    fn sound_group_id(&self, name: &[u8]) -> Result<u32> {
        self.host.sound_group_id(name)
    }

    fn system_time_ms(&self) -> u32 {
        self.host.system_time_ms
    }

    fn effect_speed(&self) -> u32 {
        self.host.system_settings.as_ref().map_or(0, |settings| {
            u32::from_le_bytes(settings.effects[..4].try_into().unwrap())
        })
    }

    fn message_defaults(&self) -> Result<&crate::message::MessageDefaults> {
        self.host
            .message_defaults
            .as_ref()
            .ok_or_else(|| Error::new(0, "宿主未配置消息默认设置"))
    }

    fn message_plane_display(&mut self, id: u32) -> Result<Option<u32>> {
        let drawing = &self.resources.compositor.drawing;
        if drawing.mode != 0 || !drawing.enabled {
            return Ok(None);
        }
        Ok(self
            .resources
            .compositor
            .drawables
            .get(id)
            .map(crate::graphics::Drawable::display))
    }

    fn submit_message(
        &mut self,
        surface: &crate::graphics::Surface,
        rectangle: crate::graphics::Rect,
        at: [i32; 2],
    ) -> Result<bool> {
        Ok(self
            .resources
            .compositor
            .drawing
            .copy_surface(surface, rectangle, at))
    }

    fn submit_message_pixels(
        &mut self,
        pixels: u32,
        rectangle: crate::graphics::Rect,
        at: [i32; 2],
    ) -> Result<bool> {
        let image = self.resources.images.pixel_image(pixels)?;
        Ok(self
            .resources
            .compositor
            .drawing
            .copy_surface(image.surface(), rectangle, at))
    }

    fn update_image_list(
        &mut self,
        list: &mut crate::graphics::ImageList,
        submit: bool,
        force: bool,
        clip: Option<crate::graphics::Rect>,
    ) -> Result<bool> {
        self.resources.update_image_list(list, submit, force, clip)
    }

    fn update_image_list_animations(
        &mut self,
        list: &mut crate::graphics::ImageList,
        now: u32,
        budget: usize,
    ) -> Result<bool> {
        list.update_animations_with_random(now, budget, &mut self.resources.images, &mut || {
            self.host.random.next_integer()
        })
    }

    fn restart_image_list_animation(
        &mut self,
        list: &mut crate::graphics::ImageList,
        key: u32,
        budget: usize,
    ) -> Result<bool> {
        list.restart_animation_with_random(key, budget, &mut self.resources.images, &mut || {
            self.host.random.next_integer()
        })
    }

    fn create_task_child(&mut self, kind: u32, name: &[u8], context: &[u8]) -> Result<u32> {
        let parent = self.host.task_id;
        create_object(self.store(), parent, kind, name, context)
    }
    fn input_device(&mut self) -> Option<&mut dyn crate::input::InputDevice> {
        self.resources
            .input_device
            .as_mut()
            .map(|device| device.as_mut() as _)
    }

    fn input_coordinates(&self) -> crate::input::CoordinateMap {
        self.resources.input_coordinates
    }
    fn input_pointer_control(&self) -> bool {
        self.resources.input_pointer_control
    }
    fn set_image_cursor(&mut self, name: &[u8], hotspot: [u16; 2]) -> Result<()> {
        self.resources.load_image_cursor(name, hotspot)
    }
    fn canvas_layout(&self) -> Result<Option<crate::graphics::CanvasLayout>> {
        self.resources
            .compositor
            .drawing
            .target
            .as_ref()
            .map(|t| crate::graphics::CanvasLayout::new(t.tile_size(), t.grid(), t.tile_size()))
            .transpose()
    }

    fn drawables(&mut self) -> Option<&mut crate::graphics::Drawables> {
        Some(&mut self.resources.compositor.drawables)
    }

    fn compositor(&mut self) -> Option<&mut crate::graphics::Compositor> {
        Some(&mut self.resources.compositor)
    }

    fn reserve_auxiliary_targets(&mut self, count: u32) -> Result<bool> {
        let settings = self.host.platform_settings.as_ref();
        if settings.is_some_and(|settings| settings.d3d_backbuffer == 0) {
            return Ok(false);
        }
        self.resources.compositor.reserve_auxiliary_targets(
            self.host.surface_size,
            count,
            settings.map_or(u32::MAX, |settings| settings.d3d_tex_size),
        )?;
        Ok(count <= 2 && self.resources.compositor.auxiliary_targets.len() >= count as usize)
    }

    fn resource_services(&mut self) -> Option<&mut dyn crate::host::resources::ResourceServices> {
        match &mut self.resources.services {
            Some(services) => Some(services.as_mut()),
            None => None,
        }
    }

    fn quiz_debug_packs(&self, handle: u32) -> Result<Vec<Vec<u8>>> {
        self.resources.raw.quiz_debug_packs(handle)
    }

    fn export_services(&mut self) -> Option<&mut dyn ExportServices> {
        match &mut self.resources.exports {
            Some(services) => Some(services.as_mut()),
            None => None,
        }
    }

    fn platform_job_services(&mut self) -> Option<&mut dyn PlatformJobServices> {
        self.resources
            .platform_jobs
            .as_deref_mut()
            .map(|service| service as _)
    }

    fn effect_services(&mut self) -> Option<&mut dyn EffectServices> {
        match &mut self.resources.effects {
            Some(services) => Some(services.as_mut()),
            None => None,
        }
    }

    fn speech_synthesizer(&mut self) -> Option<&mut dyn crate::host::audio::SpeechSynthesizer> {
        self.resources
            .speech
            .as_mut()
            .map(|service| service.as_mut() as _)
    }

    fn movie_enabled(&self) -> bool {
        self.host
            .platform_settings
            .as_ref()
            .is_none_or(|settings| settings.etc_disablemovie == 0)
    }

    fn load_plane_movie(
        &mut self,
        owner: u32,
        source: crate::host::storage::SharedSource,
        effect: bool,
    ) -> Result<[u32; 2]> {
        self.create_plane_movie(owner, source, effect)
    }

    fn plane_movie_handle(&self, owner: u32, effect: bool) -> u32 {
        self.resources.plane_movie_handle(owner, effect)
    }

    fn release_plane_movies(&mut self, owner: u32) -> Result<()> {
        self.resources.release_plane_movies(self.objects, owner);
        Ok(())
    }

    fn next_task_child(&self, previous: Option<u32>) -> Result<Option<u32>> {
        Ok(self
            .tree
            .next_child(self.host.task_id, previous.map(|id| id as i32)))
    }

    fn dispatch_task_command(
        &mut self,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let origin = self.host.task_id;
        route_command(self.store(), origin, command, buffers, output)
    }

    fn dispatch_input_command(
        &mut self,
        state: &mut crate::input::CommandState,
        delivery: crate::input::InputDelivery,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let origin = self.host.task_id;
        if !self.running.contains_key(&origin)
            || self.resources.input_callbacks.contains_key(&origin)
        {
            return Err(Error::new(0, "输入回调上下文不可用"));
        }
        self.resources
            .input_callbacks
            .insert(origin, std::mem::take(state));
        let result = route_command_mode(
            self.store(),
            origin,
            command,
            buffers,
            output,
            delivery,
            None,
        );
        *state = self.resources.input_callbacks.remove(&origin).unwrap();
        result
    }

    fn dispatch_graphics_event(
        &mut self,
        response: &mut u32,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let origin = self.host.task_id;
        if !self.running.contains_key(&origin)
            || self.resources.graphics_responses.contains_key(&origin)
        {
            return Err(Error::new(0, "图形回调上下文不可用"));
        }
        self.resources.graphics_responses.insert(origin, *response);
        let result = route_command(self.store(), origin, command, buffers, output);
        *response = self.resources.graphics_responses.remove(&origin).unwrap();
        result
    }

    fn dispatch_button_event(
        &mut self,
        button: &mut crate::input::ButtonObject,
        command: Command,
        buffers: &mut CommandBuffers<'_>,
        output: &mut u32,
    ) -> Result<bool> {
        let origin = self.host.task_id;
        if !self.running.contains_key(&origin)
            || self.resources.button_callbacks.contains_key(&origin)
        {
            return Err(Error::new(0, "按钮回调上下文不可用"));
        }
        self.resources
            .button_callbacks
            .insert(origin, button.take_for_callback());
        let result = route_command(self.store(), origin, command, buffers, output);
        *button = self.resources.button_callbacks.remove(&origin).unwrap();
        result
    }

    fn take_task_effects(&mut self, queued_messages: usize) -> TaskEffects {
        let Some(task) = self.running.get_mut(&self.host.task_id) else {
            return TaskEffects::default();
        };
        let effects = std::mem::take(&mut task.effects);
        if let Some(remaining) = &mut task.remaining_messages {
            *remaining = read::MAX_ITEMS
                .saturating_sub(queued_messages.saturating_add(effects.messages.len()));
        }
        effects
    }

    fn set_task_clock(&mut self, clock: &TaskClock) {
        self.host.set_task_clock(clock);
        if let Some(task) = self.running.get_mut(&self.host.task_id) {
            task.clock = clock.clone();
            if let Some(paused) = task.paused_changed {
                task.clock.paused = paused;
            }
        }
    }

    fn set_task_id(&mut self, id: u32) {
        self.host.task_id = id;
    }

    fn set_task_context(&mut self, context: &[u8]) {
        self.context.clear();
        self.context.extend_from_slice(context);
        if let Some(task) = self.running.get_mut(&self.host.task_id) {
            task.context = Some(context.to_vec());
        }
    }

    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        self.call_registered_native(index, arguments, vm)
    }
}
