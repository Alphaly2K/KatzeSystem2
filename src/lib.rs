//! CatSystem2 2.6.1.90 的脚本读取与执行核心。

pub mod anm;
pub mod cst;
pub mod cstl;
pub mod expression;
pub mod fes;
pub mod hg3;
pub mod kcs;
mod read;
pub mod resource;
pub mod runtime;
pub mod task;
mod text;

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// offset 的单位由脚本类型约定：字节位置或记录／指令索引，详见接口文档。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl Error {
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "字节 {}：{}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

/// Yield 表示本次推进已让出，调用者可按各解释器的调度契约继续推进。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step<E> {
    Event(E),
    Yield,
    /// KCS 普通恢复被执行模式暂停，本次调用已完成。
    Paused,
    Halted,
}
