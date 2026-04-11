use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, Context, Result};
use walkdir::WalkDir;

pub fn repo_root() -> Result<PathBuf> {
    std::env::current_dir().context("reading current directory")
}

pub fn read(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

pub fn read_lines(path: impl AsRef<Path>) -> Result<Vec<String>> {
    Ok(read(path)?.lines().map(str::to_owned).collect())
}

pub fn rust_files_under(path: impl AsRef<Path>) -> Vec<PathBuf> {
    WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.into_path())
        .filter(|path| path.extension().and_then(OsStr::to_str) == Some("rs"))
        .collect()
}

pub fn dirs_under(path: impl AsRef<Path>) -> Vec<PathBuf> {
    WalkDir::new(path)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_dir())
        .map(|entry| entry.into_path())
        .collect()
}

pub fn first_match_line(path: impl AsRef<Path>, needle: &str) -> Result<Option<usize>> {
    let lines = read_lines(path)?;
    Ok(lines
        .iter()
        .enumerate()
        .find_map(|(idx, line)| line.contains(needle).then_some(idx + 1)))
}

pub fn contains(path: impl AsRef<Path>, needle: &str) -> Result<bool> {
    Ok(read(path)?.contains(needle))
}

pub fn is_comment_match(match_line: &str) -> bool {
    let mut parts = match_line.splitn(3, ':');
    let _file = parts.next();
    let _line = parts.next();
    let content = parts.next().unwrap_or(match_line).trim_start();
    content.starts_with("//") || content.starts_with("//!") || content.starts_with("/*")
}

pub fn rg_lines(args: &[String]) -> Result<Vec<String>> {
    let output = command_output("rg", args).context("running rg")?;
    match output.status.code() {
        Some(0) => {
            let repo_root = repo_root().ok();
            Ok(String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| normalize_rg_line(line, repo_root.as_deref()))
                .collect())
        }
        Some(1) => Ok(Vec::new()),
        _ => Err(command_error("rg", args, &output)),
    }
}

pub fn rg_non_comment_lines(args: &[String]) -> Result<Vec<String>> {
    Ok(rg_lines(args)?
        .into_iter()
        .filter(|line| !is_comment_match(line))
        .collect())
}

pub fn rg_exists(args: &[String]) -> Result<bool> {
    Ok(!rg_lines(args)?.is_empty())
}

pub fn run_ok(program: &str, args: &[String]) -> Result<()> {
    let output = command_output(program, args)
        .with_context(|| format!("running {program} {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(command_error(program, args, &output))
}

pub fn command_stdout(program: &str, args: &[String]) -> Result<String> {
    let output = command_output(program, args)
        .with_context(|| format!("running {program} {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(command_error(program, args, &output))
}

pub fn command_output(program: &str, args: &[String]) -> Result<std::process::Output> {
    Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("starting {program} {}", args.join(" ")))
}

pub fn command_error(
    program: &str,
    args: &[String],
    output: &std::process::Output,
) -> anyhow::Error {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow!(
        "{program} {} failed with status {}{}\n{}",
        args.join(" "),
        output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".to_string()),
        if stderr.trim().is_empty() { "" } else { ":" },
        if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        }
    )
}

pub fn git_diff(args: &[String]) -> Result<String> {
    command_stdout("git", args)
}

pub fn repo_relative(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}

fn normalize_rg_line(line: &str, repo_root: Option<&Path>) -> String {
    let Some(repo_root) = repo_root else {
        return line.to_string();
    };
    let repo_prefix = format!("{}/", repo_root.to_string_lossy());
    if let Some(stripped) = line.strip_prefix(&repo_prefix) {
        stripped.to_string()
    } else {
        line.to_string()
    }
}
