#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageReference {
    pub name: Vec<u8>,
    pub frame: i32,
}

impl ImageReference {
    /// POS2／SIZE2 在属性应用时使用资源名和整数前缀，不调用数值表达式求值器。
    pub fn parse(input: &[u8]) -> Self {
        let split = input.iter().position(|&b| b == b':').unwrap_or(input.len());
        Self {
            name: input[..split].to_vec(),
            frame: integer_prefix(input.get(split + 1..).unwrap_or_default()).unwrap_or(0),
        }
    }
}

pub(super) fn integer_prefix(mut input: &[u8]) -> Option<i32> {
    let negative = input.first() == Some(&b'-');
    if matches!(input.first(), Some(b'-' | b'+')) {
        input = &input[1..];
    }
    if !input.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let radix = if input.starts_with(b"0x") || input.starts_with(b"0X") {
        input = &input[2..];
        16
    } else {
        10
    };
    let number = input
        .iter()
        .map_while(|&b| (b as char).to_digit(radix))
        .fold(0u32, |value, digit| {
            value.wrapping_mul(radix).wrapping_add(digit)
        }) as i32;
    Some(if negative {
        number.wrapping_neg()
    } else {
        number
    })
}
