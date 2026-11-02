//! FES 对象的身份、银行绑定与属性状态，独立于下层任务编号。

use crate::fes::{
    ButtonState, CImageState, CPlaneState, ImageState, ObjectDefinition, ObjectInitialization,
    ObjectReference, PlaneState, Scroll2State, SliderState,
};
use crate::{Error, HandleAllocator, Result, read};
use std::collections::{BTreeMap, BTreeSet};

mod animation;
mod button;
mod commands;
mod image;
mod input;
mod motion;
mod natives;
mod navigation;
mod plane;
mod scene;
mod slider;
mod task;
pub use animation::ImageAnimationRequest;
pub use navigation::NavigationOutcome;
pub use scene::Scene;
pub(super) use task::dispatch_borrowed_control_event;
pub use task::{FesFactory, FesObject, Services};

const TOP: &str = "_TOPOBJ_";

/// 按声明及银行顺序保存的媒体绑定；名称查找仍使用当前对象表。
#[derive(Debug)]
pub struct MediaRecord {
    kind: &'static str,
    target: ObjectReference,
    image: Option<u32>,
    task_id: Option<u32>,
    prepared: bool,
}

impl MediaRecord {
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn target(&self) -> &ObjectReference {
        &self.target
    }
    pub fn image_handle(&self) -> Option<u32> {
        self.image
    }
    /// 装载记录创建的 BUTTON、SLIDER 或 SCROLL2 编号，其他类型返回 None。
    pub fn task_id(&self) -> Option<u32> {
        self.task_id
    }
}

/// 已接入的 FES 类型状态；下层绘图任务与资源装载由更新阶段维护。
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectState {
    Base,
    CImage(Box<CImageState>),
    Image(Box<ImageState>),
    Button(Box<ButtonState>),
    CPlane(Box<CPlaneState>),
    Plane(Box<PlaneState>),
    Slider(Box<SliderState>),
    Scroll2(Box<Scroll2State>),
}

impl ObjectState {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Base => "BASE",
            Self::CImage(_) => "CIMAGE",
            Self::Image(_) => "IMAGE",
            Self::Button(_) => "BUTTON",
            Self::CPlane(_) => "CPLANE",
            Self::Plane(_) => "PLANE",
            Self::Slider(_) => "SLIDER",
            Self::Scroll2(_) => "SCROLL2",
        }
    }

    fn new(kind: &str) -> Result<Self> {
        Ok(match kind {
            "BASE" => Self::Base,
            "CIMAGE" => Self::CImage(Box::default()),
            "IMAGE" => Self::Image(Box::default()),
            "BUTTON" => Self::Button(Box::default()),
            "CPLANE" => Self::CPlane(Box::default()),
            "PLANE" => Self::Plane(Box::default()),
            "SLIDER" => Self::Slider(Box::default()),
            "SCROLL2" => Self::Scroll2(Box::default()),
            _ => return Err(Error::new(0, format!("FES 对象类型 {kind} 尚未接入"))),
        })
    }

    fn plane(&self) -> Option<&CPlaneState> {
        match self {
            Self::CPlane(state) => Some(state),
            Self::Plane(state) => Some(&state.base),
            _ => None,
        }
    }

    fn plane_mut(&mut self) -> Option<&mut CPlaneState> {
        match self {
            Self::CPlane(state) => Some(state),
            Self::Plane(state) => Some(&mut state.base),
            _ => None,
        }
    }

    fn image(&self) -> Option<&CImageState> {
        match self {
            Self::CImage(state) => Some(state),
            Self::Image(state) => Some(&state.base),
            Self::Button(state) => Some(&state.image.base),
            _ => None,
        }
    }

    fn image_mut(&mut self) -> Option<&mut CImageState> {
        match self {
            Self::CImage(state) => Some(state),
            Self::Image(state) => Some(&mut state.base),
            Self::Button(state) => Some(&mut state.image.base),
            _ => None,
        }
    }

    fn image_source(&self) -> Option<&ImageState> {
        match self {
            Self::Image(state) => Some(state),
            Self::Button(state) => Some(&state.image),
            _ => None,
        }
    }

    fn image_source_mut(&mut self) -> Option<&mut ImageState> {
        match self {
            Self::Image(state) => Some(state),
            Self::Button(state) => Some(&mut state.image),
            _ => None,
        }
    }

    fn control(&self) -> Option<&SliderState> {
        match self {
            Self::Slider(state) => Some(state),
            Self::Scroll2(state) => Some(&state.control),
            _ => None,
        }
    }

    fn control_mut(&mut self) -> Option<&mut SliderState> {
        match self {
            Self::Slider(state) => Some(state),
            Self::Scroll2(state) => Some(&mut state.control),
            _ => None,
        }
    }
}

