use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{tag, take_until, take_while1},
    character::complete::{char, line_ending, not_line_ending, space0, space1},
    combinator::{opt, verify},
    sequence::delimited,
};

#[derive(Debug, PartialEq)]
enum FlakeRefType {
    Github,
    Gitlab,
}

impl<'a> TryFrom<&'a str> for FlakeRefType {
    type Error = nom::Err<nom::error::Error<&'a str>>;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        match value {
            "github" => Ok(FlakeRefType::Github),
            "gitlab" => Ok(FlakeRefType::Gitlab),
            _ => Err(nom::Err::Error(nom::error::Error::new(
                value,
                nom::error::ErrorKind::Tag,
            ))),
        }
    }
}

#[derive(Debug, PartialEq)]
struct FlakeRef<'a> {
    ref_type: FlakeRefType,
    repo: &'a str,
    commit: &'a str,
}

impl<'a> FlakeRef<'a> {
    /// Parse a flake ref from the input. Query parameters in the url are ignored.
    fn parse_from(input: &'a str) -> IResult<&'a str, Self> {
        let (input, ref_type_str) = take_until(":")(input)?;
        let (input, _) = char(':')(input)?;
        let ref_type = ref_type_str.try_into()?;

        let (input, repo_and_sha) =
            verify(take_while1(|c: char| c != '?' && c != '\n'), |s: &str| {
                s.matches('/').count() == 2
            })
            .parse(input)?;
        let (input, _) = opt(|i| {
            let (i, _) = char('?')(i)?;
            not_line_ending(i)
        })
        .parse(input)?;

        let parts: Vec<&str> = repo_and_sha.rsplitn(2, '/').collect();
        Ok((
            input,
            FlakeRef {
                ref_type,
                repo: parts[1],
                commit: parts[0],
            },
        ))
    }

    fn repo_url(&self) -> String {
        match self.ref_type {
            FlakeRefType::Github => format!("https://github.com/{}", self.repo),
            FlakeRefType::Gitlab => format!("https://gitlab.com/{}", self.repo),
        }
    }

    fn sha(&self) -> String {
        self.commit[..8].to_string()
    }
}

#[derive(Debug)]
pub struct DatedFlakeRef<'a> {
    flake_ref: FlakeRef<'a>,
    date: &'a str,
}

impl<'a> DatedFlakeRef<'a> {
    fn parse_from(input: &'a str) -> IResult<&'a str, Self> {
        let (input, _) = space0(input)?;
        let (input, url) = delimited(tag("'"), take_until("'"), tag("'")).parse(input)?;
        let (input, _) = space1(input)?;
        let (input, date) = delimited(tag("("), take_until(")"), tag(")")).parse(input)?;
        let (input, _) = line_ending(input)?;

        let (_, flake_ref) = FlakeRef::parse_from(url)?;

        Ok((input, DatedFlakeRef { flake_ref, date }))
    }
}

#[derive(Debug)]
pub struct UpdateInfo<'a> {
    from: DatedFlakeRef<'a>,
    to: DatedFlakeRef<'a>,
}

