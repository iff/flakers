use flakers::{parse_entry, parse_header};
use std::io::{self, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut input = String::new();
    #[allow(clippy::expect_used)]
    io::stdin()
        .read_to_string(&mut input)
        .expect("Failed to read stdin");

    println!("<details><summary>Raw output</summary><p>");
    println!("\n```");
    println!("{}", input.trim());
    println!("```");
    println!("\n</p></details>\n");

    let remaining = match parse_header(&input) {
        Ok((remaining, _)) => remaining,
        Err(e) => {
            println!("<details><summary>Parse errors</summary><p>");
            println!("\n```");
            println!("Failed to parse header: {e}");
            println!("```");
            println!("\n</p></details>\n");
            return ExitCode::FAILURE;
        }
    };

    let mut current = remaining;
    let mut entries = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    loop {
        if current.trim().is_empty() {
            break;
        }
        match parse_entry(current) {
            Ok((rest, entry)) => {
                entries.push(entry);
                current = rest;
            }
            Err(_) => {
                let offset = current.as_ptr() as usize - input.as_ptr() as usize;
                let line_num = input[..offset].lines().count() + 1;
                let next = current
                    .split_inclusive('\n')
                    .skip(1)
                    .find(|line| line.starts_with('•'))
                    .map(|line| {
                        let offset = line.as_ptr() as usize - current.as_ptr() as usize;
                        &current[offset..]
                    });
                let bad_chunk = match next {
                    Some(rest) => &current[..current.len() - rest.len()],
                    None => current,
                };
                errors.push(format!("line {line_num}:\n{}", bad_chunk.trim()));
                match next {
                    Some(rest) => current = rest,
                    None => break,
                }
            }
        }
    }

    if !errors.is_empty() {
        println!("<details><summary>Parse errors</summary><p>");
        println!("\n```");
        for error in &errors {
            println!("{error}");
        }
        println!("```");
        println!("\n</p></details>\n");
    }

    entries
        .iter()
        .filter(|e| matches!(e, flakers::Entry::Added(_)))
        .for_each(|e| println!("{}", e.summary()));
    entries
        .iter()
        .filter(|e| matches!(e, flakers::Entry::Updated(_, _)))
        .for_each(|e| println!("{}", e.summary()));

    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
