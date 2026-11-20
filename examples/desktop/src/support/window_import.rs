use super::{
    support::image_import::{ImportPicker, ImportRequest, is_image_path},
    window_text::FilePathDialog,
};
use katze_system2::session::GameRuntime;
use sdl2::{
    event::Event,
    messagebox::{self, ButtonData, ClickedButton, MessageBoxButtonFlag, MessageBoxFlag},
    video::Window,
};
use std::path::PathBuf;

pub struct WindowImport {
    directory: PathBuf,
    picker: ImportPicker,
    dialog: Option<(ImportRequest, FilePathDialog)>,
}

impl WindowImport {
    pub fn new(directory: PathBuf, picker: ImportPicker) -> Self {
        Self {
            directory,
            picker,
            dialog: None,
        }
    }

    pub fn active(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn event(&mut self, event: &Event) {
        let Some((request, dialog)) = &mut self.dialog else {
            return;
        };
        if !request.active() {
            self.dialog = None;
            return;
        }
        let Some(path) = dialog.event(event) else {
            return;
        };
        let choice = if let Some(path) = path {
            if !is_image_path(&path) || !path.is_file() {
                let _ = messagebox::show_simple_message_box(
                    MessageBoxFlag::WARNING,
                    "Import Image",
                    "Select an existing BMP, PNG, or JPG file.",
                    dialog.window(),
                );
                return;
            }
            let Ok(path) = std::path::absolute(path) else {
                return;
            };
            let destination = self.directory.join(path.file_name().unwrap());
            if destination.is_file() {
                confirm_overwrite(dialog.window(), &destination).then_some((path, true))
            } else {
                Some((path, false))
            }
        } else {
            None
        };
        self.dialog.take().unwrap().0.complete(choice);
    }

    pub fn sync(&mut self, parent: &Window, runtime: &mut GameRuntime, text_active: bool) {
        if self
            .dialog
            .as_ref()
            .is_some_and(|(request, _)| !request.active())
        {
            self.dialog = None;
        }
        if let Some(last) = self.picker.take_last_directory() {
            runtime
                .set_image_import_directory(Some(last.to_string_lossy().encode_utf16().collect()));
        }
        if text_active || self.dialog.is_some() {
            return;
        }
        if let Some(request) = self.picker.next_request() {
            let initial = runtime
                .image_import_directory()
                .map(String::from_utf16_lossy)
                .unwrap_or_default();
            match FilePathDialog::new(parent, &initial) {
                Ok(dialog) => self.dialog = Some((request, dialog)),
                Err(error) => {
                    eprintln!("Image import dialog: {error}");
                    request.complete(None);
                }
            }
        }
    }

    pub fn draw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some((_, dialog)) = &mut self.dialog {
            dialog.draw()?;
        }
        Ok(())
    }
}

fn confirm_overwrite(parent: &Window, destination: &std::path::Path) -> bool {
    let buttons = [
        ButtonData {
            flags: MessageBoxButtonFlag::empty(),
            button_id: 1,
            text: "Replace",
        },
        ButtonData {
            flags: MessageBoxButtonFlag::RETURNKEY_DEFAULT
                | MessageBoxButtonFlag::ESCAPEKEY_DEFAULT,
            button_id: 0,
            text: "Cancel",
        },
    ];
    let name = destination.file_name().unwrap().to_string_lossy();
    matches!(messagebox::show_message_box(
        MessageBoxFlag::WARNING,
        &buttons,
        "Replace Image?",
        &format!("An image named \"{name}\" already exists."),
        parent,
        None,
    ), Ok(ClickedButton::CustomButton(button)) if button.button_id == 1)
}
