use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{tag, take_until, take_while1},
    character::complete::{char, line_ending, not_line_ending, space0, space1},
    combinator::{opt, verify},
    error::{ContextError, ErrorKind, ParseError, context},
    sequence::delimited,
};

type IRes<'a, T> = IResult<&'a str, T, Error<'a>>;

#[derive(Debug)]
pub struct ParseFailure<'a> {
    pub line_num: usize,
    pub context: Option<&'static str>,
    pub fail_line: &'a str,
    pub bad_chunk: &'a str,
}

#[derive(Debug)]
pub struct ParseResult<'a> {
    pub entries: Vec<Entry<'a>>,
    pub failures: Vec<ParseFailure<'a>>,
}

/// Error type that captures the input position and the innermost context label.
#[derive(Debug)]
pub struct Error<'a> {
    pub input: &'a str,
    pub context: Option<&'static str>,
}

impl<'a> ParseError<&'a str> for Error<'a> {
    fn from_error_kind(input: &'a str, _kind: ErrorKind) -> Self {
        Error {
            input,
            context: None,
        }
    }

    fn append(_input: &'a str, _kind: ErrorKind, other: Self) -> Self {
        other
    }

    fn or(self, other: Self) -> Self {
        // keep the error that consumed more input (shorter remaining = more progress)
        if self.input.len() <= other.input.len() {
            self
        } else {
            other
        }
    }
}

impl<'a> ContextError<&'a str> for Error<'a> {
    fn add_context(_input: &'a str, ctx: &'static str, other: Self) -> Self {
        Error {
            input: other.input,
            context: Some(other.context.unwrap_or(ctx)),
        }
    }
}

impl<'a, E> nom::error::FromExternalError<&'a str, E> for Error<'a> {
    fn from_external_error(input: &'a str, _kind: ErrorKind, _e: E) -> Self {
        Error {
            input,
            context: None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum FlakeRefType {
    Github,
    Gitlab,
    Tangled,
}

#[derive(Debug, PartialEq)]
pub struct FlakeRef<'a> {
    pub ref_type: FlakeRefType,
    pub repo: &'a str,
    pub commit: &'a str,
}

impl<'a> FlakeRef<'a> {
    fn parse_from(input: &'a str) -> IRes<'a, Self> {
        let (input, ref_type_str) = take_until(":")(input)?;
        let (input, _) = char(':')(input)?;

        match ref_type_str {
            "github" | "gitlab" => {
                let ref_type = if ref_type_str == "github" {
                    FlakeRefType::Github
                } else {
                    FlakeRefType::Gitlab
                };
                let (input, repo_and_sha) = context(
                    "expected owner/repo/sha",
                    verify(take_while1(|c: char| c != '?' && c != '\n'), |s: &str| {
                        s.matches('/').count() == 2
                    }),
                )
                .parse(input)?;
                let (input, _) = opt(|i| {
                    let (i, _) = char('?')(i)?;
                    not_line_ending(i)
                })
                .parse(input)?;
                let parts: Vec<&str> = repo_and_sha.rsplitn(2, '/').collect();
                #[allow(clippy::indexing_slicing)] // rsplitn gives us two parts
                let (commit, repo) = (parts[0], parts[1]);
                Ok((
                    input,
                    FlakeRef {
                        ref_type,
                        repo,
                        commit,
                    },
                ))
            }
            // TODO for now just parses "tangled.org" and the rest will error
            "git+https" => {
                let (input, _) =
                    context("expected tangled.org host", tag("//tangled.org/@")).parse(input)?;
                let (input, path) = take_while1(|c: char| c != '?' && c != '\n').parse(input)?;
                let (input, _) = char('?')(input)?;
                let (input, query_str) = not_line_ending(input)?;
                let params: Vec<&str> = query_str.split('&').collect();
                let commit = params
                    .iter()
                    .find_map(|p: &&str| p.strip_prefix("rev="))
                    .or_else(|| params.iter().find_map(|p: &&str| p.strip_prefix("ref=")))
                    .ok_or({
                        nom::Err::Error(Error {
                            input: query_str,
                            context: Some("expected rev= or ref= query param"),
                        })
                    })?;
                Ok((
                    input,
                    FlakeRef {
                        ref_type: FlakeRefType::Tangled,
                        repo: path,
                        commit,
                    },
                ))
            }
            _ => Err(nom::Err::Error(Error {
                input: ref_type_str,
                context: Some(
                    "unknown flake ref type: supports github, gitlab, git+https://tangled.org/@",
                ),
            })),
        }
    }

    pub fn repo_url(&self) -> String {
        match &self.ref_type {
            FlakeRefType::Github => format!("https://github.com/{}", self.repo),
            FlakeRefType::Gitlab => format!("https://gitlab.com/{}", self.repo),
            FlakeRefType::Tangled => format!("https://tangled.org/@{}", self.repo),
        }
    }

    pub fn sha(&self) -> String {
        self.commit[..8.min(self.commit.len())].to_string()
    }
}

#[derive(Debug, PartialEq)]
pub struct Date<'a>(&'a str);

impl<'a> Date<'a> {
    fn parse_from(input: &'a str) -> IRes<'a, Self> {
        context(
            "expected date in parentheses",
            delimited(tag("("), take_until(")"), tag(")")),
        )
        .map(Date)
        .parse(input)
    }
}

impl std::fmt::Display for Date<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Debug)]
pub struct DatedFlakeRef<'a> {
    pub flake_ref: FlakeRef<'a>,
    pub date: Date<'a>,
}

