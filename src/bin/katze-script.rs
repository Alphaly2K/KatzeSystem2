use katze_system2::{Result, anm, cst, cstl, expression, fes, kcs};
use std::{collections::BTreeMap, path::Path};

fn inspect(path: &Path) -> std::result::Result<(String, usize, usize), Box<dyn std::error::Error>> {
    if std::fs::metadata(path)?.len() > 64 * 1024 * 1024 {
        return Err("文件超过读取上限".into());
    }
    let bytes = std::fs::read(path)?;
    Ok(if bytes.starts_with(b"CatScene") {
        ("CST".into(), cst::Script::parse(&bytes)?.records().len(), 0)
    } else if bytes.starts_with(b"CSTL") {
        ("CSTL".into(), cstl::Table::parse(&bytes)?.messages.len(), 0)
    } else if bytes.starts_with(b"ANM\0") {
        (
            "ANM".into(),
            anm::Program::parse(&bytes)?.instructions.len(),
            0,
        )
    } else if bytes.starts_with(b"FES\0") {
        let p = fes::Program::parse(&bytes)?;
        ("FES".into(), 1, p.diagnostics.len())
    } else if bytes.starts_with(b"KCS\0") {
        (
            "KCS".into(),
            kcs::Program::parse(&bytes)?.instructions().len(),
            0,
        )
    } else {
        return Err("未识别的脚本签名".into());
    })
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("eval") if args.len() == 3 => {
            let slots: usize = args[1].parse()?;
            if slots > 1_000_000 {
                return Err("变量槽数超过上限".into());
            }
            let result: Result<i32> =
                expression::eval(&args[2], &mut expression::State::new(slots));
            println!("{}", result?);
        }
        Some("inspect") if args.len() > 1 => {
            let mut files = Vec::new();
            for arg in &args[1..] {
                let path = Path::new(arg);
                if path.is_dir() {
                    for entry in std::fs::read_dir(path)? {
                        let entry = entry?;
                        if entry.file_type()?.is_file() {
                            files.push(entry.path());
                        }
                    }
                } else {
                    files.push(path.to_owned());
                }
            }
            files.sort();
            let mut totals: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
            let mut errors = 0;
            for (i, path) in files.iter().enumerate() {
                match inspect(path) {
                    Ok((kind, count, diagnostics)) => {
                        let total = totals.entry(kind).or_default();
                        total.0 += 1;
                        total.1 += count;
                        total.2 += diagnostics;
                    }
                    Err(e) => {
                        eprintln!("输入 {}：{}", i + 1, e);
                        errors += 1;
                    }
                }
            }
            for (kind, (files, items, diagnostics)) in totals {
                println!("{kind}: files={files} items={items} diagnostics={diagnostics}");
            }
            if errors > 0 {
                return Err(format!("{errors} 个输入读取失败").into());
            }
        }
        _ => {
            return Err(
                "用法：katze-script inspect <文件或目录>... | eval <共享变量槽数> <表达式>".into(),
            );
        }
    }
    Ok(())
}
