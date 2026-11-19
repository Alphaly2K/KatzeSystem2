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

fn native_calls(
    table: &str,
    paths: &[String],
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    if !matches!(table, "ordinary" | "motion" | "particle") {
        return Err("宿主表须为 ordinary、motion 或 particle".into());
    }
    let mut files = Vec::new();
    for path in paths.iter().map(Path::new) {
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                if entry.file_type()?.is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("kcs"))
                {
                    files.push(entry.path());
                }
            }
        } else {
            files.push(path.to_owned());
        }
    }
    files.sort();
    files.dedup();
    let mut sites = BTreeMap::<(u32, u32, Option<u32>), usize>::new();
    let mut instructions = 0;
    let mut calls = 0;
    let mut unresolved_indices = 0;
    for (number, path) in files.iter().enumerate() {
        let program = (|| {
            if std::fs::metadata(path)?.len() > 64 * 1024 * 1024 {
                return Err("文件超过读取上限".into());
            }
            Ok::<_, Box<dyn std::error::Error>>(kcs::Program::parse(&std::fs::read(path)?)?)
        })()
        .map_err(|error| format!("输入 {}：{error}", number + 1))?;
        instructions += program.instructions().len();
        for instruction in program.instructions().iter().filter(|i| i.opcode == 0x2b) {
            calls += 1;
            let index = instruction.operands[0];
            if index.flags != 3 {
                unresolved_indices += 1;
                continue;
            }
            let arguments = instruction.operands[1];
            let bytes = (arguments.flags == 3).then_some(arguments.payload);
            *sites
                .entry((program.control, index.payload, bytes))
                .or_default() += 1;
        }
    }
    println!("table\tprogram_version\tindex\targument_bytes\tcall_sites");
    for ((version, index, bytes), count) in sites {
        let bytes = bytes.map_or_else(|| "?".into(), |n| n.to_string());
        println!(
            "{table}\t{}.{}\t{index}\t{bytes}\t{count}",
            version & 0xffff,
            version >> 16
        );
    }
    println!(
        "summary\ttable={table}\tprograms={}\tinstructions={instructions}\tcall_sites={calls}\tunresolved_indices={unresolved_indices}",
        files.len()
    );
    Ok(())
}

fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("native-calls") if args.len() > 2 => native_calls(&args[1], &args[2..])?,
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
                "用法：katze-script inspect <文件或目录>... | native-calls <宿主表> <KCS 文件或目录>... | eval <共享变量槽数> <表达式>".into(),
            );
        }
    }
    Ok(())
}
