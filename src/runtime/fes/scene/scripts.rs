use super::{Notification, Scene};
use crate::runtime::{
    ObjectFactory, Store, create_object, create_unloaded_ordinary, kind, route_command,
};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result};

#[derive(Default)]
pub(super) struct ChildScripts {
    pub(super) external: crate::memory::SharedMemory,
    pub(super) fes: Option<u32>,
    pub(super) kcs: Option<u32>,
    pub(super) active: Option<ScriptRequest>,
}

pub(super) enum ScriptRequest {
    SetBackground {
        source: Option<BackgroundSource>,
        image: u32,
        phase: usize,
    },
    StartFes {
        name: Vec<u8>,
        parameters: [u32; 16],
        child: Option<u32>,
        resolved: Vec<u8>,
        phase: usize,
    },
    StartKcs {
        name: Vec<u8>,
        child: Option<u32>,
        resolved: Vec<u8>,
        phase: usize,
    },
    EndFes,
    EndKcs,
}

pub(super) enum BackgroundSource {
    Image(u32),
    Plane(u32),
}

pub(super) enum ScriptDisposition {
    Continue,
    Flush,
    NextTick,
}

impl Scene {
    pub(super) fn script_command(
        &mut self,
        args: &[Vec<u8>],
        offset: usize,
        numbers: &mut crate::expression::State,
    ) -> Result<Option<ScriptDisposition>> {
        match args.first().map(Vec::as_slice) {
            Some(b"baseimg") => {
                let source = match args.get(1).filter(|name| !name.is_empty()) {
                    None => None,
                    Some(name) => {
                        let target = self.interpreter.object_reference(name, numbers)?;
                        let Some(handle) = self.objects.lookup(&target.name, target.bank) else {
                            return Ok(Some(ScriptDisposition::Continue));
                        };
                        let object = self.objects.get(handle).unwrap();
                        match &object.state {
                            super::super::ObjectState::CImage(_)
                            | super::super::ObjectState::Image(_)
                            | super::super::ObjectState::Button(_) => {
                                Some(BackgroundSource::Image(object.image.unwrap_or(0)))
                            }
                            super::super::ObjectState::Plane(_) => {
                                Some(BackgroundSource::Plane(handle))
                            }
                            _ => return Ok(Some(ScriptDisposition::Flush)),
                        }
                    }
                };
                self.notify(Notification::Script(ScriptRequest::SetBackground {
                    source,
                    image: 0,
                    phase: 0,
                }))?;
                Ok(Some(ScriptDisposition::Flush))
            }
            Some(b"endfes" | b"endkcs") => {
                self.notify(Notification::Script(if args[0] == b"endfes" {
                    ScriptRequest::EndFes
                } else {
                    ScriptRequest::EndKcs
                }))?;
                self.set_system_value("_EXITCODE_", -1);
                Ok(Some(ScriptDisposition::NextTick))
            }
            Some(b"execfes" | b"execkcs") => {
                let Some(name) = args.get(1).filter(|name| !name.is_empty()) else {
                    return Ok(Some(ScriptDisposition::Continue));
                };
                if name.len() >= 260 {
                    return Err(Error::new(offset, "FES 子脚本名称超过容量"));
                }
                if args[0] == b"execkcs" {
                    self.notify(Notification::Script(ScriptRequest::StartKcs {
                        name: name.clone(),
                        child: None,
                        resolved: Vec::new(),
                        phase: 0,
                    }))?;
                    return Ok(Some(ScriptDisposition::Flush));
                }
                let mut parameters = [0; 16];
                for (value, bytes) in parameters.iter_mut().zip(args.iter().skip(2)) {
                    *value = self.interpreter.eval(bytes, numbers)? as u32;
                }
                self.notify(Notification::Script(ScriptRequest::StartFes {
                    name: name.clone(),
                    parameters,
                    child: None,
                    resolved: Vec::new(),
                    phase: 0,
                }))?;
                Ok(Some(ScriptDisposition::Flush))
            }
            Some(b"react") => {
                let bank = self
                    .interpreter
                    .event_context()
                    .map_or(0, |context| context.bank);
                self.request_react(bank);
                Ok(Some(ScriptDisposition::Continue))
            }
            _ => Ok(None),
        }
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn process_fes_script_requests(
        &mut self,
        owner: u32,
        scene: &mut Scene,
    ) -> Result<bool> {
        for _ in 0..self.budget.max(1) {
            if scene.scripts.active.is_none() {
                scene.publish_notifications();
                let Some(Notification::Script(_)) = scene.notifications.front() else {
                    return Ok(true);
                };
                let Some(Notification::Script(request)) = scene.notifications.pop_front() else {
                    unreachable!();
                };
                scene.scripts.active = Some(request);
            }
            let mut request = scene.scripts.active.take().unwrap();
            match self.advance_fes_script_request(owner, scene, &mut request) {
                Ok(true) => (),
                Ok(false) => scene.scripts.active = Some(request),
                Err(error) => {
                    scene.scripts.active = Some(request);
                    return Err(error);
                }
            }
        }
        if scene.scripts.active.is_none() {
            scene.publish_notifications();
        }
        Ok(scene.scripts.active.is_none()
            && !matches!(scene.notifications.front(), Some(Notification::Script(_))))
    }

    fn advance_fes_script_request(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        request: &mut ScriptRequest,
    ) -> Result<bool> {
        match request {
            ScriptRequest::SetBackground {
                source,
                image,
                phase,
            } => self.advance_fes_background(owner, &scene.objects, source, image, phase),
            ScriptRequest::EndFes | ScriptRequest::EndKcs => {
                let slot = if matches!(request, ScriptRequest::EndFes) {
                    &mut scene.scripts.fes
                } else {
                    &mut scene.scripts.kcs
                };
                self.fes_command(owner, *slot, 3, [0; 3], None)?;
                *slot = None;
                Ok(true)
            }
            ScriptRequest::StartKcs {
                name,
                child,
                resolved,
                phase,
            } => {
                match *phase {
                    0 => {
                        if scene.scripts.fes.is_some() || scene.scripts.kcs.is_some() {
                            self.host
                                .debug_messages
                                .push(b"FES child script already active".to_vec());
                            return Ok(true);
                        }
                        let context = self
                            .objects
                            .get(&owner)
                            .and_then(|object| object.creation_context().map(<[u8]>::to_vec))
                            .or_else(|| {
                                self.running
                                    .get(&owner)
                                    .and_then(|task| task.context.clone())
                            })
                            .unwrap_or_default();
                        *child = Some(create_unloaded_ordinary(self.reborrow(), owner, &context)?);
                    }
                    1 => {
                        *resolved = self
                            .resources
                            .services
                            .as_mut()
                            .ok_or_else(|| Error::new(0, "运行时未配置 KCS 资源服务"))?
                            .resolve(b"kcs", name)?;
                        resolved.truncate(
                            resolved
                                .iter()
                                .position(|&byte| byte == 0)
                                .unwrap_or(resolved.len()),
                        );
                        resolved.push(0);
                    }
                    2 => self.fes_command(owner, *child, 161, [1, 0, 0], Some(resolved))?,
                    3 => {
                        if let Some(child) = *child {
                            route_command(
                                self.reborrow(),
                                owner,
                                Command {
                                    target: child,
                                    sender: owner,
                                    code: 172,
                                    data: [scene.scripts.external.len() as u32, 0, 0],
                                },
                                &mut CommandBuffers {
                                    external: Some(scene.scripts.external.clone()),
                                    ..CommandBuffers::default()
                                },
                                &mut 0,
                            )?;
                        }
                    }
                    4 => {
                        scene.scripts.kcs = *child;
                        return Ok(true);
                    }
                    _ => unreachable!(),
                }
                *phase += 1;
                Ok(false)
            }
            ScriptRequest::StartFes {
                name,
                parameters,
                child,
                resolved,
                phase,
            } => {
                match *phase {
                    0 => {
                        if scene.scripts.fes.is_some() || scene.scripts.kcs.is_some() {
                            self.host
                                .debug_messages
                                .push(b"FES child script already active".to_vec());
                            return Ok(true);
                        }
                        let context = self
                            .objects
                            .get(&owner)
                            .and_then(|object| object.creation_context().map(<[u8]>::to_vec))
                            .or_else(|| {
                                self.running
                                    .get(&owner)
                                    .and_then(|task| task.context.clone())
                            })
                            .unwrap_or_default();
                        *child = Some(create_object(
                            self.reborrow(),
                            owner,
                            kind::FES,
                            b"FE Script",
                            &context,
                        )?);
                    }
                    1 => {
                        *resolved = self
                            .resources
                            .services
                            .as_mut()
                            .ok_or_else(|| Error::new(0, "运行时未配置 FES 资源服务"))?
                            .resolve(b"fes", name)?;
                        resolved.truncate(
                            resolved
                                .iter()
                                .position(|&byte| byte == 0)
                                .unwrap_or(resolved.len()),
                        );
                        resolved.push(0);
                    }
                    2 => self.fes_command(owner, *child, 337, [0; 3], None)?,
                    3 => self.fes_command(
                        owner,
                        *child,
                        340,
                        [0, scene.objects.image_list.unwrap_or(0), 0],
                        None,
                    )?,
                    4..=19 => self.fes_command(
                        owner,
                        *child,
                        346,
                        [(*phase - 4) as u32, parameters[*phase - 4], 0],
                        None,
                    )?,
                    20 => self.fes_command(owner, *child, 338, [0; 3], Some(resolved))?,
                    21 => self.fes_command(owner, *child, 339, [0; 3], None)?,
                    22 => {
                        scene.scripts.fes = *child;
                        return Ok(true);
                    }
                    _ => unreachable!(),
                }
                *phase += 1;
                Ok(false)
            }
        }
    }

    pub(super) fn poll_fes_child_status(&mut self, owner: u32, scene: &mut Scene) -> Result<()> {
        let Some(child) = scene.scripts.fes.or(scene.scripts.kcs) else {
            return Ok(());
        };
        let mut output = 0xcccc_cccc;
        route_command(
            self.reborrow(),
            owner,
            Command {
                target: child,
                sender: owner,
                code: 23,
                data: [0; 3],
            },
            &mut CommandBuffers::default(),
            &mut output,
        )?;
        if output != u32::MAX {
            scene.set_system_value("_EXITCODE_", output as i32);
        }
        Ok(())
    }
}
