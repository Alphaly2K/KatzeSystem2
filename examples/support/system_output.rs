use crate::support::local_date_time;
use katze_system2::{
    Result,
    kcs::{LocalDateTime, SystemServices},
};

pub struct Output;

impl SystemServices for Output {
    fn local_date_time(&mut self) -> Option<LocalDateTime> {
        Some(local_date_time())
    }

    fn message_box(&mut self, task_id: u32, text: &[u16]) -> Result<()> {
        if let Err(error) = sdl2::messagebox::show_simple_message_box(
            sdl2::messagebox::MessageBoxFlag::empty(),
            "system error",
            &String::from_utf16_lossy(text),
            None,
        ) {
            eprintln!("任务 {task_id} 的系统对话框失败：{error}");
        }
        Ok(())
    }
}
