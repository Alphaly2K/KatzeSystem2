use super::{ObjectState, Objects};
use crate::{Error, Result, expression::State, fes, read};
use std::collections::BTreeSet;

const FIELD_SIZE: usize = 32;
const MAX_SELECTORS: usize = 16;

/// Selection table used by the FES `CHTABLE2` object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharacterTable {
    roots: Vec<Node>,
    base_option: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
struct Node {
    key: i32,
    children: Vec<Node>,
    choice: Option<Choice>,
}

#[derive(Clone, Debug, PartialEq)]
struct Choice {
    profile: Vec<u8>,
    options: Vec<u8>,
}

impl CharacterTable {
    pub(super) fn load(&mut self, bytes: &[u8]) -> Result<()> {
        self.roots = if bytes.is_empty() {
            Vec::new()
        } else {
            parse_nodes(bytes, 0, 0, &mut BTreeSet::new())?
        };
        Ok(())
    }

    fn count(&self, selectors: &[i32]) -> i32 {
        let mut nodes = self.roots.as_slice();
        for &key in selectors {
            let Some(node) = find(nodes, key) else {
                return 0;
            };
            nodes = &node.children;
        }
        nodes.len() as i32
    }

    fn selection(&self, selectors: &[i32]) -> Option<Vec<u8>> {
        let (&root, rest) = selectors.split_first()?;
        let (&body, options) = rest.split_first()?;
        let groups = &find(&find(&self.roots, root)?.children, body)?.children;
        if groups.is_empty() {
            return None;
        }
        let mut profile: &[u8] = b"";
        let mut fields = [0; FIELD_SIZE];
        for (group, &key) in options.iter().enumerate() {
            let Some(choice) = find(groups, group as i32)
                .and_then(|node| find(&node.children, key))
                .and_then(|node| node.choice.as_ref())
            else {
                continue;
            };
            if profile.is_empty() || profile == b"@" {
                profile = &choice.profile;
            }
            for (field, &value) in fields.iter_mut().zip(&choice.options) {
                if value != b'@' {
                    *field = value;
                } else if *field == 0 {
                    *field = b'0';
                }
            }
        }
        let mut selection = profile.to_vec();
        selection.extend_from_slice(&self.base_option);
        if let Some(end) = fields.iter().rposition(|&field| field != 0) {
            for &field in &fields[..=end] {
                selection.extend([b',', if field == 0 { b'0' } else { field }]);
            }
        }
        Some(selection)
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    bytes.split(|&byte| byte == 0).next().unwrap_or_default()
}

fn find(nodes: &[Node], key: i32) -> Option<&Node> {
    // Serialized siblings are inserted at the front by the original reader.
    nodes.iter().rev().find(|node| node.key == key)
}

fn parse_nodes(
    bytes: &[u8],
    mut offset: usize,
    depth: usize,
    seen: &mut BTreeSet<usize>,
) -> Result<Vec<Node>> {
    let mut nodes = Vec::new();
    loop {
        if depth > MAX_SELECTORS || seen.len() >= read::MAX_ITEMS || !seen.insert(offset) {
            return Err(Error::new(offset, "FES 角色表层级或记录数量无效"));
        }
        let sibling = read::u32_at(bytes, offset)? as usize;
        let child = read::u32_at(bytes, offset + 4)? as usize;
        let length = read::sized(read::u32_at(bytes, offset + 8)?, offset + 8)?;
        let payload = read::range(bytes, offset + 12, length)?;
        let link = |relative| {
            if relative == 0 {
                return Ok(None);
            }
            offset
                .checked_add(relative)
                .filter(|&next| relative >= 12 + length && next < bytes.len())
                .map(Some)
                .ok_or_else(|| Error::new(offset, "FES 角色表记录偏移无效"))
        };
        let children = match link(child)? {
            Some(child) => parse_nodes(bytes, child, depth + 1, seen)?,
            None => Vec::new(),
        };
        nodes.push(Node {
            key: read::u32_at(payload, 0)
                .map_err(|error| Error::new(offset + 12 + error.offset, "FES 角色表记录缺少键"))?
                as i32,
            children,
            choice: payload.get(36..100).map(|fields| Choice {
                profile: terminated(&fields[..FIELD_SIZE]).to_vec(),
                options: terminated(&fields[FIELD_SIZE..]).to_vec(),
            }),
        });
        let Some(sibling) = link(sibling)? else { break };
        offset = sibling;
    }
    Ok(nodes)
}

impl Objects {
    pub(super) fn dispatch_character_table_command(
        &mut self,
        interpreter: &mut fes::Interpreter,
        offset: usize,
        args: &[Vec<u8>],
        target: &fes::ObjectReference,
        numbers: &mut State,
    ) -> Result<bool> {
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        let first = banks.clone().find_map(|bank| {
            let handle = self.lookup(&target.name, bank as i32)?;
            matches!(self.get(handle)?.state, ObjectState::CharacterTable(_)).then_some(handle)
        });
        let Some(first) = first else { return Ok(false) };
        let command = args[1].as_slice();
        if command == b"baseopt" {
            let value = args.get(2).map_or(b"".as_slice(), Vec::as_slice);
            let option = terminated(value);
            if option.len() >= FIELD_SIZE {
                return Err(Error::new(offset, "FES 角色表基础选项超过容量"));
            }
            for bank in banks {
                if let Some(handle) = self.lookup(&target.name, bank as i32) {
                    if let ObjectState::CharacterTable(table) =
                        &mut self.get_mut(handle).unwrap().state
                    {
                        table.base_option = option.to_vec();
                    }
                }
            }
            return Ok(true);
        }
        if target.bank == -1 && command == b"max" {
            return Ok(true);
        }
        if command == b"getfile" && args.get(2).is_none() {
            return Ok(true);
        }
        let mut eval = |arg: &[u8]| {
            if arg.is_empty() {
                return Ok(0);
            }
            interpreter
                .eval(arg, numbers)
                .map_err(|error| Error::new(offset + error.offset, error.message))
        };
        let slot = args.get(2).map_or(Ok(0), |arg| eval(arg))?;
        let selectors = args
            .iter()
            .skip(3)
            .take(MAX_SELECTORS)
            .map(|arg| eval(arg))
            .collect::<Result<Vec<_>>>()?;
        if command == b"max" {
            let ObjectState::CharacterTable(table) = &self.get(first).unwrap().state else {
                unreachable!()
            };
            if let Some(output) = numbers.shared.get_mut(slot as usize) {
                *output = table.count(&selectors);
            }
        } else {
            for bank in banks {
                let Some(handle) = self.lookup(&target.name, bank as i32) else {
                    break;
                };
                if let ObjectState::CharacterTable(table) = &self.get(handle).unwrap().state {
                    numbers
                        .strings
                        .insert(slot as u32, table.selection(&selectors).unwrap_or_default());
                }
            }
        }
        Ok(true)
    }
}
