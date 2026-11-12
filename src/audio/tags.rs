use crate::{Error, Result, read};
use std::collections::BTreeMap;

pub(super) fn parse(bytes: &[u8]) -> Result<BTreeMap<Vec<u8>, u32>> {
    if bytes.len() > read::MAX_BYTES {
        return Err(Error::new(0, "声音 TAG 表超过读取上限"));
    }
    let mut tags = BTreeMap::new();
    if !bytes.len().is_multiple_of(24) {
        return Ok(tags);
    }
    for offset in (0..bytes.len()).step_by(24) {
        let name = read::zstr(bytes, offset)?.to_vec();
        let value = read::u32_at(bytes, offset + 20)?;
        tags.entry(name).or_insert(value);
    }
    Ok(tags)
}
