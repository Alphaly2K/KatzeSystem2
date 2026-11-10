use super::{SoundConfig, SoundPart, soundconf::MAX_DEPTH};
use crate::{
    Error, Result, read,
    resource::{XmlDocument, XmlWriter},
};

impl SoundConfig {
    /// 从分类定义构造当前配置，再应用用户设置；缺失字段使用定义中的默认值。
    /// 用户设置不创建分类，也不改变分类名称、编号、资源路由或顺序。
    /// None 保留定义初值；已提供的空文档仍应用用户字段的缺省规则。
    pub fn from_xml_with_user_settings(definition: &[u8], user: Option<&[u8]>) -> Result<Self> {
        let mut config = Self::from_xml(definition)?;
        let Some(user) = user else { return Ok(config) };
        let xml = XmlDocument::parse(user)?;
        let system_voice = config.master_system_voice.unwrap_or(-1);
        for (field, name) in [
            (&mut config.master_volume, "volume"),
            (&mut config.master_play_enabled, "play"),
            (&mut config.master_system_voice, "sysplay"),
            (&mut config.master_voice_volume, "voicevol"),
            (&mut config.master_skip_volume, "skipvol"),
        ] {
            *field = Some(xml.integer(
                format!("MASTER/{name}").as_bytes(),
                field.unwrap_or(-1) as u32,
            )? as i32);
        }
        config.master_mute = xml.integer(b"MASTER/mute", system_voice as u32)? as i32;
        config.master_voice_mute = xml.integer(b"MASTER/voicemute", system_voice as u32)? as i32;
        for part in &mut config.parts {
            apply_part(part, &xml, b"PART")?;
        }
        Ok(config)
    }

    /// 将当前主声音设置及分类设置写入用户文档，不写入分类定义和声音组范围。
    pub fn write_xml(&self, xml: &mut XmlWriter) -> Result<()> {
        write_values(
            xml,
            &mut vec!["MASTER"],
            [
                self.master_volume.unwrap_or(-1),
                self.master_play_enabled.unwrap_or(-1),
                self.master_system_voice.unwrap_or(-1),
                self.master_voice_volume.unwrap_or(-1),
                self.master_skip_volume.unwrap_or(-1),
                self.master_mute,
                self.master_voice_mute,
            ],
        )?;
        let mut visited = 0;
        let mut path = vec!["PART"];
        for part in &self.parts {
            write_part(part, xml, &mut path, 0, &mut visited)?;
        }
        Ok(())
    }
}

const FIELDS: [&str; 7] = [
    "volume",
    "play",
    "sysplay",
    "voicevol",
    "skipvol",
    "mute",
    "voicemute",
];

fn apply_part(part: &mut SoundPart, xml: &XmlDocument, parent: &[u8]) -> Result<()> {
    let mut path = parent.to_vec();
    path.push(b'/');
    path.extend_from_slice(&part.xml_key);
    let base = path.len();
    let system_voice = part.system_voice;
    for (field, name) in [
        (&mut part.volume, "volume"),
        (&mut part.play_enabled, "play"),
        (&mut part.system_voice, "sysplay"),
        (&mut part.voice_volume, "voicevol"),
        (&mut part.skip_volume, "skipvol"),
        (&mut part.mute, "mute"),
        (&mut part.voice_mute, "voicemute"),
    ] {
        path.truncate(base);
        path.push(b'/');
        path.extend_from_slice(name.as_bytes());
        let fallback = if matches!(name, "mute" | "voicemute") {
            system_voice
        } else {
            *field
        };
        *field = xml.integer(&path, fallback as u32)? as i32;
    }
    path.truncate(base);
    path.extend_from_slice(b"/part");
    for child in &mut part.children {
        apply_part(child, xml, &path)?;
    }
    Ok(())
}

fn write_part<'a>(
    part: &'a SoundPart,
    xml: &mut XmlWriter,
    path: &mut Vec<&'a str>,
    depth: usize,
    visited: &mut usize,
) -> Result<()> {
    *visited += 1;
    if depth >= MAX_DEPTH || *visited > read::MAX_ITEMS {
        return Err(Error::new(0, "声音分类树超过写出上限"));
    }
    let key = std::str::from_utf8(&part.xml_key)
        .map_err(|_| Error::new(0, "声音分类 XML 键名需要 UTF-8 文本"))?;
    path.push(key);
    write_values(
        xml,
        path,
        [
            part.volume,
            part.play_enabled,
            part.system_voice,
            part.voice_volume,
            part.skip_volume,
            part.mute,
            part.voice_mute,
        ],
    )?;
    path.push("part");
    for child in &part.children {
        write_part(child, xml, path, depth + 1, visited)?;
    }
    path.pop();
    path.pop();
    Ok(())
}

fn write_values(xml: &mut XmlWriter, path: &mut Vec<&str>, values: [i32; 7]) -> Result<()> {
    for (name, value) in FIELDS.into_iter().zip(values) {
        path.push(name);
        xml.write_integer(path, value as u32)?;
        path.pop();
    }
    Ok(())
}