/// 一个 FES 对象。handle 是不透明标识，不是任务编号；重复声明也有独立身份。
#[derive(Debug)]
pub struct Object {
    handle: u32,
    name: String,
    bank: u32,
    parent: Option<u32>,
    children: BTreeSet<u32>,
    task_id: Option<u32>,
    image: Option<u32>,
    images: Vec<u32>,
    input_tasks: Vec<u32>,
    raw_image: u32,
    image_offset: [f32; 2],
    animation: ImageAnimationRequest,
    button_update: Option<crate::fes::ImageUpdates>,
    motions: crate::kcs::MotionQueue,
    motion_delay: u32,
    pub state: ObjectState,
}

impl Object {
    pub fn handle(&self) -> u32 {
        self.handle
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn bank(&self) -> u32 {
        self.bank
    }

    pub fn parent(&self) -> Option<u32> {
        self.parent
    }

    pub fn children(&self) -> impl Iterator<Item = u32> + '_ {
        self.children.iter().copied()
    }

    /// 当前下层任务；重复声明覆盖此编号，各记录的编号另行保留。
    pub fn task_id(&self) -> Option<u32> {
        self.task_id
    }

    /// 当前来源包装；重复声明指向最后准备的包装，尚未装填时没有像素。
    pub fn image_handle(&self) -> Option<u32> {
        self.image
    }

    /// 本对象持有的全部包装，包含重复声明记录的独立包装。
    pub fn image_handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.images.iter().copied()
    }

    /// 装载记录所创建的全部按钮；当前编号另由 task_id 查询。
    pub fn button_task_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.input_tasks
            .iter()
            .copied()
            .filter(|_| matches!(self.state, ObjectState::Button(_)))
    }

    /// 装载记录所创建的全部滑块任务。
    pub fn slider_task_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.input_tasks
            .iter()
            .copied()
            .filter(|_| matches!(self.state, ObjectState::Slider(_)))
    }

    /// 装载记录所创建的全部 SCROLL2 任务。
    pub fn scroll2_task_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.input_tasks
            .iter()
            .copied()
            .filter(|_| matches!(self.state, ObjectState::Scroll2(_)))
    }

    /// IMAGE 独立持有的原始图像数据；零表示没有来源，查询不增加持有。
    pub fn raw_image_handle(&self) -> u32 {
        self.raw_image
    }

    /// IMAGE 保存的请求；列表动画的实时播放与显示状态由共享列表维护。
    pub fn image_animation_request(&self) -> Option<&ImageAnimationRequest> {
        self.state.image_source().map(|_| &self.animation)
    }

    pub fn motions(&self) -> &crate::kcs::MotionQueue {
        &self.motions
    }

    pub fn motion_delay(&self) -> u32 {
        self.motion_delay
    }
}

/// 一份 FES 的对象存储。绑定保留首次值，数量表与对象表分别维护。
pub struct Objects {
    handles: HandleAllocator,
    top: u32,
    objects: BTreeMap<u32, Object>,
    bindings: BTreeMap<String, BTreeMap<i32, u32>>,
    counts: BTreeMap<String, u32>,
    update_order: Vec<MediaRecord>,
    canvas_offset: [i32; 2],
    image_list: Option<u32>,
    input_task: Option<u32>,
    image_clip: crate::graphics::Rect,
    motion_update: Option<motion::Traversal>,
}

