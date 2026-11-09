use super::{Drawables, Projection};
use crate::kcs::{Host, Interpreter};
use crate::runtime::{BaseObject, TaskObject};
use crate::task::{Command, CommandBuffers, CommandOutcome, Control, TaskClock};
use crate::{Error, Result, read};

/// 绘图坐标组的二维偏移、缩放和矩阵。
#[derive(Clone, Debug, PartialEq)]
pub struct Camera2D {
    pub offset: [f32; 2],
    pub scale: [f32; 2],
    pub matrix: [f32; 16],
}

impl Default for Camera2D {
    fn default() -> Self {
        Self {
            offset: [0.; 2],
            scale: [1.; 2],
            matrix: Projection::default().coordinate_matrix,
        }
    }
}

impl Camera2D {
    /// 替换已选择的坐标组，保留投影中心、深度及最终画布变换。
    pub fn apply_projection(&self, projection: &mut Projection) {
        projection.coordinate_offset = self.offset;
        projection.coordinate_scale = self.scale;
        projection.coordinate_matrix = self.matrix;
    }
}

impl Drawables {
    /// 相机编号独立递增；零保留给调用者提供的默认投影坐标组。
    pub fn create_camera(&mut self) -> Result<u32> {
        if self.cameras.len() == read::MAX_ITEMS {
            return Err(Error::new(0, "二维相机数量超过上限"));
        }
        let id = self.next_camera.max(1);
        self.next_camera = id
            .checked_add(1)
            .ok_or_else(|| Error::new(0, "二维相机编号耗尽"))?;
        self.cameras.insert(id, Camera2D::default());
        Ok(id)
    }

    pub fn camera(&self, id: u32) -> Option<&Camera2D> {
        self.cameras.get(&id)
    }

    pub fn camera_mut(&mut self, id: u32) -> Option<&mut Camera2D> {
        self.cameras.get_mut(&id)
    }

    pub fn release_camera(&mut self, id: u32) -> bool {
        self.cameras.remove(&id).is_some()
    }
}

/// 类型 65 的二维相机任务；相机记录在实际释放时回收。
pub struct Camera2DObject {
    base: BaseObject,
    camera: Option<u32>,
}

impl Default for Camera2DObject {
    fn default() -> Self {
        Self::new()
    }
}

impl Camera2DObject {
    pub fn new() -> Self {
        Self {
            base: BaseObject::new(),
            camera: None,
        }
    }

    pub fn camera_id(&self) -> Option<u32> {
        self.camera
    }
}

impl TaskObject for Camera2DObject {
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

    fn set_creation_context(&mut self, context: &[u8]) -> Result<()> {
        if context.is_empty() {
            Ok(())
        } else {
            Err(Error::new(0, "二维相机创建上下文尚未接入"))
        }
    }

    fn register_graphics(&mut self, drawables: &mut Drawables) -> Result<()> {
        if self.camera.is_none() {
            self.camera = Some(drawables.create_camera()?);
        }
        Ok(())
    }

    fn update_body(&mut self, host: &mut dyn Host, increment: u32, budget: usize) -> Result<bool> {
        self.base.update_body(host, increment, budget)
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
        if command.code == 589 {
            *output = self.camera.unwrap_or(0);
            return Ok(CommandOutcome::finished(false));
        }
        if matches!(command.code, 51 | 57 | 66 | 590) {
            let vector = if matches!(command.code, 51 | 66) {
                Some(values::<2>(buffers.data1.as_deref())?)
            } else {
                None
            };
            let drawings = host
                .drawables()
                .ok_or_else(|| Error::new(0, "宿主未提供绘图对象服务"))?;
            if let Some(camera) = self.camera.and_then(|id| drawings.camera_mut(id)) {
                if std::env::var_os("KATZE_TRACE_NATIVE").is_some() && command.code == 51 {
                    eprintln!("[camera] id={:?} offset={:?}", self.camera, vector);
                }
                match command.code {
                    51 => camera.offset = vector.unwrap(),
                    57 => camera.matrix = values::<16>(buffers.data1.as_deref())?,
                    66 => camera.scale = vector.unwrap(),
                    590 => *camera = Camera2D::default(),
                    _ => unreachable!(),
                }
            }
            return Ok(CommandOutcome::finished(false));
        }
        self.base
            .dispatch_command(kind, command, output, host, budget)
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
        self.base.request_delete(host, budget)
    }

    fn prepare_release(&mut self, host: &mut dyn Host, _budget: usize) -> Result<bool> {
        if let Some(camera) = self.camera {
            host.drawables()
                .ok_or_else(|| Error::new(0, "宿主未提供绘图对象服务"))?
                .release_camera(camera);
            self.camera = None;
        }
        Ok(true)
    }
}

fn values<const N: usize>(bytes: Option<&[u8]>) -> Result<[f32; N]> {
    let bytes = bytes
        .filter(|bytes| bytes.len() >= N * 4)
        .ok_or_else(|| Error::new(0, "二维相机输入缓冲区缺失或过短"))?;
    Ok(std::array::from_fn(|i| {
        f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
