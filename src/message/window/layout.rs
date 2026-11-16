use crate::{Error, Result, read, text};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Glyph {
    pub code: u16,
    pub color: u32,
    pub position: [i32; 2],
    pub size: [i32; 2],
    pub ruby: bool,
    pub base: usize,
    pub time: u32,
    pub drawn: bool,
}

pub(super) struct Layout {
    pub config: [i32; 24],
    pub glyphs: Vec<Glyph>,
    pub cursor: [i32; 2],
    pub color: u32,
    pub sizes: [i32; 5],
    pub hanging: bool,
    pub hanging_enabled: bool,
    pub starts: Vec<u16>,
    pub ends: Vec<u16>,
    pub configured: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            config: [0; 24],
            glyphs: Vec::new(),
            cursor: [0; 2],
            color: 0,
            sizes: [20; 5],
            hanging: false,
            hanging_enabled: false,
            starts: Vec::new(),
            ends: Vec::new(),
            configured: false,
        }
    }
}

pub(crate) fn codes(bytes: &[u8]) -> Vec<u16> {
    let mut at = 0;
    let mut output = Vec::new();
    while let Some(&first) = bytes.get(at).filter(|&&b| b != 0) {
        let (code, length) = if pair(&bytes[at..]) {
            (u16::from_be_bytes([first, bytes[at + 1]]), 2)
        } else {
            (first as i8 as i16 as u16, 1)
        };
        output.push(code);
        at += length;
    }
    output
}

fn pair(bytes: &[u8]) -> bool {
    bytes.len() >= 2
        && matches!(bytes[0], 0x81..=0x9f | 0xe0..=0xef)
        && matches!(bytes[1], 0x40..=0x7e | 0x80..=0xfc)
}

impl Layout {
    pub fn schedule(&mut self, next: usize, now: u32, fps: u32) {
        let mut counter = 0u32;
        for i in next..self.glyphs.len() {
            self.glyphs[i].time = if self.config[17] == 0 {
                now
            } else if self.glyphs[i].ruby {
                self.glyphs[self.glyphs[i].base].time
            } else {
                let scaled = fps.wrapping_mul(counter) as f32 / 60.;
                counter = counter.wrapping_add(self.config[18] as u32);
                now.wrapping_add((scaled as i32).max(1) as u32)
            };
        }
    }

    pub fn configure(&mut self, bytes: &[u8]) -> Result<bool> {
        read::range(bytes, 0, 0x468)?;
        let config = std::array::from_fn(|i| read::u32_at(bytes, i * 4).unwrap() as i32);
        if config[15] == 0 {
            return Ok(false);
        }
        if config[15] < 0 || config[15] as usize > read::MAX_ITEMS {
            return Err(Error::new(0, "文本窗口字符容量超出范围"));
        }
        let set = |at| -> Result<Vec<u16>> {
            let count = read::u32_at(bytes, at)? as usize;
            if count >= 256 {
                return Err(Error::new(at, "文本窗口禁则字符表超过容量"));
            }
            Ok(read::range(bytes, at + 4, count * 2)?
                .chunks_exact(2)
                .map(|v| u16::from_le_bytes(v.try_into().unwrap()))
                .collect())
        };
        let starts = set(0x60)?;
        let ends = set(0x264)?;
        self.config = config;
        self.color = config[16] as u32;
        self.sizes = [config[6]; 5];
        self.starts = starts;
        self.ends = ends;
        self.glyphs.clear();
        self.configured = true;
        Ok(true)
    }

    pub fn clear(&mut self) {
        self.cursor = [self.config[11], self.config[12]];
        self.color = self.config[16] as u32;
        self.glyphs.clear();
        self.hanging = false;
    }

    fn room(&self) -> bool {
        self.glyphs.len() + 1 < self.config[15] as u32 as usize
    }