impl<'a> UpdateInfo<'a> {
    fn parse_from(input: &'a str) -> IResult<&'a str, Self> {
        let (input, from) = DatedFlakeRef::parse_from(input)?;
        let (input, _) = space0(input)?;
        let (input, _) = tag("→")(input)?;
        let (input, to) = DatedFlakeRef::parse_from(input)?;

        Ok((input, UpdateInfo { from, to }))
    }

    fn url(&self) -> Option<String> {
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
    fn parse_from(input: &'a str) -> IResult<&'a str, Self> {
        alt((
            |i| {
                let (i, _) = space0(i)?;
                let (i, _) = tag("follows ")(i)?;
                let (i, repo) = delimited(tag("'"), take_until("'"), tag("'")).parse(i)?;
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
}

impl<'a> Entry<'a> {
    pub fn summary(&self) -> String {
        match self {
            Entry::Updated(name, info) => format!(
                " - Updated input [`{name}`]({}): [`{}` ➡️ `{}`]({}) <sub>({} to {})<sub/>",
                info.from.flake_ref.repo_url(),
                info.from.flake_ref.sha(),
                info.to.flake_ref.sha(),
                info.url().unwrap(), // TODO: handle None
                info.from.date,
                info.to.date,
            )
            .to_string(),
            Entry::Added(info) => match info {
                AddInfo::Follows(repo) => format!(" - Added input (follows `{}`)", repo),
                AddInfo::New(dated_ref) => format!(
                    " - Added input [`{}`]({}) ({})",
                    dated_ref.flake_ref.sha(),
                    dated_ref.flake_ref.repo_url(),
                    dated_ref.date
                ),
            },
        }
    }
}

pub fn parse_header(input: &str) -> IResult<&str, ()> {
    let (input, _) = tag("Flake lock file updates:")(input)?;
    let (input, _) = line_ending(input)?;
    let (input, _) = line_ending(input)?;
    Ok((input, ()))
}

fn parse_updated(input: &str) -> IResult<&str, Entry<'_>> {
    let (input, _) = tag("• Updated input '")(input)?;
    let (input, package) = take_until("':")(input)?;
    let (input, _) = tag("':")(input)?;
    let (input, _) = line_ending(input)?;
    let (input, info) = UpdateInfo::parse_from.parse(input)?;
    Ok((input, Entry::Updated(package, info)))
}

fn parse_added(input: &str) -> IResult<&str, Entry<'_>> {
    let (input, _) = tag("• Added input '")(input)?;
    let (input, _) = take_until("':")(input)?;
    let (input, _) = tag("':")(input)?;
    let (input, _) = line_ending(input)?;
    let (input, info) = AddInfo::parse_from.parse(input)?;
    Ok((input, Entry::Added(info)))
}

pub fn parse_entry(input: &str) -> IResult<&str, Entry<'_>> {
    alt((parse_updated, parse_added)).parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nom::multi::many0;

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
• Updated input 'nixpkgs':
    'github:nixos/nixpkgs/dc704e6102e76aad573f63b74c742cd96f8f1e6c' (2025-10-02)
  → 'github:nixos/nixpkgs/2dad7af78a183b6c486702c18af8a9544f298377' (2025-10-09)
• Updated input 'osh-oxy':
    'github:iff/osh-oxy/e79f39e33912abd5b18ca7f5f1e0d0744d4a09e6' (2025-10-02)
  → 'github:iff/osh-oxy/eed066ec93dba6a85b709a31f482ebcdc376ce88' (2025-10-10)
• Added input 'nihilistic-nvim/rustacean-nvim/gen-luarc/flake-parts':
    follows 'nihilistic-nvim/rustacean-nvim/flake-parts'
"#;

        let remaining = parse_header(input).expect("Failed to parse header").0;
        let (_, entries) = many0(parse_entry)
            .parse(remaining)
            .expect("Failed to parse entries");

        assert_eq!(entries.len(), 7);

        match &entries[0] {
            Entry::Updated(name, info) => {
                assert_eq!(*name, "home-manager");
                assert_eq!(info.from.flake_ref.ref_type, FlakeRefType::Github);
                assert_eq!(info.from.flake_ref.repo, "nix-community/home-manager");
                assert_eq!(
                    info.from.flake_ref.commit,
                    "bd92e8ee4a6031ca3dd836c91dc41c13fca1e533"
                );
                assert_eq!(info.from.date, "2025-10-03");
                assert_eq!(info.to.flake_ref.ref_type, FlakeRefType::Github);
                assert_eq!(info.to.flake_ref.repo, "nix-community/home-manager");
                assert_eq!(
                    info.to.flake_ref.commit,
                    "bcccb01d0a353c028cc8cb3254cac7ebae32929e"
                );
                assert_eq!(info.to.date, "2025-10-10");
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
                assert_eq!(info.date, "2024-11-13");
            }
            _ => panic!("Expected Added entry with New"),
        }

        match &entries.last().unwrap() {
            Entry::Added(AddInfo::Follows(repo)) => {
                assert_eq!(*repo, "nihilistic-nvim/rustacean-nvim/flake-parts");
            }
            _ => panic!("Expected Added entry with Follows"),
        }
    }
}
