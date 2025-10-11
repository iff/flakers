use flakers::{parse_entry, parse_header};
use nom::multi::many0;
use nom::Parser;
use std::io::{self, Read};

fn main() {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .expect("Failed to read stdin");

    let remaining = match parse_header(&input) {
        Ok((remaining, _)) => remaining,
        Err(e) => {
            eprintln!("Failed to parse header: {}", e);
            std::process::exit(1);
        }
    };

    let entries = match many0(parse_entry).parse(remaining) {
        Ok((_, entries)) => entries,
        Err(e) => {
            eprintln!("Failed to parse entries: {}", e);
            std::process::exit(1);
        }
    };

    println!("<details><summary>Raw output</summary><p>");
    println!("\n```");
    print!("{}", input);
    println!("```");
    println!("\n</p></details>\n");

    for entry in &entries {
        println!("{}", entry.summary());
    }
}
