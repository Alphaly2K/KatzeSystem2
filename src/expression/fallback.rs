use super::{Node, Parser, assignment};
use crate::{Error, Result};

#[derive(Clone, Copy)]
enum Operator {
    Binary(&'static str),
    Unary(&'static str),
    Increment(bool, bool),
    Variable(bool),
    Open(bool),
}

impl Operator {
    fn priority(self) -> (i32, i32) {
        match self {
            Self::Binary(op) => match op {
                "*" | "/" | "%" => (27, 26),
                "+" | "-" => (25, 24),
                "<<" | ">>" => (23, 22),
                "<" | "<=" | ">" | ">=" => (21, 20),
                "==" | "!=" => (19, 18),
                "&" => (17, 16),
                "^" => (15, 14),
                "|" => (13, 12),
                "&&" => (11, 10),
                "||" => (9, 8),
                op if assignment(op) => (4, 5),
                _ => (0, 0),
            },
            Self::Unary("~") => (0, 29),
            Self::Unary(_) | Self::Increment(_, true) => (28, 29),
            Self::Increment(_, false) => (29, 28),
            Self::Variable(_) => (30, 31),
            Self::Open(_) => (0, 33),
        }
    }
}

#[derive(Clone, Copy)]
enum Previous {
    Operator,
    Name,
    Value,
    Closing,
}

pub(super) fn parse(bytes: &[u8]) -> Result<Node> {
    if bytes.len() > 65536 {
        return Err(Error::new(0, "表达式过长"));
    }
    let mut cursor = 0;
    let mut values = Vec::new();
    let mut operators = Vec::<(Operator, usize)>::new();
    let mut previous = Previous::Operator;
    let mut tokens = 0;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte.is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        tokens += 1;
        if tokens > 512 {
            return Err(Error::new(cursor, "表达式节点过多"));
        }
        let at = cursor;
        if byte.is_ascii_digit() {
            let mut parser = Parser {
                source: bytes,
                pos: cursor,
                depth: 0,
                nodes: 0,
            };
            values.push(Node::Number(parser.number(true)?));
            cursor = parser.pos;
            previous = Previous::Value;
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            cursor += 1;
            while bytes
                .get(cursor)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                cursor += 1;
            }
            values.push(Node::Named(
                std::str::from_utf8(&bytes[at..cursor]).unwrap().to_owned(),
                None,
            ));
            previous = Previous::Name;
            continue;
        }
        if byte == b'"' {
            cursor += 1;
            while bytes.get(cursor).is_some_and(|b| *b != b'"') {
                cursor += 1;
            }
            if cursor == bytes.len() {
                return Err(Error::new(at, "表达式字符串缺少结束引号"));
            }
            cursor += 1;
            values.push(Node::Number(0));
            previous = Previous::Value;
            continue;
        }
        if byte == b'@' {
            values.push(Node::Current);
            cursor += 1;
            previous = Previous::Value;
            continue;
        }
        if matches!(byte, b')' | b']') {
            let index = byte == b']';
            loop {
                let (operator, position) = operators
                    .pop()
                    .ok_or_else(|| Error::new(at, "表达式括号未配对"))?;
                if let Operator::Open(open_index) = operator {
                    if open_index != index {
                        return Err(Error::new(at, "表达式括号未配对"));
                    }
                    break;
                }
                reduce(operator, position, &mut values)?;
            }
            if index {
                let bank = pop(&mut values, at)?;
                match values.last_mut() {
                    Some(Node::Named(_, target)) => *target = Some(Box::new(bank)),
                    Some(Node::Number(_)) => (),
                    _ => return Err(Error::new(at, "此表达式索引目标尚未接入")),
                }
            }
            cursor += 1;
            previous = Previous::Closing;
            continue;
        }
        let operator = match byte {
            b'(' | b'[' => {
                cursor += 1;
                Operator::Open(byte == b'[')
            }
            b'#' | b'$' | b'\\' => {
                cursor += 1;
                Operator::Variable(byte == b'\\')
            }
            _ => {
                let text = [
                    "<<=", ">>=", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "==",
                    "!=", "<=", ">=", "&&", "||", "<<", ">>", "=", "+", "-", "*", "/", "%", "&",
                    "|", "^", "<", ">", "!", "~", ",", ".", "?", ":",
                ]
                .into_iter()
                .find(|text| bytes[cursor..].starts_with(text.as_bytes()))
                .ok_or_else(|| Error::new(at, "表达式词项尚未接入"))?;
                cursor += text.len();
                match text {
                    "++" | "--" => Operator::Increment(
                        text == "++",
                        matches!(previous, Previous::Name | Previous::Closing),
                    ),
                    "+" | "-" if matches!(previous, Previous::Operator) => Operator::Unary(text),
                    "!" | "~" => Operator::Unary(text),
                    _ => Operator::Binary(text),
                }
            }
        };
        while operators.last().is_some_and(|(top, _)| {
            !matches!(top, Operator::Open(_)) && top.priority().0 >= operator.priority().1
        }) {
            let (top, position) = operators.pop().unwrap();
            reduce(top, position, &mut values)?;
        }
        operators.push((operator, at));
        previous = Previous::Operator;
    }
    while let Some((operator, at)) = operators.pop() {
        if matches!(operator, Operator::Open(_)) {
            return Err(Error::new(at, "表达式括号未配对"));
        }
        reduce(operator, at, &mut values)?;
    }
    Ok(values.pop().unwrap_or(Node::Number(0)))
}

