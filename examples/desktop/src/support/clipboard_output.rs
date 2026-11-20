use katze_system2::host::platform::{ClipboardRequest, ClipboardText};

#[derive(Default)]
pub struct Output {
    video: Option<sdl2::VideoSubsystem>,
}

impl Output {
    pub fn pump(
        &mut self,
        requests: Vec<ClipboardRequest>,
        encoding: &'static encoding_rs::Encoding,
    ) {
        if requests.is_empty() {
            return;
        }
        if self.video.is_none() {
            match sdl2::init().and_then(|context| context.video()) {
                Ok(video) => self.video = Some(video),
                Err(error) => {
                    eprintln!("剪贴板服务初始化失败：{error}");
                    return;
                }
            }
        }
        let clipboard = self.video.as_ref().unwrap().clipboard();
        for request in requests {
            let text = match request.text {
                ClipboardText::Bytes(bytes) => encoding.decode(&bytes).0.into_owned(),
                ClipboardText::Utf16(units) => String::from_utf16_lossy(&units),
            };
            if let Err(error) = clipboard.set_clipboard_text(&text) {
                eprintln!("任务 {} 的剪贴板写入失败：{error}", request.task_id);
            }
        }
    }
}
