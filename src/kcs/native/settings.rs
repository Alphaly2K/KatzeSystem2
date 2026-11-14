use super::{words, write_string};
use crate::kcs::{HostState, Interpreter, NativeReturn, strings};
use crate::{Error, Result, read};

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    if matches!(index, 605..=607 | 844) {
        let name = strings::format(vm, read::u32_at(arguments, 0)?, 0)?;
        let settings = host
            .system_settings
            .as_ref()
            .ok_or_else(|| Error::new(vm.pc, "宿主未配置全局设置"))?;
        let value = match index {
            605 => settings.legacy_file_volume(&name, host.sound_groups.as_ref())?,
            606 => settings.voice_file_play_enabled(&name)?,
            607 => settings.voice_file_volume(&name)?,
            _ => settings
                .voice_file_index(&name)?
                .map_or(u32::MAX, |index| index as u32),
        };
        return Ok(NativeReturn::Value(value));
    }
    if index == 852 {
        let settings = host
            .system_settings
            .as_ref()
            .ok_or_else(|| Error::new(vm.pc, "宿主未配置全局设置"))?;
        return Ok(NativeReturn::Value(settings.backlog_static));
    }
    if index == 1036 {
        if arguments.len() != 8 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let addresses = [read::u32_at(arguments, 0)?, read::u32_at(arguments, 4)?];
        for address in addresses {
            vm.read(address, 1)?;
        }
        for (mode, address) in addresses.into_iter().enumerate() {
            let prefix = host.languages.prefix(mode as u32).unwrap_or_default();
            if prefix.len() >= 4096 {
                return Err(Error::new(vm.pc, "语言前缀超过转换缓冲区上限"));
            }
            write_string(vm, address, prefix, 1)?;
        }
        return Ok(NativeReturn::Continue);
    }
    if index == 1062 {
        if arguments.len() != 8 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let mode = u32::from_le_bytes(arguments[..4].try_into().unwrap());
        let address = u32::from_le_bytes(arguments[4..].try_into().unwrap());
        if mode <= 1 {
            let bytes = host.languages.expand(&vm.read_string(address)?, mode)?;
            write_string(vm, address, &bytes, 1)?;
        }
        return Ok(NativeReturn::Continue);
    }
    if matches!(index, 939..=964 | 995 | 1000 | 1001) {
        let name = if index >= 943 {
            strings::format(vm, read::u32_at(arguments, 0)?, 0)?
        } else {
            Vec::new()
        };
        let config = host
            .sound_config
            .as_ref()
            .ok_or_else(|| Error::new(vm.pc, "宿主未提供新声音分类配置"))?;
        let value = match index {
            939 => config
                .master_volume
                .ok_or_else(|| Error::new(vm.pc, "宿主未提供声音主音量"))?,
            940 => config
                .master_play_enabled
                .ok_or_else(|| Error::new(vm.pc, "宿主未提供声音主播放开关"))?,
            941 => config
                .master_system_voice
                .ok_or_else(|| Error::new(vm.pc, "宿主未提供声音主系统语音开关"))?,
            942 => config.master_mute(),
            964 => config.effective_file_volume(&name)?,
            995 => config.effective_file_voice_volume(&name)?,
            1000 => config.file_part(&name)?.map_or(0, |part| part.same_id),
            1001 => config.file_part(&name)?.map_or(-1, |part| part.id),
            _ => {
                let mode = (index - 943) / 7;
                let field = (index - 943) % 7;
                let part = match mode {
                    0 => config.file_part(&name)?,
                    1 => config.type_part(&name)?,
                    _ => config.group_part(&name)?,
                };
                match field {
                    0 => part.map_or(100, |p| p.volume),
                    1 => part.map_or(i32::from(mode != 1), |p| p.play_enabled),
                    2 => part.map_or(i32::from(mode != 1), |p| p.system_voice),
                    3 => part.map_or(100, |p| p.master_volume),
                    4 => part.map_or(100, |p| p.voice_volume),
                    5 => part.map_or(-1, |p| p.skip_volume),
                    _ => part.map_or(if mode == 2 { 0 } else { 100 }, |p| {
                        if p.mute == -1 { 0 } else { p.mute }
                    }),
                }
            }
        };
        return Ok(NativeReturn::Value(value as u32));
    }
    if matches!(index, 359 | 360) {
        let count = if index == 359 { 4 } else { 3 };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        for (index, address) in arguments.chunks_exact(4).enumerate() {
            let address = u32::from_le_bytes(address.try_into().unwrap());
            // 原地址解析器对区外起点返回空指针；地址零仍指向普通区首字节。
            if vm.read(address, 1).is_err() {
                continue;
            }
            let block = host
                .system_settings
                .as_mut()
                .ok_or_else(|| Error::new(vm.pc, "宿主未配置全局设置"))?
                .block_mut(index);
            if count == 4 {
                vm.write(address, block)?;
            } else {
                block.copy_from_slice(&vm.read(address, block.len())?);
            }
        }
        return Ok(NativeReturn::Continue);
    }
    if index == 361 {
        if !arguments.is_empty() {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        crate::kcs::save_configuration(host, None, vm.pc)?;
        return Ok(NativeReturn::Continue);
    }
    if matches!(index, 565 | 566) {
        return Ok(NativeReturn::Value(if index == 565 {
            host.frame_capture_enabled
        } else {
            host.frame_capture_index
        }));
    }
    if index == 938 {
        return Ok(NativeReturn::Value(host.sound_config_enabled));
    }
    if matches!(index, 741 | 999 | 1050) {
        let settings = host
            .system_settings
            .as_ref()
            .ok_or_else(|| Error::new(vm.pc, "宿主未配置全局设置"))?;
        return Ok(NativeReturn::Value(if index == 999 {
            settings.auto_select
        } else if index == 1050 {
            settings.scene_animation_enabled
        } else if settings.fullscreen != 0 {
            settings.fullscreen_mw_pos
        } else {
            0
        }));
    }
    let count = match index {
        533 | 789 => 0,
        1053 => 2,
        790 => 4,
        _ => unreachable!(),
    };
    let args = words(arguments, count, vm.pc)?;
    Ok(match index {
        533 => {
            host.system_settings_dirty = 1;
            NativeReturn::Continue
        }

        1053 => {
            use crate::language::{LanguageChangeRequest, LanguageTarget};
            let (selection, target) = match args[0] {
                0 => (&mut host.languages.primary_index, LanguageTarget::Interface),
                1 => (&mut host.languages.message_index, LanguageTarget::MainText),
                2 => (&mut host.languages.subtitle_index, LanguageTarget::Subtitle),
                _ => return Ok(NativeReturn::Continue),
            };
            if *selection != args[1] {
                if host.language_change_requests.len() >= read::MAX_ITEMS {
                    return Err(Error::new(vm.pc, "语言刷新请求数量超过上限"));
                }
                *selection = args[1];
                host.system_settings_dirty = 1;
                host.language_change_requests.push(LanguageChangeRequest {
                    task_id: host.task_id,
                    target,
                });
            }
            NativeReturn::Continue
        }
        789 => NativeReturn::Value(host.configured_shared_slots),
        790 => {
            if args[1] >= 8 {
                return Ok(NativeReturn::Value(0));
            }
            let settings = host
                .system_settings
                .as_ref()
                .ok_or_else(|| Error::new(vm.pc, "宿主未配置全局设置"))?;
            let ranges = if args[0] == 0 {
                &settings.local_flag_ranges
            } else {
                &settings.global_flag_ranges
            };
            vm.write(args[2], &ranges[args[1] as usize][0].to_le_bytes())?;
            // 起点输出可以覆盖已退栈的索引参数；两处输出地址已在写入前确定。
            let index = if let Some(base) = vm.native_arguments {
                u32::from_le_bytes(vm.read((base + 4) as u32, 4)?.try_into().unwrap())
            } else {
                args[1]
            };
            let range = ranges
                .get(index as usize)
                .ok_or_else(|| Error::new(vm.pc, "KCS 变量范围索引越界"))?;
            vm.write(args[3], &range[1].to_le_bytes())?;
            NativeReturn::Value(1)
        }
        _ => unreachable!(),
    })
}
