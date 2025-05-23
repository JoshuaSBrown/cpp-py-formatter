// Preludes
use rayon::prelude::*;

// Standard Imports
use std::{
    env,
    error::Error,
    ffi::OsStr,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{self, Child, Command, Stdio},
};

// Third Party Imports
use clap::{Arg, ArgMatches, SubCommand};
use glob::Pattern;
use reqwest::{blocking::Client, header};
use serde::de::DeserializeOwned;

mod github;

use github::*;

fn main() -> Result<(), Box<dyn Error>> {
    App::run()
}

#[derive(Debug)]
struct App {
    clang_format_path: PathBuf,
    includes: Vec<Pattern>,
    excludes: Vec<Pattern>,
    github_workspace: PathBuf,
    bot_name: String,
    github_token: String,
    github_client: Client,
    black_path: PathBuf,
    py_includes: Vec<Pattern>,
}

impl App {
    fn run() -> Result<(), Box<dyn Error>> {
        let matches = clap::App::new("cpp-py-format")
            .author("Andrew Gaspar <andrew.gaspar@outlook.com>, Joshua Brown <joshbro42867@yahoo.com")
            .about("Runner code for executing clang-format, and black")
            .arg(
                Arg::with_name("github-token")
                    .long("github-token")
                    .takes_value(true)
                    .required(true)
            )
            .arg(
                Arg::with_name("clang-format-version")
                    .long("clang-format-version")
                    .takes_value(true)
                    .default_value("10")
                    .conflicts_with("clang-format-override"),
            )
            .arg(
                Arg::with_name("clang-format-override")
                    .long("clang-format-override")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("black-override")
                    .long("black-override")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("include")
                    .long("include")
                    .takes_value(true)
                    .required(true)
                    .value_delimiter(",")
                    .default_value("**/*.c,**/*.h,**/*.C,**/*.H,**/*.cpp,**/*.hpp,**/*.cxx,**/*.hxx,**/*.c++,**/*.h++,**/*.cc,**/*.hh"),
            )
            .arg(
                Arg::with_name("py_include")
                    .long("py_include")
                    .takes_value(true)
                    .required(true)
                    .value_delimiter(",")
                    .default_value("**/*.py"),
            )
            .arg(
                Arg::with_name("exclude")
                    .long("exclude")
                    .takes_value(true)
                    .value_delimiter(",")
                    .default_value(""),
            )
            .arg(
                Arg::with_name("bot-name")
                    .long("bot-name")
                    .takes_value(true)
                    .default_value("cpp-py-formatter")
            )
            .subcommand(SubCommand::with_name("command"))
            .subcommand(SubCommand::with_name("check"))
            .subcommand(SubCommand::with_name("list"))
            .get_matches();

        let clang_format_version = matches.value_of("clang-format-version").unwrap();

        let clang_format_path: PathBuf =
            if let Some(clang_format_override) = matches.value_of("clang-format-override") {
                clang_format_override.into()
            } else if env::var("GITHUB_ACTION").is_ok() {
                format!("/clang-format/clang-format-{}", clang_format_version).into()
            } else {
                String::from_utf8(
                    Command::new("which")
                        .arg("clang-format")
                        .output()
                        .unwrap()
                        .stdout,
                )
                .unwrap()
                .lines()
                .next()
                .unwrap()
                .into()
            };

        if !clang_format_path.exists() {
            eprintln!("Error: No clang-format version {}", clang_format_version);
            std::process::exit(1);
        }

        let black_path: PathBuf =
            if let Some(black_override) = matches.value_of("black-override") {
                black_override.into()
            } else {
                String::from_utf8(
                    Command::new("which")
                        .arg("black")
                        .output()
                        .unwrap()
                        .stdout,
                )
                .unwrap()
                .lines()
                .next()
                .unwrap()
                .into()
            };

        if !black_path.exists() {
            eprintln!("Error: No black python formatter found");
            std::process::exit(1);
        }

        let includes = matches
            .values_of("include")
            .unwrap()
            .map(Pattern::new)
            .collect::<Result<Vec<_>, _>>()?;
        let py_includes = matches
            .values_of("py_include")
            .unwrap()
            .map(Pattern::new)
            .collect::<Result<Vec<_>, _>>()?;
        let excludes = matches
            .values_of("exclude")
            .unwrap()
            .map(Pattern::new)
            .collect::<Result<Vec<_>, _>>()?;

        if let Ok(github_workspace) = env::var("GITHUB_WORKSPACE") {
            env::set_current_dir(PathBuf::from(github_workspace))?;
        }

        let github_token = matches.value_of("github-token").unwrap().to_owned();

        let github_client = {
            let mut headers = header::HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                header::HeaderValue::from_str(&format!("token {}", github_token))?,
            );

            Client::builder()
                .user_agent("cpp-py-formatter")
                .default_headers(headers)
                .build()?
        };

