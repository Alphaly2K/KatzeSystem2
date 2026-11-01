//! FES 对象的身份、银行绑定与属性状态，独立于下层任务编号。

use crate::fes::{
    CImageState, CPlaneState, ImageState, ObjectDefinition, ObjectInitialization, ObjectReference,
};
use crate::{Error, HandleAllocator, Result, read};
use std::collections::{BTreeMap, BTreeSet};

mod plane;

const TOP: &str = "_TOPOBJ_";

/// 已接入的 FES 类型状态；下层绘图任务与资源装载由更新阶段维护。
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectState {
    Base,
    CImage(Box<CImageState>),
    Image(Box<ImageState>),
    CPlane(Box<CPlaneState>),
}

impl ObjectState {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Base => "BASE",
            Self::CImage(_) => "CIMAGE",
            Self::Image(_) => "IMAGE",
            Self::CPlane(_) => "CPLANE",
        }
    }

    fn new(kind: &str) -> Result<Self> {
        Ok(match kind {
            "BASE" => Self::Base,
            "CIMAGE" => Self::CImage(Box::default()),
            "IMAGE" => Self::Image(Box::default()),
            "CPLANE" => Self::CPlane(Box::default()),
            _ => return Err(Error::new(0, format!("FES 对象类型 {kind} 尚未接入"))),
        })
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

    /// CPLANE 更新所创建的下层任务；尚未创建或创建失败时为 None。
    pub fn task_id(&self) -> Option<u32> {
        self.task_id
    }
}

/// 一份 FES 的对象存储。绑定保留首次值，数量表与对象表分别维护。
pub struct Objects {
    handles: HandleAllocator,
    top: u32,
    objects: BTreeMap<u32, Object>,
    bindings: BTreeMap<String, BTreeMap<i32, u32>>,
    counts: BTreeMap<String, u32>,
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
                    state: ObjectState::Base,
                },
            )]),
            bindings: BTreeMap::from([(TOP.to_owned(), BTreeMap::from([(0, top)]))]),
            counts: BTreeMap::from([(TOP.to_owned(), 1)]),
        })
    }

    pub fn top(&self) -> u32 {
        self.top
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

    /// 创建请求中的全部银行，返回新对象的标识，包括未取得名称绑定的重复项。
    pub fn define(&mut self, definition: &ObjectDefinition) -> Result<Vec<u32>> {
        if definition.name.is_empty()
            || definition.name.len() > 31
            || !definition.name.is_ascii()
            || definition.name.as_bytes().contains(&0)
        {
            return Err(Error::new(0, "FES 对象名称需要 1～31 个非零 ASCII 字节"));
        }
        let count = definition.banks as usize;
        if count == 0 || count > read::MAX_ITEMS - self.objects.len() {
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
                ObjectState::CImage(state) => {
                    state.apply(&initialization.attributes, bank as u32);
                    false
                }
                ObjectState::Image(state) => {
                    state.apply(&initialization.attributes, bank as u32, &mut transform_name)?;
                    false
                }
                ObjectState::CPlane(state) => state.apply(&initialization.attributes, bank as u32),
            };
            if dirty {
                self.invalidate(handle);
            }
        }
        Ok(())
    }

    fn invalidate(&mut self, handle: u32) {
        let mut pending = vec![handle];
        while let Some(handle) = pending.pop() {
            let object = self.objects.get_mut(&handle).unwrap();
            if let ObjectState::CPlane(state) = &mut object.state {
                state.dirty = true;
            }
            pending.extend(object.children.iter().copied());
        }
    }

    /// 移出对象及后代，后代先返回；调用方负责释放其下层媒体资源和任务。
    /// 删除不修改名称数量表，也不会把重复声明的未绑定对象提升为绑定。
    pub fn remove(&mut self, handle: u32) -> Result<Vec<Object>> {
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