    fn newline(&mut self) -> bool {
        self.glyphs.push(Glyph::default());
        if !self.room() {
            return false;
        }
        self.cursor[1] = self.cursor[1].wrapping_add(self.config[9].wrapping_add(self.config[6]));
        self.cursor[0] = if self.hanging {
            self.config[8].wrapping_add(self.config[6])
        } else {
            0
        };
        true
    }

    fn place(&mut self, code: u16) -> bool {
        if !self.room() {
            return false;
        }
        if self.hanging_enabled && self.glyphs.is_empty() && code == 0x8175 {
            self.hanging = true;
        }
        if code == 0 {
            return self.newline();
        }
        let size = self.config[6];
        let advance = if code & 0xff00 != 0 {
            size
        } else {
            size.wrapping_sub(3) / 2 + 4
        };
        if self.config[22] != 0 {
            let ahead = self.config[23]
                .wrapping_mul(size)
                .wrapping_add(self.cursor[0])
                .wrapping_add(advance);
            if ahead >= self.config[2] {
                if !self.starts.contains(&code) && !self.ends.contains(&code) {
                    if !self.newline() {
                        return false;
                    }
                } else if self.cursor[0].wrapping_add(advance) > self.config[2] {
                    let mut at = self.glyphs.len();
                    while at > 0 {
                        at -= 1;
                        if !self.ends.contains(&self.glyphs[at].code) {
                            let insert = at + 1;
                            self.glyphs.insert(insert, Glyph::default());
                            self.cursor[1] =
                                self.cursor[1].wrapping_add(self.config[9].wrapping_add(size));
                            self.cursor[0] = if self.hanging {
                                self.config[8].wrapping_add(size)
                            } else {
                                0
                            };
                            for glyph in &mut self.glyphs[insert + 1..] {
                                glyph.position[0] = self.cursor[0];
                                self.cursor[0] = self.cursor[0]
                                    .wrapping_add(glyph.size[0])
                                    .wrapping_add(self.config[8]);
                            }
                            break;
                        }
                    }
                }
            }
        }
        if self.cursor[0].wrapping_add(advance) > self.config[2] && !self.newline() {
            return false;
        }
        if self.config[9]
            .wrapping_add(self.cursor[1])
            .wrapping_add(size)
            > self.config[3]
        {
            return false;
        }
        self.glyphs.push(Glyph {
            code,
            color: self.color,
            position: self.cursor,
            size: [advance, size],
            ..Default::default()
        });
        self.cursor[0] = self.cursor[0]
            .wrapping_add(self.config[8])
            .wrapping_add(advance);
        true
    }

    fn ruby(&mut self, mut start: usize, end: usize, codes: &[u16]) -> Result<()> {
        while start < self.glyphs.len() && self.glyphs[start].code == 0 {
            start += 1;
        }
        if start >= self.glyphs.len() {
            return Ok(());
        }
        if self.glyphs.len() + codes.len() > self.config[15] as u32 as usize {
            return Err(Error::new(0, "文本窗口注音超过字符记录容量"));
        }
        let size = self.config[7];
        let color = self.glyphs[start].color;
        let first = self.glyphs.len();
        self.glyphs.extend(codes.iter().map(|&code| Glyph {
            code,
            color,
            size: [
                if code & 0xff00 != 0 {
                    size
                } else {
                    size / 2 + 1
                },
                size,
            ],
            ruby: true,
            ..Default::default()
        }));
        let spacing = self.config[8];
        let width = self.glyphs[start..end]
            .iter()
            .fold(0i32, |n, g| n.wrapping_add(g.size[0]).wrapping_add(spacing))
            .wrapping_sub(spacing);
        let cell = size.wrapping_sub(2);
        let ruby_width = codes.iter().fold(0i32, |n, &code| {
            n.wrapping_add(if code & 0xff00 != 0 { cell } else { cell / 2 })
        });
        let step = if width > ruby_width {
            (width as u32 / (codes.len() as u32 + 1)) as i32
        } else {
            cell
        };
        let mut offset = width
            .wrapping_sub((codes.len() as i32).wrapping_sub(1).wrapping_mul(step))
            .wrapping_sub(cell)
            / 2;
        if self.glyphs[start].position[0].wrapping_add(offset) < 0 {
            offset = 0;
        }
        let mut distance = offset;
        let mut bound = self.glyphs[start].size[0];
        for at in first..self.glyphs.len() {
            while offset >= bound && start + 1 < end {
                let old = self.glyphs[start].size[0].wrapping_add(spacing);
                start += 1;
                while start + 1 < end && self.glyphs[start].code == 0 {
                    start += 1;
                }
                bound = bound
                    .wrapping_add(self.glyphs[start].size[0])
                    .wrapping_add(spacing);
                distance = distance.wrapping_sub(old);
            }
            self.glyphs[at].base = start;
            self.glyphs[at].position = [
                self.glyphs[start].position[0].wrapping_add(distance).max(0),
                self.glyphs[start].position[1],
            ];
            offset = offset.wrapping_add(step);
            distance = distance.wrapping_add(step);
        }
        Ok(())
    }

