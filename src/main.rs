use flakers::{Entry, parse_commit_message, render_entry};
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

    let result = match parse_commit_message(&input) {
        Ok(result) => result,
        Err(e) => {
            println!("<details><summary>Parse errors</summary><p>");
            println!("\n```");
            println!(
                "Failed to parse header ({}): `{}`",
                e.context.unwrap_or("unknown"),
                e.input.lines().next().unwrap_or("")
            );
            println!("```");
            println!("\n</p></details>\n");
            return ExitCode::FAILURE;
        }
    };

    if !result.failures.is_empty() {
        println!("<details><summary>Parse errors</summary><p>");
        println!("\n```");
        for failure in &result.failures {
            println!(
                "line {} ({}): `{}`\n{}",
                failure.line_num,
                failure.context.unwrap_or("unknown"),
                failure.fail_line,
                failure.bad_chunk
            );
        }
        println!("```");
        println!("\n</p></details>\n");
    }

    result
        .entries
        .iter()
        .filter(|e| matches!(e, Entry::Added(_)))
        .for_each(|e| println!("{}", render_entry(e)));
    result
        .entries
        .iter()
        .filter(|e| matches!(e, Entry::Updated(_, _)))
        .for_each(|e| println!("{}", render_entry(e)));

    if result.failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