impl<'a> DatedFlakeRef<'a> {
    fn parse_from(input: &'a str) -> IRes<'a, Self> {
        let (input, _) = space0(input)?;
        let (input, url) = context(
            "expected flake url in single quotes",
            delimited(tag("'"), take_until("'"), tag("'")),
        )
        .parse(input)?;
        let (input, _) = space1(input)?;
        let (input, date) = Date::parse_from(input)?;
        let (input, _) = line_ending(input)?;

        let (_, flake_ref) = context("flake ref url", FlakeRef::parse_from).parse(url)?;

        Ok((input, DatedFlakeRef { flake_ref, date }))
    }
}

#[derive(Debug)]
pub struct UpdateInfo<'a> {
    pub from: DatedFlakeRef<'a>,
    pub to: DatedFlakeRef<'a>,
}

impl<'a> UpdateInfo<'a> {
    fn parse_from(input: &'a str) -> IRes<'a, Self> {
        let (input, from) = context("'from' flake ref", DatedFlakeRef::parse_from).parse(input)?;
        let (input, _) = space0(input)?;
        let (input, _) = context("expected arrow", tag("→")).parse(input)?;
        let (input, to) = context("'to' flake ref", DatedFlakeRef::parse_from).parse(input)?;

        Ok((input, UpdateInfo { from, to }))
    }

    pub fn url(&self) -> Option<String> {
        let from = &self.from.flake_ref;
        let to = &self.to.flake_ref;

        if from.repo != to.repo || from.ref_type != to.ref_type {
            return None;
        }

        Some(format!(
            "{}/compare/{}...{}",
            from.repo_url(),
            from.commit,
            to.commit
        ))
    }
}

#[derive(Debug)]
pub enum AddInfo<'a> {
    Follows(&'a str),
    New(DatedFlakeRef<'a>),
}

impl<'a> AddInfo<'a> {
    fn parse_from(input: &'a str) -> IRes<'a, Self> {
        alt((
            |i| {
                let (i, _) = space0(i)?;
                let (i, _) = tag("follows ")(i)?;
                let (i, repo) = context(
                    "expected repo in single quotes",
                    delimited(tag("'"), take_until("'"), tag("'")),
                )
                .parse(i)?;
                let (i, _) = line_ending(i)?;
                Ok((i, AddInfo::Follows(repo)))
            },
            |i| {
                let (i, flake_ref) = DatedFlakeRef::parse_from(i)?;
                Ok((i, AddInfo::New(flake_ref)))
            },
        ))
        .parse(input)
    }
}

#[derive(Debug)]
pub enum Entry<'a> {
    Updated(&'a str, UpdateInfo<'a>),
    Added(AddInfo<'a>),
    Removed(&'a str),
}

pub fn render_entry(entry: &Entry) -> String {
    match entry {
        Entry::Updated(name, info) => {
            let url = info
                .url()
                .unwrap_or_else(|| String::from("incompatible url"));
            format!(
                " - Updated input [`{name}`]({}): [`{}` ➡️ `{}`]({}) <sub>({} to {})<sub/>",
                info.from.flake_ref.repo_url(),
                info.from.flake_ref.sha(),
                info.to.flake_ref.sha(),
                url,
                info.from.date,
                info.to.date,
            )
        }
        Entry::Added(info) => match info {
            AddInfo::Follows(repo) => format!(" - Added input (follows `{}`)", repo),
            AddInfo::New(dated_ref) => format!(
                " - Added input [`{}`]({}) ({})",
                dated_ref.flake_ref.sha(),
                dated_ref.flake_ref.repo_url(),
                dated_ref.date
            ),
        },
        Entry::Removed(name) => format!(" - Removed input `{name}`"),
    }
}

pub fn parse_commit_message(input: &str) -> Result<ParseResult<'_>, Error<'_>> {
    let (mut current, _) = parse_header(input).map_err(|e| match e {
        nom::Err::Error(e) | nom::Err::Failure(e) => e,
        nom::Err::Incomplete(_) => Error {
            input,
            context: Some("incomplete input"),
        },
    })?;

    let mut entries = Vec::new();
    let mut failures = Vec::new();

    loop {
        if current.trim().is_empty() {
            break;
        }
        match parse_entry(current) {
            Ok((rest, entry)) => {
                entries.push(entry);
                current = rest;
            }
            Err(nom::Err::Incomplete(_)) => break,
            Err(nom::Err::Error(e) | nom::Err::Failure(e)) => {
                let offset = current.as_ptr() as usize - input.as_ptr() as usize;
                let line_num = input[..offset].lines().count() + 1;
                let next = current
                    .split_inclusive('\n')
                    .skip(1)
                    .find(|line| line.starts_with('•'))
                    .map(|line| {
                        let line_offset = line.as_ptr() as usize - current.as_ptr() as usize;
                        &current[line_offset..]
                    });
                let bad_chunk = match next {
                    Some(rest) => &current[..current.len() - rest.len()],
                    None => current,
                };
                failures.push(ParseFailure {
                    line_num,
                    context: e.context,
                    fail_line: e.input.lines().next().unwrap_or(""),
                    bad_chunk: bad_chunk.trim(),
                });
                match next {
                    Some(rest) => current = rest,
                    None => break,
                }
            }
        }
    }

    Ok(ParseResult { entries, failures })
}

fn parse_header(input: &str) -> IRes<'_, ()> {
    let (input, _) = tag("Flake lock file updates:")(input)?;
    let (input, _) = line_ending(input)?;
    let (input, _) = line_ending(input)?;
    Ok((input, ()))
}

fn parse_entry(input: &str) -> IRes<'_, Entry<'_>> {
    let (input, _) = tag("• ")(input)?;
    let (input, verb) = alt((tag("Updated"), tag("Added"), tag("Removed"))).parse(input)?;
    let (input, _) = tag(" input '")(input)?;
    let (input, name) = take_until("'")(input)?;
    let (input, _) = char('\'')(input)?;

    match verb {
        "Updated" => {
            let (input, _) = tag(":")(input)?;
            let (input, _) = line_ending(input)?;
            let (input, info) = UpdateInfo::parse_from.parse(input)?;
            Ok((input, Entry::Updated(name, info)))
        }
        "Added" => {
            let (input, _) = tag(":")(input)?;
            let (input, _) = line_ending(input)?;
            let (input, info) = AddInfo::parse_from.parse(input)?;
            Ok((input, Entry::Added(info)))
        }
        "Removed" => {
            let (input, _) = line_ending(input)?;
            Ok((input, Entry::Removed(name)))
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_unknown_flake_ref() {
        let input = "• Updated input 'foo':\n    'git://xyz.org/bar?rev=abc' (2025-01-01)\n  → 'git://xyz.org/bar?rev=def' (2025-01-02)\n";
        match parse_entry(input) {
            Err(nom::Err::Error(e)) => {
                assert_eq!(
                    e.context,
                    Some(
                        "unknown flake ref type: supports github, gitlab, git+https://tangled.org/@"
                    )
                );
                assert_eq!(e.input, "git");
            }
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_error_tangled_wrong_host() {
        let input = "• Updated input 'foo':\n    'git+https://tang.org/@foo/bar?rev=abc' (2025-01-01)\n  → 'git+https://tangled.org/@foo/bar?rev=def' (2025-01-02)\n";
        match parse_entry(input) {
            Err(nom::Err::Error(e)) => assert_eq!(e.context, Some("expected tangled.org host")),
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_flake_ref_tangled() {
        let input = "git+https://tangled.org/@tangled.org/core?ref=refs/heads/master&rev=9afe7ce0460ac5b13ad0079bddf35e92adbc5fcb";
        let result = FlakeRef::parse_from(input).expect("parseable flake ref");

        assert_eq!(
            result.1,
            FlakeRef {
                ref_type: FlakeRefType::Tangled,
                repo: "tangled.org/core",
                commit: "9afe7ce0460ac5b13ad0079bddf35e92adbc5fcb",
            }
        );
    }

    #[test]
    fn test_parse_flake_ref() {
        let input = "github:nix-community/home-manager/bd92e8ee4a6031ca3dd836c91dc41c13fca1e533";
        let result = FlakeRef::parse_from(input).expect("parseable flake ref");

        assert_eq!(
            result.1,
            FlakeRef {
                ref_type: FlakeRefType::Github,
                repo: "nix-community/home-manager",
                commit: "bd92e8ee4a6031ca3dd836c91dc41c13fca1e533",
            }
        );
    }

    #[test]
    fn test_parse_flake_ref_with_query() {
        let input =
            "github:nix-community/home-manager/bd92e8ee4a6031ca3dd836c91dc41c13fca1e533?shallow=1";
        let result = FlakeRef::parse_from(input).expect("parseable flake ref");

        assert_eq!(
            result.1,
            FlakeRef {
                ref_type: FlakeRefType::Github,
                repo: "nix-community/home-manager",
                commit: "bd92e8ee4a6031ca3dd836c91dc41c13fca1e533",
            }
        );
    }

    #[test]
    fn test_parse_full_input() {
        let input = r#"Flake lock file updates:

• Updated input 'home-manager':
    'github:nix-community/home-manager/bd92e8ee4a6031ca3dd836c91dc41c13fca1e533' (2025-10-03)
  → 'github:nix-community/home-manager/bcccb01d0a353c028cc8cb3254cac7ebae32929e' (2025-10-10)
• Updated input 'hypr-contrib':
    'github:hyprwm/contrib/513d71d3f42c05d6a38e215382c5a6ce971bd77d' (2025-09-30)
  → 'github:hyprwm/contrib/32e1a75b65553daefb419f0906ce19e04815aa3a' (2025-10-04)
• Updated input 'nihilistic-nvim':
    'github:iff/nihilistic-nvim/be0d9f0311c22ca7ef0d19431d3b2f537a95b764' (2025-10-06)
  → 'github:iff/nihilistic-nvim/9e091eb0f9ccee2ab2711b2226fec9c6af15fb6a' (2025-10-07)
• Added input 'ltstatus/flake-utils':
    'github:numtide/flake-utils/11707dc2f618dd54ca8739b309ec4fc024de578b' (2024-11-13)
• Removed input 'llm-agents/bun2nix/import-tree'
• Updated input 'nixpkgs':
    'github:nixos/nixpkgs/dc704e6102e76aad573f63b74c742cd96f8f1e6c' (2025-10-02)
  → 'github:nixos/nixpkgs/2dad7af78a183b6c486702c18af8a9544f298377' (2025-10-09)
• Updated input 'osh-oxy':
    'github:iff/osh-oxy/e79f39e33912abd5b18ca7f5f1e0d0744d4a09e6' (2025-10-02)
  → 'github:iff/osh-oxy/eed066ec93dba6a85b709a31f482ebcdc376ce88' (2025-10-10)
• Updated input 'tangled':
    'git+https://tangled.org/@tangled.org/core?ref=refs/heads/master&rev=9afe7ce0460ac5b13ad0079bddf35e92adbc5fcb' (2026-05-04)
  → 'git+https://tangled.org/@tangled.org/core?ref=refs/heads/master&rev=11e11dde5087e8a86012a359b9a4a847197ea2f6' (2026-05-04)
• Added input 'nihilistic-nvim/rustacean-nvim/gen-luarc/flake-parts':
    follows 'nihilistic-nvim/rustacean-nvim/flake-parts'
"#;

        let result = parse_commit_message(input).expect("Failed to parse commit message");
        assert!(result.failures.is_empty());
        let entries = result.entries;

        assert_eq!(entries.len(), 9);

        match &entries[0] {
            Entry::Updated(name, info) => {
                assert_eq!(*name, "home-manager");
                assert_eq!(info.from.flake_ref.ref_type, FlakeRefType::Github);
                assert_eq!(info.from.flake_ref.repo, "nix-community/home-manager");
                assert_eq!(
                    info.from.flake_ref.commit,
                    "bd92e8ee4a6031ca3dd836c91dc41c13fca1e533"
                );
                assert_eq!(info.from.date, Date("2025-10-03"));
                assert_eq!(info.to.flake_ref.ref_type, FlakeRefType::Github);
                assert_eq!(info.to.flake_ref.repo, "nix-community/home-manager");
                assert_eq!(
                    info.to.flake_ref.commit,
                    "bcccb01d0a353c028cc8cb3254cac7ebae32929e"
                );
                assert_eq!(info.to.date, Date("2025-10-10"));
            }
            _ => panic!("Expected Updated entry"),
        }

        match &entries[3] {
            Entry::Added(AddInfo::New(info)) => {
                assert_eq!(info.flake_ref.ref_type, FlakeRefType::Github);
                assert_eq!(info.flake_ref.repo, "numtide/flake-utils");
                assert_eq!(
                    info.flake_ref.commit,
                    "11707dc2f618dd54ca8739b309ec4fc024de578b"
                );
                assert_eq!(info.date, Date("2024-11-13"));
            }
            _ => panic!("Expected Added entry with New"),
        }

        match &entries[4] {
            Entry::Removed(name) => {
                assert_eq!(*name, "llm-agents/bun2nix/import-tree");
            }
            _ => panic!("Expected Removed entry"),
        }

        match &entries.last().unwrap() {
            Entry::Added(AddInfo::Follows(repo)) => {
                assert_eq!(*repo, "nihilistic-nvim/rustacean-nvim/flake-parts");
            }
            _ => panic!("Expected Added entry with Follows"),
        }
    }

    #[test]
    fn test_parse_commit_message_header_failure() {
        let input = "Not a flake lock file\n\n• Updated input 'foo':\n";
        let err = parse_commit_message(input).expect_err("expected header parse failure");
        assert_eq!(
            err.input,
            "Not a flake lock file\n\n• Updated input 'foo':\n"
        );
    }

    #[test]
    fn test_parse_commit_message_partial_failure() {
        let input = "Flake lock file updates:\n\n• Updated input 'foo':\n    'git://xyz.org/bar?rev=abc' (2025-01-01)\n  → 'git://xyz.org/bar?rev=def' (2025-01-02)\n• Removed input 'bar'\n";

        let result = parse_commit_message(input).expect("should parse despite entry error");

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.failures.len(), 1);

        match &result.entries[0] {
            Entry::Removed(name) => assert_eq!(*name, "bar"),
            _ => panic!("expected Removed entry"),
        }

        let failure = &result.failures[0];
        assert_eq!(failure.line_num, 3);
        assert_eq!(
            failure.context,
            Some("unknown flake ref type: supports github, gitlab, git+https://tangled.org/@")
        );
        assert_eq!(failure.fail_line, "git");
        assert_eq!(
            failure.bad_chunk,
            "• Updated input 'foo':\n    'git://xyz.org/bar?rev=abc' (2025-01-01)\n  → 'git://xyz.org/bar?rev=def' (2025-01-02)"
        );
    }

    #[test]
    fn test_parse_commit_message_all_failures() {
        let input = "Flake lock file updates:\n\n• Updated input 'foo':\n    'git://xyz.org/bar?rev=abc' (2025-01-01)\n  → 'git://xyz.org/bar?rev=def' (2025-01-02)\n";

        let result = parse_commit_message(input).expect("should return Ok with failures");
        assert!(result.entries.is_empty());
        assert_eq!(result.failures.len(), 1);
    }

    #[test]
    fn test_render_entry_updated_same_repo() {
        let sha_a = "a".repeat(40);
        let sha_b = "b".repeat(40);
        let input = format!(
            "• Updated input 'nixpkgs':\n    'github:nixos/nixpkgs/{sha_a}' (2025-01-01)\n  → 'github:nixos/nixpkgs/{sha_b}' (2025-01-02)\n"
        );
        let (_, entry) = parse_entry(&input).expect("valid entry");
        assert_eq!(
            render_entry(&entry),
            format!(
                " - Updated input [`nixpkgs`](https://github.com/nixos/nixpkgs): \
                [`aaaaaaaa` ➡️ `bbbbbbbb`](https://github.com/nixos/nixpkgs/compare/{sha_a}...{sha_b}) \
                <sub>(2025-01-01 to 2025-01-02)<sub/>"
            )
        );
    }

    #[test]
    fn test_render_entry_updated_cross_repo() {
        let sha_a = "a".repeat(40);
        let sha_b = "b".repeat(40);
        let input = format!(
            "• Updated input 'foo':\n    'github:owner/repo-a/{sha_a}' (2025-01-01)\n  → 'github:owner/repo-b/{sha_b}' (2025-01-02)\n"
        );
        let (_, entry) = parse_entry(&input).expect("valid entry");
        assert!(render_entry(&entry).contains("incompatible url"));
    }

    #[test]
    fn test_render_entry_added_new() {
        let sha = "a".repeat(40);
        let input = format!(
            "• Added input 'flake-utils':\n    'github:numtide/flake-utils/{sha}' (2025-01-01)\n"
        );
        let (_, entry) = parse_entry(&input).expect("valid entry");
        assert_eq!(
            render_entry(&entry),
            " - Added input [`aaaaaaaa`](https://github.com/numtide/flake-utils) (2025-01-01)"
        );
    }

    #[test]
    fn test_render_entry_added_follows() {
        let input = "• Added input 'foo/flake-parts':\n    follows 'foo/flake-utils'\n";
        let (_, entry) = parse_entry(input).expect("valid entry");
        assert_eq!(
            render_entry(&entry),
            " - Added input (follows `foo/flake-utils`)"
        );
    }

    #[test]
    fn test_render_entry_removed() {
        let input = "• Removed input 'old-dep'\n";
        let (_, entry) = parse_entry(input).expect("valid entry");
        assert_eq!(render_entry(&entry), " - Removed input `old-dep`");
    }

    #[test]
    fn test_update_info_url_same_repo() {
        let sha_a = "a".repeat(40);
        let sha_b = "b".repeat(40);
        let input = format!(
            "• Updated input 'foo':\n    'github:owner/repo/{sha_a}' (2025-01-01)\n  → 'github:owner/repo/{sha_b}' (2025-01-02)\n"
        );
        let (_, entry) = parse_entry(&input).expect("valid entry");
        match entry {
            Entry::Updated(_, info) => assert_eq!(
                info.url(),
                Some(format!(
                    "https://github.com/owner/repo/compare/{sha_a}...{sha_b}"
                ))
            ),
            _ => panic!("expected Updated"),
        }
    }

    #[test]
    fn test_update_info_url_different_repo() {
        let sha_a = "a".repeat(40);
        let sha_b = "b".repeat(40);
        let input = format!(
            "• Updated input 'foo':\n    'github:owner/repo-a/{sha_a}' (2025-01-01)\n  → 'github:owner/repo-b/{sha_b}' (2025-01-02)\n"
        );
        let (_, entry) = parse_entry(&input).expect("valid entry");
        match entry {
            Entry::Updated(_, info) => assert_eq!(info.url(), None),
            _ => panic!("expected Updated"),
        }
    }

    #[test]
    fn test_update_info_url_different_ref_type() {
        let sha_a = "a".repeat(40);
        let sha_b = "b".repeat(40);
        let input = format!(
            "• Updated input 'foo':\n    'github:owner/repo/{sha_a}' (2025-01-01)\n  → 'gitlab:owner/repo/{sha_b}' (2025-01-02)\n"
        );
        let (_, entry) = parse_entry(&input).expect("valid entry");
        match entry {
            Entry::Updated(_, info) => assert_eq!(info.url(), None),
            _ => panic!("expected Updated"),
        }
    }
}