    pub fn align(&mut self) {
        let mut top = match self.config[19] {
            1 => self.config[7],
            2 => self.config[12],
            _ => return,
        };
        let mut first = 0;
        let mut height = 0;
        for end in 0..=self.glyphs.len() {
            if end < self.glyphs.len() && self.glyphs[end].code != 0 {
                if !self.glyphs[end].ruby {
                    height = height.max(self.glyphs[end].size[1]);
                }
                continue;
            }
            for at in first..end {
                self.glyphs[at].position[1] = if self.glyphs[at].ruby {
                    self.glyphs[self.glyphs[at].base].position[1].wrapping_add(self.config[10])
                } else if self.config[19] == 1 {
                    top
                } else {
                    top.wrapping_add(height)
                        .wrapping_sub(self.glyphs[at].size[1])
                };
            }
            top = top.wrapping_add(self.config[9]).wrapping_add(height);
            first = end + 1;
            height = 0;
        }
    }

    pub fn append(&mut self, bytes: &[u8]) -> Result<bool> {
        if !self.configured {
            return Ok(false);
        }
        let bytes = bytes.split(|&b| b == 0).next().unwrap_or_default();
        let (mut at, mut added) = (0, false);
        let (mut group, mut ruby, mut start, mut end) = (false, false, 0, 0);
        let mut reading = Vec::new();
        while at < bytes.len() {
            let wide = pair(&bytes[at..]);
            if wide || self.config[20] == 0 {
                let length = if wide { 2 } else { 1 };
                let code = if wide {
                    u16::from_be_bytes([bytes[at], bytes[at + 1]])
                } else {
                    bytes[at] as u16
                };
                if ruby {
                    reading.push(code);
                } else if self.place(code) {
                    added = true;
                } else {
                    break;
                }
                at += length;
                continue;
            }
            match bytes[at] {
                b'[' => {
                    group = true;
                    ruby = false;
                    start = self.glyphs.len();
                    end = start;
                    reading.clear();
                    at += 1;
                }
                b'/' if group => {
                    ruby = true;
                    end = self.glyphs.len();
                    at += 1;
                }
                b']' if ruby => {
                    self.ruby(start, end, &reading)?;
                    group = false;
                    ruby = false;
                    reading.clear();
                    at += 1;
                }
                b'\\' => {
                    at += 1;
                    match bytes.get(at).copied().unwrap_or(0) {
                        b'c' | b'C' => {
                            at += 1;
                            if let Some((length, color)) = text::integer_prefix(&bytes[at..]) {
                                self.color = color;
                                at += length;
                                if bytes.get(at) == Some(&b';') {
                                    at += 1;
                                }
                            }
                        }
                        b'f' | b'F' => {
                            at += 1;
                            let next = bytes.get(at).copied().unwrap_or(0).to_ascii_lowercase();
                            let size = match next {
                                b's' | b'l' => {
                                    at += 1;
                                    let repeated = bytes
                                        .get(at)
                                        .is_some_and(|v| v.to_ascii_lowercase() == next);
                                    at += usize::from(repeated);
                                    self.sizes[if next == b's' {
                                        usize::from(!repeated)
                                    } else {
                                        3 + usize::from(repeated)
                                    }]
                                }
                                b'n' => {
                                    at += 1;
                                    self.sizes[2]
                                }
                                _ => {
                                    if let Some((length, value)) =
                                        text::integer_prefix(&bytes[at..])
                                    {
                                        let relative = matches!(bytes.get(at), Some(b'+' | b'-'));
                                        at += length;
                                        if relative {
                                            self.config[6].wrapping_add(value as i32)
                                        } else {
                                            value as i32
                                        }
                                    } else {
                                        self.sizes[2]
                                    }
                                }
                            };
                            self.config[6] = size;
                            if bytes.get(at) == Some(&b';') {
                                at += 1;
                            }
                        }
                        b'n' | b'N' => {
                            at += 1;
                            self.place(0);
                        }
                        b'\\' => {
                            at += 1;
                            self.place(b'\\' as u16);
                        }
                        _ => {
                            at += 1;
                        }
                    }
                }
                byte => {
                    if ruby {
                        reading.push(byte as u16);
                    } else if self.place(byte as u16) {
                        added = true;
                    }
                    at += 1;
                }
            }
            if reading.len() > 24 {
                return Err(Error::new(at, "文本窗口注音超过原始临时容量"));
            }
        }
        self.align();
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configure(layout: &mut Layout, capacity: u32) {
        let mut bytes = vec![0; 0x468];
        for (i, value) in [
            0u32,
            0,
            60,
            60,
            4,
            1,
            10,
            8,
            1,
            2,
            (-5i32) as u32,
            0,
            0,
            0,
            0,
            capacity,
            0xff33_2211,
            1,
            3,
            1,
            1,
            0,
            0,
            1,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        layout.configure(&bytes).unwrap();
    }

    #[test]
    fn ruby_records_keep_base_alignment_and_inherit_reveal_times() {
        let mut layout = Layout::default();
        configure(&mut layout, 32);
        assert!(layout.append(b"[AB/cd]E").unwrap());
        assert_eq!(
            layout
                .glyphs
                .iter()
                .map(|g| (g.code, g.position, g.size, g.ruby, g.base))
                .collect::<Vec<_>>(),
            [
                (65, [0, 8], [7, 10], false, 0),
                (66, [8, 8], [7, 10], false, 0),
                (99, [2, 3], [5, 8], true, 0),
                (100, [7, 3], [5, 8], true, 1),
                (69, [16, 8], [7, 10], false, 0),
            ]
        );
        layout.schedule(0, 42, 60);
        assert_eq!(
            layout.glyphs.iter().map(|g| g.time).collect::<Vec<_>>(),
            [43, 45, 43, 45, 48]
        );
    }

    #[test]
    fn full_ascii_pages_still_parse_tags_and_reconfiguration_retains_the_cursor() {
        let mut layout = Layout::default();
        configure(&mut layout, 3);
        assert!(layout.append(b"ABC\\f30;").unwrap());
        assert_eq!(layout.glyphs.len(), 2);
        assert_eq!(layout.config[6], 30);
        assert_eq!(layout.cursor, [16, 0]);
        configure(&mut layout, 3);
        assert!(layout.glyphs.is_empty());
        assert_eq!(layout.cursor, [16, 0]);
        layout.clear();
        assert!(layout.append(b"AB\x82\xa0\\f30;").unwrap());
        assert_eq!(layout.config[6], 10);
    }
}