impl Objects {
    /// 与运行时图像、数据等存储共用同一编号源；构造时登记根 BASE 对象。
    pub fn new(handles: HandleAllocator) -> Result<Self> {
        let top = handles.allocate()?;
        Ok(Self {
            handles,
            top,
            objects: BTreeMap::from([(
                top,
                Object {
                    handle: top,
                    name: TOP.to_owned(),
                    bank: 0,
                    parent: None,
                    children: BTreeSet::new(),
                    task_id: None,
                    image: None,
                    images: Vec::new(),
                    input_tasks: Vec::new(),
                    raw_image: 0,
                    image_offset: [0.; 2],
                    animation: ImageAnimationRequest::default(),
                    button_update: None,
                    motions: crate::kcs::MotionQueue::default(),
                    motion_delay: 0,
                    state: ObjectState::Base,
                },
            )]),
            bindings: BTreeMap::from([(TOP.to_owned(), BTreeMap::from([(0, top)]))]),
            counts: BTreeMap::from([(TOP.to_owned(), 1)]),
            update_order: Vec::new(),
            canvas_offset: [0; 2],
            image_list: None,
            input_task: None,
            image_clip: crate::graphics::Rect::default(),
            motion_update: None,
        })
    }

    pub fn top(&self) -> u32 {
        self.top
    }

    pub fn canvas_offset(&self) -> [i32; 2] {
        self.canvas_offset
    }

    /// 保存共享绘制服务提供的画布偏移；在后续 PLANE 属性更新时应用。
    pub fn set_canvas_offset(&mut self, offset: [i32; 2]) {
        self.canvas_offset = offset;
    }

    /// 绑定直属所属任务的类型 15 共享列表及全局图像裁剪。
    /// 不触发对象失效；调用方在图像更新前完成列表创建和绑定。
    pub fn set_image_list(&mut self, task: u32, clip: crate::graphics::Rect) {
        self.image_list = Some(task);
        self.image_clip = clip;
    }

    pub fn image_list(&self) -> Option<u32> {
        self.image_list
    }

    pub fn set_input_task(&mut self, task: u32) {
        self.input_task = Some(task);
    }

    pub fn input_task(&self) -> Option<u32> {
        self.input_task
    }

    /// 包含根对象及重复声明产生的未绑定对象。
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn get(&self, handle: u32) -> Option<&Object> {
        self.objects.get(&handle)
    }

    pub fn get_mut(&mut self, handle: u32) -> Option<&mut Object> {
        self.objects.get_mut(&handle)
    }

    /// 精确银行查找；−1 不在此处展开数组，缺失绑定返回 None。
    pub fn lookup(&self, name: &str, bank: i32) -> Option<u32> {
        self.bindings.get(name)?.get(&bank).copied()
    }

    /// 首次登记的数量；后续扩展或删除具体银行不改写此值。
    pub fn count(&self, name: &str) -> u32 {
        self.counts.get(name).copied().unwrap_or(0)
    }

