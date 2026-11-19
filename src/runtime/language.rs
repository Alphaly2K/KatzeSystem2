use super::{ObjectFactory, Runtime};
use crate::{
    Error, Result,
    language::{LanguageTarget, StringTables},
    resource::ResourceMode,
};

impl<F: ObjectFactory> Runtime<F> {
    /// The current translation tables for host UI strings.
    pub fn string_tables(&self) -> &StringTables {
        &self.host.string_tables
    }

    /// Load the initial strings after startup configuration and resource setup.
    /// When no interface prefix is selected, load the game table alone.
    pub fn load_startup_language_tables(&mut self) -> Result<()> {
        let target = if self.interface_language_available() {
            LanguageTarget::Interface
        } else {
            LanguageTarget::MainText
        };
        self.reload_language_tables(target)
    }

    fn interface_language_available(&self) -> bool {
        self.host
            .string_table_config
            .as_ref()
            .is_some_and(|config| {
                (self.host.languages.primary_index as usize) < config.prefixes.len()
            })
    }

    /// Reload the selected tables without consuming queued language changes.
    pub fn reload_language_tables(&mut self, target: LanguageTarget) -> Result<()> {
        if target == LanguageTarget::Subtitle {
            return Ok(());
        }
        let config = self
            .host
            .string_table_config
            .as_ref()
            .ok_or_else(|| Error::new(0, "运行时未配置字符串表路径"))?;
        config.reload(
            target,
            &self.host.languages,
            &mut self.host.string_tables,
            |path| {
                let services = self
                    .resources
                    .services
                    .as_deref_mut()
                    .ok_or_else(|| Error::new(0, "运行时未配置字符串表资源服务"))?;
                if let Some(bytes) = services.read_utf16(path, ResourceMode::File)? {
                    return Ok(Some(bytes));
                }
                services.read_utf16(path, ResourceMode::LooseFirst)
            },
        )
    }

    /// Apply one language change using the current language selection.
    /// The queue is left intact for callers that manage language requests themselves.
    pub fn refresh_language(&mut self, target: LanguageTarget) -> Result<()> {
        let count = self.host.languages.prefixes.len();
        if target == LanguageTarget::Interface
            && self.host.languages.primary_index as usize >= count
        {
            return Ok(());
        }
        self.reload_language_tables(target)?;
        let mark_changed = match target {
            LanguageTarget::Interface => {
                self.interface_language_refreshed |= self.interface_language_available();
                true
            }
            LanguageTarget::MainText => (self.host.languages.message_index as usize) < count,
            LanguageTarget::Subtitle => {
                if self.host.languages.subtitle_index as usize >= count {
                    self.host.languages.subtitle_index = u32::MAX;
                }
                true
            }
        };
        if mark_changed {
            self.host.system_settings_dirty = 1;
        }
        Ok(())
    }

    pub(super) fn apply_pending_language_changes(&mut self) -> Result<()> {
        let mut requests = std::mem::take(&mut self.host.language_change_requests).into_iter();
        while let Some(request) = requests.next() {
            if let Err(error) = self.refresh_language(request.target) {
                self.host
                    .language_change_requests
                    .extend(std::iter::once(request).chain(requests));
                return Err(error);
            }
        }
        Ok(())
    }
}