        let app = App {
            clang_format_path,
            includes,
            excludes,
            github_workspace: PathBuf::new(),
            bot_name: matches.value_of("bot-name").unwrap().into(),
            github_token,
            github_client,
            black_path,
            py_includes,
        };

        if let Some(matches) = matches.subcommand_matches("list") {
            app.list(matches);
            process::exit(0);
        }

        match matches.subcommand() {
            ("command", matches) => app.command(matches.unwrap())?,
            ("check", matches) => app.check(matches.unwrap())?,
            _ => panic!("Unexcepted subcommand"),
        }

        Ok(())
    }

    fn clone(&self, full_name: &str, branch: &str, depth: usize) -> Result<(), Box<dyn Error>> {
        assert!(cmd(
            "git",
            &[
                "clone",
                "-b",
                branch,
                "--tags",
                "--depth",
                &depth.to_string(),
                &format!(
                    "https://x-access-token:{}@github.com/{}.git",
                    self.github_token, full_name
                ),
                ".",
            ]
        )?
        .wait()?
        .success());

        Ok(())
    }

    fn list_files<'a>(&'a self, includes: &'a [Pattern], excludes: &'a [Pattern]) -> impl Iterator<Item = String> + 'a {
        BufReader::new(
            Command::new("git")
                .args(&["ls-tree", "-r", "HEAD", "--name-only", "--full-tree"])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap()
                .stdout
                .unwrap(),
        )
            .lines()
            .map(|s| s.unwrap())
            .filter(move |s| includes.iter().any(|p| p.matches(&s)))
            .filter(move |s| !excludes.iter().any(|p| p.matches(&s)))
    }

    fn list_cpp_files<'a>(&'a self) -> impl Iterator<Item = String> + 'a {
        self.list_files(&self.includes, &self.excludes)
    }

    fn list_py_files<'a>(&'a self) -> impl Iterator<Item = String> + 'a {
        self.list_files(&self.py_includes, &self.excludes)
    }

    fn format_all(&self) {
        self.list_cpp_files().par_bridge().for_each(|file| {
            cmd(&self.clang_format_path, &["-i", &file])
                .unwrap()
                .wait()
                .unwrap();
        });
        self.list_py_files().par_bridge().for_each(|file| {
            cmd(&self.black_path, &[ &file])
                .unwrap()
                .wait()
                .unwrap();
        });
    }

    fn output_help(
        &self,
        app: &clap::App,
        pull_request: GitHubPullRequest,
    ) -> Result<(), Box<dyn Error>> {
        let mut help = Vec::new();
        app.write_help(&mut help)?;

        // put the usage in code quotes
        let body = format!(
            "\
```
USAGE:
    @{} format [--amend]

FLAGS:
    --amend Amends the previous commit with formatting
```",
            self.bot_name
        );

        self.github_client
            .post(&pull_request.comments_url)
            .json(&GitHubIssueCreate { body })
            .send()?;
        Ok(())
    }

    fn configure(&self) -> Result<(), Box<dyn Error>> {
        assert!(cmd(
            "git",
            &[
                "config",
                "--global",
                "user.email",
                &format!("{}@automation.bot", self.bot_name),
            ]
        )?
        .wait()?
        .success());

        assert!(
            cmd("git", &["config", "--global", "user.name", &self.bot_name])?
                .wait()?
                .success()
        );

        Ok(())
    }

    fn command(&self, _matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
        if env::var("GITHUB_EVENT_NAME")? != "issue_comment" {
            eprintln!("Error: This action is only compatible with 'issue_comment' events");
            process::exit(1);
        }

        let payload: GitHubIssueCommentEvent = load_payload()?;

        if !payload
            .comment
            .body
            .starts_with(&format!("@{}", self.bot_name))
        {
            eprintln!("Error: The command must start with @{}", self.bot_name);
            std::process::exit(1);
        }

        let pull_request = match payload.issue.pull_request {
            Some(pr) => {
                let response = self.github_client.get(&pr.url).send()?;
                if !response.status().is_success() {
                    println!("Error: {}", response.text()?);
                    std::process::exit(1);
                } else {
                    response.json::<GitHubPullRequest>()?
                }
            }
            None => {
                eprintln!("Error: cpp-py-formatter only works with PR comments");
                std::process::exit(1);
            }
        };

        let command_arr = shell_words::split(&payload.comment.body)?;

        let mut app = clap::App::new(&format!("@{}", self.bot_name))
            .subcommand(SubCommand::with_name("format").arg(Arg::with_name("amend").long("amend")));

        let matches = match app.get_matches_from_safe_borrow(command_arr) {
            Ok(matches) => matches,
            Err(_) => {
                self.output_help(&app, pull_request)?;
                std::process::exit(1);
            }
        };

        let matches = if let Some(matches) = matches.subcommand_matches("format") {
            matches
        } else {
            self.output_help(&app, pull_request)?;
            std::process::exit(1);
        };

        self.clone(
            &pull_request.head.repo.full_name,
            &pull_request.head.r#ref,
            if matches.is_present("amend") { 2 } else { 1 },
        )?;
        self.configure()?;
        self.format_all();

        let git_diff = Command::new("git")
            .args(&["diff", "--exit-code"])
            .output();

        let git_diff_output = git_diff.unwrap();
        let git_diff_str = String::from_utf8_lossy(&git_diff_output.stdout);

        if git_diff_str.len() > 0 {
            if !matches.is_present("amend") {
                assert!(cmd("git", &["commit", "-am", "cpp-py-formatter"])?
                    .wait()?
                    .success());

                assert!(cmd("git", &["push"])?.wait()?.success());
            } else {
                assert!(cmd("git", &["commit", "-a", "--amend", "--no-edit"])?
                    .wait()?
                    .success());

                assert!(cmd("git", &["push", "--force"])?.wait()?.success());
            }
        }

        Ok(())
    }

    fn check(&self, _matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
        let payload: GitHubPushEvent = load_payload()?;
        let branch = ref_to_branch(&payload.r#ref);
        self.clone(&payload.repository.full_name, branch, 1)?;
        self.format_all();

        process::exit(
            cmd("git", &["diff", "--exit-code"])?
                .wait()?
                .code()
                .unwrap_or(1),
        )
    }

    fn list(&self, _matches: &ArgMatches) {
        for file in self.list_cpp_files() {
            println!("{}", file);
        }
        for file in self.list_py_files() {
            println!("{}", file);
        }
        process::exit(0)
    }
}

fn load_payload<T: DeserializeOwned>() -> Result<T, Box<dyn Error>> {
    let github_event_path = env::var("GITHUB_EVENT_PATH")?;
    let github_event = std::fs::read_to_string(&github_event_path)?;
    Ok(serde_json::from_str(&github_event)?)
}

fn ref_to_branch(r#ref: &str) -> &str{
    let branch_prefix = "refs/heads/";
    let tag_prefix = "refs/tags/";

    if r#ref.starts_with(branch_prefix) {
        &r#ref[branch_prefix.len()..]
    } else if r#ref.starts_with(tag_prefix) {
        &r#ref[tag_prefix.len()..]
    } else {
        // Will error out if the ref is neither a branch or tag on clone 
        r#ref
    }
}

fn cmd<S: AsRef<OsStr>>(program: S, args: &[&str]) -> Result<Child, Box<dyn Error>> {
    print!("> Running: {}", program.as_ref().to_str().unwrap());
    for a in args {
        print!(" {}", a);
    }
    println!();
    Ok(Command::new(program).args(args).spawn()?)
}