    /// 按声明和银行顺序解析当前绑定，保留重复记录并跳过已删除的银行。
    pub fn update_order(&self) -> impl Iterator<Item = u32> + '_ {
        self.update_order
            .iter()
            .filter_map(|record| self.lookup(&record.target.name, record.target.bank))
    }

    pub fn media_records(&self) -> &[MediaRecord] {
        &self.update_order
    }

    /// 创建请求中的全部银行，返回新对象的标识，包括未取得名称绑定的重复项。
    pub fn define(&mut self, definition: &ObjectDefinition) -> Result<Vec<u32>> {
        self.require_motion_idle()?;
        if definition.name.is_empty()
            || definition.name.len() > 31
            || !definition.name.is_ascii()
            || definition.name.as_bytes().contains(&0)
        {
            return Err(Error::new(0, "FES 对象名称需要 1～31 个非零 ASCII 字节"));
        }
        let count = definition.banks as usize;
        if count == 0
            || count > read::MAX_ITEMS - self.objects.len()
            || count > read::MAX_ITEMS - self.update_order.len()
        {
            return Err(Error::new(0, "FES 对象数量超出上限"));
        }
        let initial = ObjectState::new(definition.kind)?;
        let handles: Vec<_> = (0..count)
            .map(|_| self.handles.allocate())
            .collect::<Result<_>>()?;
        let bindings = self.bindings.entry(definition.name.clone()).or_default();
        for (bank, &handle) in handles.iter().enumerate() {
            self.objects.insert(
                handle,
                Object {
                    handle,
                    name: definition.name.clone(),
                    bank: bank as u32,
                    parent: Some(self.top),
                    children: BTreeSet::new(),
                    task_id: None,
                    image: None,
                    images: Vec::new(),
                    input_tasks: Vec::new(),
                    raw_image: 0,
                    image_offset: [0.; 2],
                    animation: ImageAnimationRequest::default(),
                    button_update: None,
                    motions: crate::kcs::MotionQueue::default(),
                    motion_delay: 0,
                    state: initial.clone(),
                },
            );
            bindings.entry(bank as i32).or_insert(handle);
        }
        self.objects
            .get_mut(&self.top)
            .unwrap()
            .children
            .extend(handles.iter().copied());
        self.counts
            .entry(definition.name.clone())
            .or_insert(definition.banks);
        self.update_order
            .extend((0..definition.banks).map(|bank| MediaRecord {
                kind: definition.kind,
                target: ObjectReference {
                    name: definition.name.clone(),
                    bank: bank as i32,
                },
                image: None,
                task_id: None,
                prepared: false,
            }));
        Ok(handles)
    }

    /// 同步对象命名值及数组数量；保留同一解释器的其他系统命名值。
    pub fn publish_bindings(&self, interpreter: &mut crate::fes::Interpreter) {
        for (name, &count) in &self.counts {
            interpreter.set_object_bank_count(name, count);
            interpreter.named.insert(
                name.clone(),
                self.bindings
                    .get(name)
                    .into_iter()
                    .flat_map(|banks| banks.iter().map(|(&bank, &handle)| (bank, handle as i32)))
                    .collect(),
            );
        }
    }

    /// 修改 FES 父子关系，不改变任务树；None 解除父级。循环和根重挂返回诊断。
    pub fn set_parent(&mut self, handle: u32, parent: Option<u32>) -> Result<()> {
        self.require_motion_idle()?;
        let object = self
            .get(handle)
            .ok_or_else(|| Error::new(0, "FES 对象标识无效"))?;
        if handle == self.top {
            return Err(Error::new(0, "FES 根对象不能重挂父级"));
        }
        let old = object.parent;
        let mut ancestor = parent;
        while let Some(id) = ancestor {
            if id == handle {
                return Err(Error::new(0, "FES 对象父级构成循环"));
            }
            ancestor = self
                .get(id)
                .ok_or_else(|| Error::new(0, "FES 父对象标识无效"))?
                .parent;
        }
        if let Some(old) = old {
            self.objects.get_mut(&old).unwrap().children.remove(&handle);
        }
        if let Some(parent) = parent {
            self.objects
                .get_mut(&parent)
                .unwrap()
                .children
                .insert(handle);
        }
        self.objects.get_mut(&handle).unwrap().parent = parent;
        Ok(())
    }

    /// 先应用 PARENT，再按各银行的实际类型应用属性；错误保留此前已完成的修改。
    pub fn initialize(
        &mut self,
        initialization: &ObjectInitialization,
        mut transform_name: impl FnMut(&[u8]) -> Result<Vec<u8>>,
    ) -> Result<()> {
        self.require_motion_idle()?;
        let ObjectReference { name, bank } = &initialization.target;
        let banks = if *bank == -1 {
            0..i64::from(self.count(name))
        } else {
            i64::from(*bank)..i64::from(*bank) + 1
        };
        for bank in banks {
            let bank = bank as i32;
            let handle = self
                .lookup(name, bank)
                .ok_or_else(|| Error::new(0, format!("FES 对象 {name}[{bank}] 不存在")))?;
            if let Some(parent) = &initialization.attributes.parent {
                self.set_parent(handle, self.lookup(&parent.name, parent.bank))?;
            }
            let dirty = match &mut self.objects.get_mut(&handle).unwrap().state {
                ObjectState::Base => false,
                ObjectState::CImage(state) => state.apply(&initialization.attributes, bank as u32),
                ObjectState::Image(state) => {
                    state.apply(&initialization.attributes, bank as u32, &mut transform_name)?
                }
                ObjectState::Button(state) => {
                    state.apply(&initialization.attributes, bank as u32, &mut transform_name)?
                }
                ObjectState::CPlane(state) => state.apply(&initialization.attributes, bank as u32),
                ObjectState::Plane(state) => state.apply(&initialization.attributes, bank as u32),
                ObjectState::Slider(state) => {
                    state.apply(&initialization.attributes, bank as u32)?
                }
                ObjectState::Scroll2(state) => state
                    .control
                    .apply(&initialization.attributes, bank as u32)?,
            };
            if dirty {
                self.invalidate(handle)?;
            }
        }
        Ok(())
    }

    /// 显示属性直接修改或运动回写后，标记对象及后代等待重新登记。
    pub fn invalidate(&mut self, handle: u32) -> Result<()> {
        if !self.objects.contains_key(&handle) {
            return Err(Error::new(0, "FES 对象标识无效"));
        }
        let mut pending = vec![handle];
        while let Some(handle) = pending.pop() {
            let object = self.objects.get_mut(&handle).unwrap();
            if let Some(state) = object.state.plane_mut() {
                state.dirty = true;
            }
            if let Some(state) = object.state.image_mut() {
                state.updates.invalidate();
            }
            if let Some(state) = object.state.control_mut() {
                state.updates.dirty = true;
            }
            pending.extend(object.children.iter().copied());
        }
        Ok(())
    }

    /// 移出对象及后代，后代先返回；调用方负责释放其下层媒体资源和任务。
    /// 删除不修改名称数量表，也不会把重复声明的未绑定对象提升为绑定。
    pub fn remove(&mut self, handle: u32) -> Result<Vec<Object>> {
        self.require_motion_idle()?;
        if handle == self.top {
            return Err(Error::new(0, "FES 根对象随整个对象存储释放"));
        }
        if !self.objects.contains_key(&handle) {
            return Err(Error::new(0, "FES 对象标识无效"));
        }
        let mut removed = Vec::new();
        let mut stack = vec![(handle, false)];
        while let Some((handle, visited)) = stack.pop() {
            if !visited {
                stack.push((handle, true));
                stack.extend(
                    self.objects[&handle]
                        .children
                        .iter()
                        .rev()
                        .map(|&id| (id, false)),
                );
                continue;
            }
            let object = self.objects.remove(&handle).unwrap();
            for record in &mut self.update_order {
                if record
                    .image
                    .is_some_and(|image| object.images.contains(&image))
                    || record
                        .task_id
                        .is_some_and(|task| object.input_tasks.contains(&task))
                {
                    record.image = None;
                    record.task_id = None;
                    record.prepared = false;
                }
            }
            if let Some(parent) = object.parent {
                self.objects
                    .get_mut(&parent)
                    .unwrap()
                    .children
                    .remove(&handle);
            }
            let bindings = self.bindings.get_mut(&object.name).unwrap();
            if bindings.get(&(object.bank as i32)) == Some(&handle) {
                bindings.remove(&(object.bank as i32));
            }
            removed.push(object);
        }
        Ok(removed)
    }
}
