use super::HostState;
use crate::{
    Error, Result,
    host::{ConfigurationFile, DeviceBinding},
    input::DeviceKeys,
    resource::XmlWriter,
    settings::write_configuration_with_bindings,
};

pub(crate) fn save_configuration(
    host: &mut HostState,
    keys: Option<&[DeviceBinding; 80]>,
    pc: usize,
) -> Result<()> {
    let services = host
        .configuration_services
        .as_mut()
        .ok_or_else(|| Error::new(pc, "宿主未配置用户配置写入服务"))?;
    let system = host
        .system_settings
        .as_ref()
        .ok_or_else(|| Error::new(pc, "宿主未配置全局设置"))?;
    let platform = host
        .platform_settings
        .as_ref()
        .ok_or_else(|| Error::new(pc, "宿主未配置平台设置"))?;
    let keys = keys
        .or_else(|| host.configuration_keys.as_ref().map(DeviceKeys::bindings))
        .ok_or_else(|| Error::new(pc, "宿主未提供按键配置"))?;
    let mut xml = XmlWriter::new();
    write_configuration_with_bindings(&mut xml, system, platform, &host.languages, keys)?;
    services.write(host.task_id, ConfigurationFile::Main, &xml.to_bytes()?)?;
    if host.sound_config_enabled != 0 {
        let sound = host
            .sound_config
            .as_ref()
            .ok_or_else(|| Error::new(pc, "宿主未提供新声音分类配置"))?;
        let mut xml = XmlWriter::new();
        sound.write_xml(&mut xml)?;
        services.write(host.task_id, ConfigurationFile::Sound, &xml.to_bytes()?)?;
    }
    Ok(())
}
