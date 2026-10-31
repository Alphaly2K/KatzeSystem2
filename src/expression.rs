use crate::{Error, Result};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct State {
    pub shared: Vec<i32>,
    pub local: Vec<i32>,
    pub strings: BTreeMap<u32, Vec<u8>>,
    /// 宿主提供的只读命名值，按名称和银行号索引。
    pub named: BTreeMap<String, BTreeMap<i32, i32>>,
    pub current: i32,
}

impl State {
    pub fn new(shared_slots: usize) -> Self {
        Self {
            shared: vec![0; shared_slots],
            local: vec![0; 512],
            strings: BTreeMap::new(),
            named: BTreeMap::new(),
            current: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Place {
    Shared(usize),
    Local(usize),
}

#[derive(Clone, Copy)]
enum Value {
    Number(i32),
    Reference(Place),
}

impl Value {
    fn load(self, state: &State) -> i32 {
        match self {
            Self::Number(n) => n,
            Self::Reference(p) => p.get(state),
        }
    }
}

impl Place {
    fn get(self, state: &State) -> i32 {
        match self {
            Self::Shared(i) => state.shared[i],
            Self::Local(i) => state.local[i],
        }
    }
    fn put(self, state: &mut State, n: i32) {
        match self {
            Self::Shared(i) => state.shared[i] = n,
            Self::Local(i) => state.local[i] = n,
        }
    }
}

#[derive(Clone, Debug)]
enum Node {
    Number(i32),
    Current,
    Variable(bool, Box<Node>, usize),
    Named(String, Option<Box<Node>>),
    Unary(String, Box<Node>, usize),
    Binary(String, Box<Node>, Box<Node>, usize),
}

#[derive(Clone, Debug)]
pub struct Expression(Node);

impl Expression {
    /// 完整解析后才执行，非法后缀不会产生部分赋值副作用。
    pub fn parse(source: &str) -> Result<Self> {
        if source.len() > 65536 {
            return Err(Error::new(0, "表达式过长"));
        }
        let mut parser = Parser {
            source: source.as_bytes(),
            pos: 0,
            depth: 0,
            nodes: 0,
        };
        let root = parser.expr(1)?;
        parser.space();
        if parser.pos != source.len() {
            return Err(Error::new(parser.pos, "未消费的表达式后缀"));
        }
        Ok(Self(root))
    }

    pub fn evaluate(&self, state: &mut State) -> Result<i32> {
        Ok(evaluate(&self.0, state)?.load(state))
    }
}

pub fn eval(source: &str, state: &mut State) -> Result<i32> {
    Expression::parse(source)?.evaluate(state)
}

/// 对应 CST 使用的数值宿主入口，移除一个末尾百分号。
pub fn eval_parameter(source: &str, state: &mut State) -> Result<i32> {
    eval(source.strip_suffix('%').unwrap_or(source), state)
}

fn evaluate(node: &Node, state: &mut State) -> Result<Value> {
    Ok(match node {
        Node::Number(n) => Value::Number(*n),
        Node::Current => Value::Number(state.current),
        Node::Named(name, index) => {
            let bank = match index {
                Some(index) => evaluate(index, state)?.load(state),
                None => 0,
            };
            Value::Number(
                state
                    .named
                    .get(name)
                    .and_then(|banks| banks.get(&bank))
                    .copied()
                    .unwrap_or(0),
            )
        }
        Node::Variable(local, index, at) => {
            let i = evaluate(index, state)?.load(state);
            let storage = if *local { &state.local } else { &state.shared };
            if i < 0 || i as usize >= storage.len() {
                return Err(Error::new(*at, "变量编号越界"));
            }
            Value::Reference(if *local {
                Place::Local(i as usize)
            } else {
                Place::Shared(i as usize)
            })
        }
        Node::Unary(op, arg, at) => {
            let v = evaluate(arg, state)?;
            let n = v.load(state);
            Value::Number(match op.as_str() {
                "-" => n.wrapping_neg(),
                "!" => (n == 0) as i32,
                "~" => !n,
                "++" | "--" => {
                    let Value::Reference(p) = v else {
                        return Err(Error::new(*at, "自增减需要变量"));
                    };
                    let result = n.wrapping_add(if op == "++" { 1 } else { -1 });
                    p.put(state, result);
                    result
                }
                _ => unreachable!(),
            })
        }
        Node::Binary(op, left, right, at) => {
            let a = evaluate(left, state)?;
            let b = evaluate(right, state)?.load(state);
            if assignment(op) {
                let Value::Reference(p) = a else {
                    return Err(Error::new(*at, "赋值目标需要变量"));
                };
                let n = if op == "=" {
                    b
                } else {
                    binary(&op[..op.len() - 1], p.get(state), b, true, *at)?
                };
                p.put(state, n);
                Value::Number(n)
            } else {
                // 左侧变量保留位置，右子树的副作用完成后再读其值。
                Value::Number(binary(op, a.load(state), b, false, *at)?)
            }
        }
    })
}

pub(crate) fn binary(op: &str, a: i32, b: i32, compound: bool, at: usize) -> Result<i32> {
    Ok(match op {
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" | "%" => {
            if b == 0 && !compound {
                0
            } else if b == 0 || (a == i32::MIN && b == -1) {
                return Err(Error::new(at, "整数除法异常"));
            } else if op == "/" {
                a / b
            } else {
                a % b
            }
        }
        "<<" => a.wrapping_shl(b as u32),
        ">>" => a.wrapping_shr(b as u32),
        "<" => (a < b) as i32,
        "<=" => (a <= b) as i32,
        ">" => (a > b) as i32,
        ">=" => (a >= b) as i32,
        "==" => (a == b) as i32,
        "!=" => (a != b) as i32,
        "&" => a & b,
        "^" => a ^ b,
        "|" => a | b,
        "&&" => (a != 0 && b != 0) as i32,
        "||" => (a != 0 || b != 0) as i32,
        _ => return Err(Error::new(at, "未支持的数值运算")),
    })
}

fn assignment(op: &str) -> bool {
    matches!(
        op,
        "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>="
    )
}

struct Parser<'a> {
    source: &'a [u8],
    pos: usize,
    depth: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self
            .source
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn take(&mut self, s: &[u8]) -> bool {
        self.space();
        if self.source[self.pos..].starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }
    fn expr(&mut self, min: u8) -> Result<Node> {
        self.depth += 1;
        if self.depth > 128 {
            return Err(Error::new(self.pos, "表达式嵌套过深"));
        }
        let mut left = self.primary()?;
        loop {
            self.space();
            let Some(op) = self.operator() else {
                break;
            };
            let bp = match op {
                "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" => 1,
                "||" => 2,
                "&&" => 3,
                "|" => 4,
                "^" => 5,
                "&" => 6,
                "==" | "!=" => 7,
                "<" | ">" | "<=" | ">=" => 8,
                "<<" | ">>" => 9,
                "+" | "-" => 10,
                "*" | "/" | "%" => 11,
                _ => unreachable!(),
            };
            if bp < min {
                break;
            }
            let at = self.pos;
            self.pos += op.len();
            let right = self.expr(if assignment(op) { bp } else { bp + 1 })?;
            self.node()?;
            left = Node::Binary(op.into(), Box::new(left), Box::new(right), at);
        }
        self.depth -= 1;
        Ok(left)
    }
    fn operator(&self) -> Option<&'static str> {
        [
            "<<=", ">>=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "==", "!=", "<=", ">=",
            "&&", "||", "<<", ">>", "=", "+", "-", "*", "/", "%", "&", "|", "^", "<", ">",
        ]
        .into_iter()
        .find(|s| self.source[self.pos..].starts_with(s.as_bytes()))
    }
    fn primary(&mut self) -> Result<Node> {
        self.node()?;
        self.space();
        let at = self.pos;
        for op in ["++", "--", "-", "!", "~"] {
            if self.take(op.as_bytes()) {
                return Ok(Node::Unary(op.into(), Box::new(self.expr(12)?), at));
            }
        }
        let mut node = if self.take(b"(") {
            let n = self.expr(1)?;
            if !self.take(b")") {
                return Err(Error::new(self.pos, "缺少右括号"));
            }
            n
        } else if self.take(b"@") {
            Node::Current
        } else if let Some(&prefix @ (b'#' | b'$' | b'\\')) = self.source.get(self.pos) {
            self.pos += 1;
            let index = if self.take(b"(") {
                let n = self.expr(1)?;
                if !self.take(b")") {
                    return Err(Error::new(self.pos, "变量索引缺少右括号"));
                }
                n
            } else {
                Node::Number(self.number(false)?)
            };
            Node::Variable(prefix == b'\\', Box::new(index), at)
        } else if self
            .source
            .get(self.pos)
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            let start = self.pos;
            while self
                .source
                .get(self.pos)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                self.pos += 1;
            }
            let name = String::from_utf8(self.source[start..self.pos].to_vec()).unwrap();
            let bank = if self.take(b"[") {
                let index = self.expr(1)?;
                if !self.take(b"]") {
                    return Err(Error::new(self.pos, "命名值索引缺少右方括号"));
                }
                Some(Box::new(index))
            } else {
                None
            };
            Node::Named(name, bank)
        } else {
            Node::Number(self.number(true)?)
        };
        for op in ["++", "--"] {
            if self.take(op.as_bytes()) {
                node = Node::Unary(op.into(), Box::new(node), at);
                break;
            }
        }
        Ok(node)
    }
    fn node(&mut self) -> Result<()> {
        self.nodes += 1;
        if self.nodes > 512 {
            return Err(Error::new(self.pos, "表达式节点过多"));
        }
        Ok(())
    }
    fn number(&mut self, hex: bool) -> Result<i32> {
        self.space();
        let start = self.pos;
        let radix = if hex && self.source[self.pos..].starts_with(b"0x") {
            self.pos += 2;
            16
        } else {
            10
        };
        let digits = self.pos;
        let mut n = 0u32;
        while let Some(&b) = self.source.get(self.pos) {
            let Some(d) = (b as char).to_digit(radix) else {
                break;
            };
            n = n.wrapping_mul(radix).wrapping_add(d);
            self.pos += 1;
        }
        if self.pos == digits {
            return Err(Error::new(start, "需要整数或变量"));
        }
        Ok(n as i32)
    }
}
