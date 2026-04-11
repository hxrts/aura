use std::{collections::BTreeSet, path::PathBuf};

use rustc_hir::QPath;
use rustc_lint::LateContext;
use rustc_span::{source_map::SourceMap, Span};

pub(crate) fn source_file_path(source_map: &SourceMap, span: Span) -> PathBuf {
    PathBuf::from(format!(
        "{}",
        source_map
            .lookup_source_file(span.lo())
            .name
            .prefer_remapped_unconditionally()
    ))
}

pub(crate) fn normalized_source_path(source_map: &SourceMap, span: Span) -> String {
    source_file_path(source_map, span)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn path_matches_scope(path: &str, scopes: &[&str]) -> bool {
    scopes.iter().any(|scope| path.contains(scope))
}

pub(crate) fn qpath_def_path<'tcx>(cx: &LateContext<'tcx>, qpath: &QPath<'tcx>) -> Option<String> {
    let def_id = match qpath {
        QPath::Resolved(_, path) => path.res.opt_def_id()?,
        QPath::TypeRelative(_, segment) => segment.res.opt_def_id()?,
    };
    Some(cx.tcx.def_path_str(def_id).replace('\\', "/"))
}

#[derive(Default)]
pub(crate) struct SeenSpans(BTreeSet<(String, u32)>);

impl SeenSpans {
    pub(crate) fn insert(&mut self, path: String, line: u32) -> bool {
        self.0.insert((path, line))
    }
}
