use super::{ObjectFactory, Runtime};
use crate::{
    host::{
        audio::SpeechSynthesizer,
        effect::EffectServices,
        font::FontServices,
        graphics::GraphicsBackend,
        movie::MovieDecoderFactory,
        platform::{AchievementServices, PlatformJobServices, SystemServices},
        resources::{ArchiveConfig, CachedResources, ResourceServices},
        storage::{
            ConfigurationServices, ExportServices, FileServices, FolderPaths, SaveServices,
            ScriptSettings,
        },
    },
    settings::PlatformSettings,
};

impl<F: ObjectFactory> Runtime<F> {
    /// Configure synchronous resource lookup and reads. Already loaded objects
    /// remain available; future lookups use a new cache over this service.
    pub fn set_resources(&mut self, services: impl ResourceServices + 'static) {
        self.resources.services = Some(Box::new(CachedResources::new(services)));
        self.resources.animation_aliases = None;
    }

    /// Set the archive categories and names reported to scripts.
    pub fn set_archive_config(&mut self, config: ArchiveConfig) {
        self.host.archive_config = Some(config);
    }

    /// Supply the path values used by script folder queries and expansion.
    pub fn set_folder_paths(&mut self, paths: FolderPaths) {
        self.host.folder_paths = Some(paths);
    }

    /// Initialize the script settings with the host's persisted values.
    pub fn set_script_settings(&mut self, settings: ScriptSettings) {
        self.host.script_settings = settings;
    }

    /// Apply the host's startup and user platform settings to the session.
    pub fn set_platform_settings(&mut self, settings: PlatformSettings) {
        self.host.platform_settings = Some(settings);
    }

    pub fn platform_settings(&self) -> Option<&PlatformSettings> {
        self.host.platform_settings.as_ref()
    }

    /// Access the effective platform settings for host UI integration.
    pub fn platform_settings_mut(&mut self) -> Option<&mut PlatformSettings> {
        self.host.platform_settings.as_mut()
    }

    /// Install the renderer for script captures and active screen filter passes.
    /// A requested readback fails if no backend has been installed.
    pub fn set_readback_backend(&mut self, backend: impl GraphicsBackend + 'static) {
        self.resources.readback_backend = Some(Box::new(backend));
    }

    /// Select the media execution mode for objects created after this call.
    pub fn set_media_decode_mode(&mut self, mode: crate::host::media::DecodeMode) {
        self.resources.decode_mode = mode;
    }

    /// Set the decoder factory for future movie loads, including plane movies.
    pub fn set_movie_decoder_factory(&mut self, factory: impl MovieDecoderFactory + 'static) {
        self.resources.movie_decoder_factory = Some(std::sync::Arc::new(factory));
    }

    /// Install the service for subsequent FES character model selections.
    pub fn set_model_services(
        &mut self,
        services: impl crate::host::model::ModelServices + 'static,
    ) {
        self.resources.model_services = Some(Box::new(services));
    }

    pub fn set_system_services(&mut self, services: impl SystemServices + 'static) {
        self.host.system_services = Some(Box::new(services));
    }

    pub fn set_achievement_services(&mut self, services: impl AchievementServices + 'static) {
        self.host.set_achievement_services(services);
    }

    pub fn set_file_services(&mut self, services: impl FileServices + 'static) {
        self.host.file_services = Some(Box::new(services));
    }

    pub fn set_export_services(&mut self, services: impl ExportServices + 'static) {
        self.resources.exports = Some(Box::new(services));
    }

    pub fn set_platform_job_services(&mut self, services: impl PlatformJobServices + 'static) {
        self.resources.platform_jobs = Some(Box::new(services));
    }

    pub fn set_effect_services(&mut self, services: impl EffectServices + 'static) {
        self.resources.effects = Some(Box::new(services));
    }

    pub fn set_speech_synthesizer(&mut self, synthesizer: impl SpeechSynthesizer + 'static) {
        self.resources.speech = Some(Box::new(synthesizer));
    }

    pub fn set_configuration_services(&mut self, services: impl ConfigurationServices + 'static) {
        self.host.configuration_services = Some(Box::new(services));
    }

    /// Supply save reads, writes and timestamps. Loaded containers are retained.
    pub fn set_save_services(&mut self, services: impl SaveServices + 'static) {
        self.host.save_services = Some(Box::new(services));
    }

    pub fn set_fonts(&mut self, fonts: impl FontServices + 'static) {
        self.resources.fonts = Some(Box::new(fonts));
    }
}
