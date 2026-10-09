use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use syn::File;

/// A positive cfg predicate must require `test` in every admitted configuration.
/// `not(test)` and `any(test, unix)` still admit production declarations.
pub(crate) fn has_cfg_test_attr(attrs: &[syn::Attribute]) -> bool {
    fn requires_test(meta: &syn::Meta) -> bool {
        match meta {
            syn::Meta::Path(path) => path.is_ident("test"),
            syn::Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
                let Ok(items) = list.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                ) else {
                    return false;
                };
                if list.path.is_ident("all") {
                    items.iter().any(requires_test)
                } else {
                    !items.is_empty() && items.iter().all(requires_test)
                }
            }
            _ => false,
        }
    }
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Meta>()
                .is_ok_and(|meta| requires_test(&meta))
    })
}

pub(crate) struct ParsedRustFile {
    pub(crate) path: PathBuf,
    pub(crate) source: String,
    pub(crate) syntax: File,
}

pub(crate) fn collect_tracked_rust_files(
    paths: &[PathBuf],
) -> Result<Option<Vec<PathBuf>>, String> {
    for path in paths {
        if !path.is_file() && !path.is_dir() {
            return Err(format!("path does not exist: {}", path.display()));
        }
    }
    let mut command = Command::new("git");
    command.args([
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
    ]);
    for path in paths {
        command.arg(path);
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(_) => return Ok(None),
    };
    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("git ls-files output was not valid utf-8: {error}"))?;

    let mut files: Vec<_> = stdout
        .lines()
        .map(PathBuf::from)
        .filter(|path| path.extension() == Some(OsStr::new("rs")))
        .filter(|path| path.exists())
        .collect();
    // Explicit files are intentional inputs even when an ignore rule covers them.
    files.extend(
        paths
            .iter()
            .filter(|path| path.is_file() && path.extension() == Some(OsStr::new("rs")))
            .cloned(),
    );
    files.sort();
    files.dedup();
    Ok(Some(files))
}

pub(crate) fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    collect_files_with_extensions(path, &["rs"], files)
}

pub(crate) fn collect_source_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    collect_files_with_extensions(path, &["rs", "ts", "js"], files)
}

pub(crate) fn load_source(file: &Path) -> Result<String, String> {
    fs::read_to_string(file).map_err(|error| format!("failed to read {}: {error}", file.display()))
}

pub(crate) fn load_parsed_rust_files(files: &[PathBuf]) -> Result<Vec<ParsedRustFile>, String> {
    files
        .iter()
        .map(|file| {
            let source = load_source(file)?;
            let syntax = syn::parse_file(&source)
                .map_err(|error| format!("failed to parse {}: {error}", file.display()))?;
            Ok(ParsedRustFile {
                path: file.clone(),
                source,
                syntax,
            })
        })
        .collect()
}

fn collect_files_with_extensions(
    path: &Path,
    extensions: &[&str],
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if path.is_file() {
        if has_allowed_extension(path, extensions) {
            files.push(path.to_path_buf());
        }
        return Ok(());
    }
    if !path.is_dir() {
        return Err(format!("path does not exist: {}", path.display()));
    }

    for entry in fs::read_dir(path)
        .map_err(|error| format!("failed to read directory {}: {error}", path.display()))?
    {
        let entry = entry.map_err(|error| {
            format!("failed to read directory entry {}: {error}", path.display())
        })?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_files_with_extensions(&entry_path, extensions, files)?;
        } else if has_allowed_extension(&entry_path, extensions) {
            files.push(entry_path);
        }
    }

    Ok(())
}

fn has_allowed_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extensions.contains(&extension))
}