fn pop(values: &mut Vec<Node>, at: usize) -> Result<Node> {
    values
        .pop()
        .ok_or_else(|| Error::new(at, "表达式操作数不足"))
}

fn reduce(operator: Operator, at: usize, values: &mut Vec<Node>) -> Result<()> {
    let right = pop(values, at)?;
    let node = match operator {
        Operator::Variable(local) => Node::Variable(local, Box::new(right), at),
        Operator::Unary(op) => Node::Unary(op.into(), Box::new(right), at),
        Operator::Increment(increment, postfix) => {
            if !matches!(right, Node::Variable(..)) {
                Node::NoWrite(Box::new(right), None)
            } else if postfix {
                Node::Postfix(increment, Box::new(right), at)
            } else {
                Node::Unary(
                    if increment { "++" } else { "--" }.into(),
                    Box::new(right),
                    at,
                )
            }
        }
        Operator::Binary(op) => {
            let left = pop(values, at)?;
            if matches!(op, "," | "." | "?" | ":") {
                Node::Number(0)
            } else if assignment(op) && !matches!(left, Node::Variable(..)) {
                Node::NoWrite(Box::new(left), Some(Box::new(right)))
            } else {
                Node::Binary(op.into(), Box::new(left), Box::new(right), at)
            }
        }
        Operator::Open(_) => unreachable!(),
    };
    values.push(node);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::expression::{State, eval, eval_fes_fallback};

    #[test]
    fn stack_roots_keep_discarded_operands_inert_and_preserve_native_priorities() {
        for (source, result, shared) in [
            ("missing pos ($0+=1) ($1+=2)", 13, [7, 13]),
            ("$0=9 stray ($1+=1)", 0, [7, 12]),
            ("1+(2 3)", 5, [7, 11]),
            ("~1+2", -4, [7, 11]),
            ("+($0+=1)", 0, [8, 11]),
            ("$1=+($0+=1)", 8, [8, 8]),
            ("+$0*3+4", 25, [7, 11]),
            ("+$0++", 0, [7, 11]),
            ("($0)++", 7, [8, 11]),
            ("$0++", 8, [8, 11]),
            ("missing[($0+=1)]=($1+=1)", 0, [8, 12]),
            ("1,($0+=1)", 0, [7, 11]),
        ] {
            let mut state = State::new(2);
            state.shared.copy_from_slice(&[7, 11]);
            assert_eq!(
                eval_fes_fallback(source, &mut state).unwrap(),
                result,
                "{source}"
            );
            assert_eq!(state.shared, shared, "{source}");
        }
        let mut state = State::new(2);
        assert!(eval("$0=1 stray ($1+=1)", &mut state).is_err());
        assert!(eval_fes_fallback("missing ($0+=1", &mut state).is_err());
        assert_eq!(state.shared, [0, 0]);
    }
}
