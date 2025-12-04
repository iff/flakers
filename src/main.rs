use flakers::{parse_entry, parse_header};
use nom::{Parser,multi::many0};
use std::io::{self, Read};
use clap::Parser as ArgsParser;
use rand::{distr::Alphabetic, Rng};

#[derive(ArgsParser)]
struct Args {
    /// If set, output is wrapped in an armor, allowing it to be piped
    /// directly into $GITHUB_OUTPUT.
    #[arg(long)]
    github_output_armor: Option<String>,
}

struct GitHubOutputArmor {
    delimiter: String,
}

impl Drop for GitHubOutputArmor {
    fn drop(&mut self) {
        println!("{}", self.delimiter);
    }
}

fn main() {
    let args = Args::parse();

    let mut input = String::new();
    #[allow(clippy::expect_used)]
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

    let _guard = if let Some(output_key) = &args.github_output_armor {
        let delimiter = rand::rng()
            .sample_iter(&Alphabetic)
            .take(20)
            .map(char::from)
            .collect::<String>();

        println!("{}<<{}", output_key, &delimiter);
        Some(GitHubOutputArmor { delimiter })
    } else {
        None
    };

    println!("<details><summary>Raw output</summary><p>");
    println!("\n```");
    print!("{}", input);
    println!("```");
    println!("\n</p></details>\n");

    entries
        .iter()
        .filter(|e| matches!(e, flakers::Entry::Added(_)))
        .for_each(|e| println!("{}", e.summary()));
    entries
        .iter()
        .filter(|e| matches!(e, flakers::Entry::Updated(_, _)))
        .for_each(|e| println!("{}", e.summary()));
}
