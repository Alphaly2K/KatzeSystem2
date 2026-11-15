use crate::{Result, read, text};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak, atomic::AtomicU32},
};

#[derive(Default)]
// 装载程序持有强引用，停止效果不卸载共享激活状态。
pub(super) struct Activations(BTreeMap<String, Weak<AtomicU32>>);

impl Activations {
    pub(super) fn load(&mut self, name: &[u8]) -> Result<Arc<AtomicU32>> {
        let key = text::cp932(name)?.replace('\\', "/").to_uppercase();
        self.0.retain(|_, state| state.strong_count() != 0);
        if let Some(state) = self.0.get(&key).and_then(Weak::upgrade) {
            return Ok(state);
        }
        let state = Arc::new(AtomicU32::new(1));
        self.0.insert(key, Arc::downgrade(&state));
        Ok(state)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Rain,
    Notice,
    Anger,
    Sigh,
    Down,
    Sweat,
    Dis,
    Common,
    Note,
    Angry,
    FlowerBurst,
    Exclamation,
    FilmNoise,
    Asease,
    Surprise,
    Silent,
    Question,
    Sparkle,
    Concentration,
    Glint,
    Energy,
    Digi,
    DigiRing,
    Rising,
    Double,
    Shy,
    Warukira,
    Hadou,
    Rising3,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// 只读取插件标识和常量版本导出，不装载或执行宿主机器码。
pub(super) fn identify(bytes: &[u8]) -> Option<Kind> {
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let pe = read::u32_at(bytes, 0x3c).ok()? as usize;
    let header = bytes.get(pe..pe.checked_add(24)?)?;
    if &header[..4] != b"PE\0\0" || u16_at(header, 4)? != 0x14c {
        return None;
    }
    let optional_size = u16_at(header, 20)? as usize;
    let optional_at = pe.checked_add(24)?;
    let optional = bytes.get(optional_at..optional_at.checked_add(optional_size)?)?;
    if u16_at(optional, 0)? != 0x10b || read::u32_at(optional, 92).ok()? == 0 {
        return None;
    }
    let sections_at = optional_at.checked_add(optional_size)?;
    let sections_size = (u16_at(header, 6)? as usize).checked_mul(40)?;
    let sections = bytes.get(sections_at..sections_at.checked_add(sections_size)?)?;
    let offset = |rva: u32| -> Option<usize> {
        for section in sections.chunks_exact(40) {
            let start = read::u32_at(section, 12).ok()?;
            let size = read::u32_at(section, 16).ok()?;
            if let Some(delta) = rva.checked_sub(start).filter(|&n| n < size) {
                return (read::u32_at(section, 20).ok()? as usize).checked_add(delta as usize);
            }
        }
        None
    };
    let exports_at = offset(read::u32_at(optional, 96).ok()?)?;
    let exports = bytes.get(exports_at..exports_at.checked_add(40)?)?;
    let name_at = offset(read::u32_at(exports, 12).ok()?)?;
    let name = read::zstr(bytes, name_at).ok()?;
    let kind = if name.eq_ignore_ascii_case(b"rain.ppi") {
        Kind::Rain
    } else if name.eq_ignore_ascii_case(b"em_notice.ppi") {
        Kind::Notice
    } else if name.eq_ignore_ascii_case(b"em_oko.ppi") {
        Kind::Anger
    } else if name.eq_ignore_ascii_case(b"em_sigh.ppi") {
        Kind::Sigh
    } else if name.eq_ignore_ascii_case(b"em_down.ppi") {
        Kind::Down
    } else if name.eq_ignore_ascii_case(b"em_sweat.ppi") {
        Kind::Sweat
    } else if name.eq_ignore_ascii_case(b"em_dis.ppi") {
        Kind::Dis
    } else if name.eq_ignore_ascii_case(b"em_common.ppi") {
        Kind::Common
    } else if name.eq_ignore_ascii_case(b"em_note.ppi") {
        Kind::Note
    } else if name.eq_ignore_ascii_case(b"em_angry.ppi") {
        Kind::Angry
    } else if name.eq_ignore_ascii_case(b"ohanaburst.ppi") {
        Kind::FlowerBurst
    } else if name.eq_ignore_ascii_case(b"em_exclamation.ppi") {
        Kind::Exclamation
    } else if name.eq_ignore_ascii_case(b"filmnoise.ppi") {
        Kind::FilmNoise
    } else if name.eq_ignore_ascii_case(b"asease.ppi") {
        Kind::Asease
    } else if name.eq_ignore_ascii_case(b"em_suprise.ppi") {
        Kind::Surprise
    } else if name.eq_ignore_ascii_case(b"em_silent.ppi") {
        Kind::Silent
    } else if name.eq_ignore_ascii_case(b"em_question.ppi") {
        Kind::Question
    } else if name.eq_ignore_ascii_case(b"kirakira.ppi") {
        Kind::Sparkle
    } else if name.eq_ignore_ascii_case(b"concent.ppi") {
        Kind::Concentration
    } else if name.eq_ignore_ascii_case(b"em_kyupin.ppi") {
        Kind::Glint
    } else if name.eq_ignore_ascii_case(b"energy.ppi") {
        Kind::Energy
    } else if name.eq_ignore_ascii_case(b"digi.ppi") {
        Kind::Digi
    } else if name.eq_ignore_ascii_case(b"digiring.ppi") {
        Kind::DigiRing
    } else if name.eq_ignore_ascii_case(b"rising4.ppi") {
        Kind::Rising
    } else if name.eq_ignore_ascii_case(b"em_double.ppi") {
        Kind::Double
    } else if name.eq_ignore_ascii_case(b"em_shy.ppi") {
        Kind::Shy
    } else if name.eq_ignore_ascii_case(b"warukira.ppi") {
        Kind::Warukira
    } else if name.eq_ignore_ascii_case(b"hadou.ppi") {
        Kind::Hadou
    } else if name.eq_ignore_ascii_case(b"rising3.ppi") {
        Kind::Rising3
    } else {
        return None;
    };
    let base = read::u32_at(exports, 16).ok()?;
    let count = read::u32_at(exports, 20).ok()?;
    let functions_at = offset(read::u32_at(exports, 28).ok()?)?;
    let constant = |ordinal: u32| -> Option<u32> {
        let index = ordinal.checked_sub(base).filter(|&i| i < count)? as usize;
        let at = functions_at.checked_add(index.checked_mul(4)?)?;
        let code_at = offset(read::u32_at(bytes, at).ok()?)?;
        let code = bytes.get(code_at..code_at.checked_add(6)?)?;
        if code[0] != 0xb8 || code[5] != 0xc3 {
            return None;
        }
        read::u32_at(code, 1).ok()
    };
    let version = match kind {
        Kind::Rising3 => 0x0002_0000,
        Kind::Down
        | Kind::Silent
        | Kind::Glint
        | Kind::Energy
        | Kind::Digi
        | Kind::DigiRing
        | Kind::Shy => 0x0002_0002,
        _ => 0x0002_0001,
    };
    (constant(3)? == version && constant(4)? == 1).then_some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn activation_is_shared_until_the_last_loaded_program_is_released() -> Result<()> {
        let mut states = Activations::default();
        let first = states.load(b"\x83\x5c\\Concent.ppi")?;
        assert_eq!(first.load(Ordering::Relaxed), 1);
        first.store(20, Ordering::Relaxed);
        let second = states.load(b"\x83\x5c/concent.PPI")?;
        assert!(Arc::ptr_eq(&first, &second));
        let effect = first.clone();
        drop(first);
        drop(effect);
        assert_eq!(
            states
                .load(b"\x83\x5c/CONCENT.ppi")?
                .load(Ordering::Relaxed),
            20
        );
        assert_eq!(
            states.load(b"other/concent.ppi")?.load(Ordering::Relaxed),
            1
        );
        drop(second);
        let reloaded = states.load(b"\x83\x5c/concent.ppi")?;
        assert_eq!(reloaded.load(Ordering::Relaxed), 1);
        assert_eq!(states.0.len(), 1);
        assert_eq!(
            Activations::default()
                .load(b"\x83\x5c/concent.ppi")?
                .load(Ordering::Relaxed),
            1
        );
        Ok(())
    }
}
