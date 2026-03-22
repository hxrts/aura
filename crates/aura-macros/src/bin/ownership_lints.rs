use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::Span;
use quote::ToTokens;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    AttrStyle, Block, Expr, ExprAwait, ExprCall, ExprGroup, ExprMethodCall, ExprParen, ExprPath,
    ExprReference, File, FnArg, ImplItem, ImplItemFn, Item, ItemFn, ItemMod, ItemStruct, Local,
    MetaNameValue, Pat, ReturnType, Token, Type, Visibility,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum LintMode {
    WorkflowNoViewReadsForDecisions,
    WorkflowNoViewWrites,
    WorkflowNoFallbackDefaults,
    WorkflowNoViewDerivedReadiness,
    WorkflowNoViewDerivedRecipientResolution,
    WorkflowUnboundedRuntimeAwaits,
    SemanticOwnerBoundedAwaits,
    BestEffortSideEffectBoundary,
    SemanticOwnerDetachedContinuation,
    SemanticOwnerNoSpawn,
    SemanticOwnerProofSuccess,
    WorkflowProofBearingSuccess,
    ProofIssuerAuthoritativeSource,
    ParityCriticalIgnoredResults,
    ActorOwnedTaskSpawn,
    AsyncSessionOwnership,
    FrontendSemanticHandoffBoundary,
    HarnessMoveOwnershipBoundary,
    HarnessReadinessOwnership,
    HarnessRecoveryOwnership,
    OptionalOwnerBoundary,
    TimeoutPolicyBoundary,
    TimeDomainUsage,
    AuthoritativeRefNoReresolution,
    WeakToStrongIdentifierUpgrade,
}

impl LintMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "workflow-no-view-reads-for-decisions" => Ok(Self::WorkflowNoViewReadsForDecisions),
            "workflow-no-view-writes" => Ok(Self::WorkflowNoViewWrites),
            "workflow-no-fallback-defaults" => Ok(Self::WorkflowNoFallbackDefaults),
            "workflow-no-view-derived-readiness" => Ok(Self::WorkflowNoViewDerivedReadiness),
            "workflow-no-view-derived-recipient-resolution" => {
                Ok(Self::WorkflowNoViewDerivedRecipientResolution)
            }
            "workflow-unbounded-runtime-awaits" => Ok(Self::WorkflowUnboundedRuntimeAwaits),
            "semantic-owner-bounded-awaits" => Ok(Self::SemanticOwnerBoundedAwaits),
            "best-effort-side-effect-boundary" => Ok(Self::BestEffortSideEffectBoundary),
            "semantic-owner-detached-continuation" => Ok(Self::SemanticOwnerDetachedContinuation),
            "semantic-owner-no-spawn" => Ok(Self::SemanticOwnerNoSpawn),
            "semantic-owner-proof-success" => Ok(Self::SemanticOwnerProofSuccess),
            "workflow-proof-bearing-success" => Ok(Self::WorkflowProofBearingSuccess),
            "proof-issuer-authoritative-source" => Ok(Self::ProofIssuerAuthoritativeSource),
            "parity-critical-ignored-results" => Ok(Self::ParityCriticalIgnoredResults),
            "actor-owned-task-spawn" => Ok(Self::ActorOwnedTaskSpawn),
            "async-session-ownership" => Ok(Self::AsyncSessionOwnership),
            "frontend-semantic-handoff-boundary" => Ok(Self::FrontendSemanticHandoffBoundary),
            "harness-move-ownership-boundary" => Ok(Self::HarnessMoveOwnershipBoundary),
            "harness-readiness-ownership" => Ok(Self::HarnessReadinessOwnership),
            "harness-recovery-ownership" => Ok(Self::HarnessRecoveryOwnership),
            "optional-owner-boundary" => Ok(Self::OptionalOwnerBoundary),
            "timeout-policy-boundary" => Ok(Self::TimeoutPolicyBoundary),
            "time-domain-usage" => Ok(Self::TimeDomainUsage),
            "authoritative-ref-no-reresolution" => Ok(Self::AuthoritativeRefNoReresolution),
            "weak-to-strong-identifier-upgrade" => Ok(Self::WeakToStrongIdentifierUpgrade),
            other => Err(format!("unknown lint mode: {other}")),
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::WorkflowNoViewReadsForDecisions => "workflow-no-view-reads-for-decisions",
            Self::WorkflowNoViewWrites => "workflow-no-view-writes",
            Self::WorkflowNoFallbackDefaults => "workflow-no-fallback-defaults",
            Self::WorkflowNoViewDerivedReadiness => "workflow-no-view-derived-readiness",
            Self::WorkflowNoViewDerivedRecipientResolution => {
                "workflow-no-view-derived-recipient-resolution"
            }
            Self::WorkflowUnboundedRuntimeAwaits => "workflow-unbounded-runtime-awaits",
            Self::SemanticOwnerBoundedAwaits => "semantic-owner-bounded-awaits",
            Self::BestEffortSideEffectBoundary => "best-effort-side-effect-boundary",
            Self::SemanticOwnerDetachedContinuation => "semantic-owner-detached-continuation",
            Self::SemanticOwnerNoSpawn => "semantic-owner-no-spawn",
            Self::SemanticOwnerProofSuccess => "semantic-owner-proof-success",
            Self::WorkflowProofBearingSuccess => "workflow-proof-bearing-success",
            Self::ProofIssuerAuthoritativeSource => "proof-issuer-authoritative-source",
            Self::ParityCriticalIgnoredResults => "parity-critical-ignored-results",
            Self::ActorOwnedTaskSpawn => "actor-owned-task-spawn",
            Self::AsyncSessionOwnership => "async-session-ownership",
            Self::FrontendSemanticHandoffBoundary => "frontend-semantic-handoff-boundary",
            Self::HarnessMoveOwnershipBoundary => "harness-move-ownership-boundary",
            Self::HarnessReadinessOwnership => "harness-readiness-ownership",
            Self::HarnessRecoveryOwnership => "harness-recovery-ownership",
            Self::OptionalOwnerBoundary => "optional-owner-boundary",
            Self::TimeoutPolicyBoundary => "timeout-policy-boundary",
            Self::TimeDomainUsage => "time-domain-usage",
            Self::AuthoritativeRefNoReresolution => "authoritative-ref-no-reresolution",
            Self::WeakToStrongIdentifierUpgrade => "weak-to-strong-identifier-upgrade",
        }
    }
}

struct ParsedRustFile {
    path: PathBuf,
    source: String,
    syntax: File,
}

type StrongReferenceRegistry = HashMap<String, String>;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mode = args
        .next()
        .ok_or_else(|| "usage: ownership_lints <lint-mode> <path> [<path> ...]".to_string())
        .and_then(|value| LintMode::parse(&value))?;
    let paths = args.map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("expected at least one path to scan".to_string());
    }

    let mut rust_files = Vec::new();
    for path in &paths {
        collect_rust_files(path, &mut rust_files)?;
    }
    rust_files.sort();
    rust_files.dedup();

    let mut parsed_files = Vec::new();
    for file in &rust_files {
        let source = fs::read_to_string(file)
            .map_err(|error| format!("failed to read {}: {error}", file.display()))?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("failed to parse {}: {error}", file.display()))?;
        parsed_files.push(ParsedRustFile {
            path: file.clone(),
            source,
            syntax,
        });
    }

    let strong_references = collect_strong_reference_registry(&parsed_files);

    let mut violations = Vec::new();
    for file in &parsed_files {
        violations.extend(scan_file(
            mode,
            &file.path,
            &file.source,
            &file.syntax,
            &strong_references,
        ));
    }

    if !violations.is_empty() {
        for violation in violations {
            eprintln!("{violation}");
        }
        return Err(match mode {
            LintMode::WorkflowNoViewReadsForDecisions => {
                "parity-critical workflow code still reads projections to make semantic decisions"
                    .to_string()
            }
            LintMode::WorkflowNoViewWrites => {
                "parity-critical workflow code still mutates projections directly".to_string()
            }
            LintMode::WorkflowNoFallbackDefaults => {
                "parity-critical workflow code still masks missing authoritative state with fallback defaults"
                    .to_string()
            }
            LintMode::WorkflowNoViewDerivedReadiness => {
                "authoritative readiness is still being derived from projections".to_string()
            }
            LintMode::WorkflowNoViewDerivedRecipientResolution => {
                "recipient resolution still depends on projected state".to_string()
            }
            LintMode::WorkflowUnboundedRuntimeAwaits => {
                "aura-app workflow/app code still contains direct runtime awaits outside explicit timeout wrappers"
                    .to_string()
            }
            LintMode::SemanticOwnerBoundedAwaits => {
                "semantic owner protocol violations remain in owner or handoff functions".to_string()
            }
            LintMode::BestEffortSideEffectBoundary => {
                "best-effort boundaries still own raw side effects or primary lifecycle publication".to_string()
            }
            LintMode::SemanticOwnerDetachedContinuation => {
                "semantic owners still launch detached continuation work after terminal publication"
                    .to_string()
            }
            LintMode::SemanticOwnerNoSpawn => {
                "semantic owners still spawn directly instead of using explicit child-operation ownership"
                    .to_string()
            }
            LintMode::SemanticOwnerProofSuccess => {
                "proof-bound semantic owners still publish plain success or omit typed proof-bearing success"
                    .to_string()
            }
            LintMode::WorkflowProofBearingSuccess => {
                "workflow code still publishes plain success directly instead of consuming typed postcondition proofs"
                    .to_string()
            }
            LintMode::ProofIssuerAuthoritativeSource => {
                "typed semantic success proofs are still minted outside #[authoritative_source(...)] helpers"
                    .to_string()
            }
            LintMode::ParityCriticalIgnoredResults => {
                "parity-critical helper results are still being discarded silently".to_string()
            }
            LintMode::ActorOwnedTaskSpawn => {
                "raw task spawning or async ownership escape hatches remain outside sanctioned modules"
                    .to_string()
            }
            LintMode::AsyncSessionOwnership => {
                "direct VM/session mutation bypasses runtime/session_ingress.rs".to_string()
            }
            LintMode::FrontendSemanticHandoffBoundary => {
                "frontend semantic handoff boundaries still expose bypass paths".to_string()
            }
            LintMode::HarnessMoveOwnershipBoundary => {
                "shared semantic move ownership escapes approved handle / receipt boundary modules"
                    .to_string()
            }
            LintMode::HarnessReadinessOwnership => {
                "frontend/harness modules author or refresh authoritative readiness outside approved coordinators"
                    .to_string()
            }
            LintMode::HarnessRecoveryOwnership => {
                "parity-critical observation code may not introduce sleeps, retries, or recovery helpers outside approved owner modules"
                    .to_string()
            }
            LintMode::OptionalOwnerBoundary => {
                "parity-critical boundaries still expose optional owner or spawner shapes"
                    .to_string()
            }
            LintMode::TimeoutPolicyBoundary => {
                "timeout policy boundary still exposes raw time primitives".to_string()
            }
            LintMode::TimeDomainUsage => {
                "semantic layers are using direct wall-clock time primitives instead of typed time domains"
                    .to_string()
            }
            LintMode::AuthoritativeRefNoReresolution => {
                "authoritative-ref-only functions still downgrade back to resolver or fallback helpers"
                    .to_string()
            }
            LintMode::WeakToStrongIdentifierUpgrade => {
                "weak identifiers still upgrade directly into canonical strong references or owned handles"
                    .to_string()
            }
        });
    }

    println!("{}: clean", mode.display_name());
    Ok(())
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        if path.extension() == Some(OsStr::new("rs")) {
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
            collect_rust_files(&entry_path, files)?;
        } else if entry_path.extension() == Some(OsStr::new("rs")) {
            files.push(entry_path);
        }
    }

    Ok(())
}

fn scan_file(
    mode: LintMode,
    file: &Path,
    source: &str,
    syntax: &File,
    strong_references: &StrongReferenceRegistry,
) -> Vec<String> {
    match mode {
        LintMode::ActorOwnedTaskSpawn => return scan_actor_owned_task_spawn(file, syntax),
        LintMode::AsyncSessionOwnership => return scan_async_session_ownership(file, source),
        LintMode::FrontendSemanticHandoffBoundary => {
            return scan_frontend_semantic_handoff_boundary(file, syntax);
        }
        LintMode::ProofIssuerAuthoritativeSource => {
            return scan_proof_issuer_authoritative_source(file, syntax);
        }
        LintMode::HarnessMoveOwnershipBoundary => {
            return scan_harness_move_ownership_boundary(file, source);
        }
        LintMode::HarnessReadinessOwnership => {
            return scan_harness_readiness_ownership(file, source);
        }
        LintMode::HarnessRecoveryOwnership => {
            return scan_harness_recovery_ownership(file, source);
        }
        LintMode::OptionalOwnerBoundary => {
            return scan_optional_owner_boundary(file, source);
        }
        LintMode::TimeoutPolicyBoundary => return scan_timeout_policy_boundary(file, syntax),
        LintMode::TimeDomainUsage => return scan_time_domain_usage(file, syntax),
        LintMode::AuthoritativeRefNoReresolution => {}
        LintMode::WeakToStrongIdentifierUpgrade => {
            return scan_weak_to_strong_identifier_upgrade(file, syntax, strong_references);
        }
        LintMode::WorkflowNoViewReadsForDecisions
        | LintMode::WorkflowNoViewWrites
        | LintMode::WorkflowNoFallbackDefaults
        | LintMode::WorkflowNoViewDerivedReadiness
        | LintMode::WorkflowNoViewDerivedRecipientResolution
        | LintMode::WorkflowUnboundedRuntimeAwaits
        | LintMode::SemanticOwnerBoundedAwaits
        | LintMode::BestEffortSideEffectBoundary
        | LintMode::SemanticOwnerDetachedContinuation
        | LintMode::SemanticOwnerNoSpawn
        | LintMode::SemanticOwnerProofSuccess
        | LintMode::WorkflowProofBearingSuccess
        | LintMode::ParityCriticalIgnoredResults => {}
    }

    let mut violations = Vec::new();
    for item in &syntax.items {
        scan_item(mode, file, source, item, &mut violations);
    }
    violations
}

fn collect_strong_reference_registry(files: &[ParsedRustFile]) -> StrongReferenceRegistry {
    let mut registry = StrongReferenceRegistry::new();
    for file in files {
        collect_strong_reference_items(&file.syntax.items, &mut registry);
    }
    registry
}

fn collect_strong_reference_items(items: &[Item], registry: &mut StrongReferenceRegistry) {
    for item in items {
        match item {
            Item::Struct(item_struct) => {
                if let Some(domain) = strong_reference_domain(&item_struct.attrs) {
                    registry.insert(item_struct.ident.to_string(), domain);
                }
            }
            Item::Enum(item_enum) => {
                if let Some(domain) = strong_reference_domain(&item_enum.attrs) {
                    registry.insert(item_enum.ident.to_string(), domain);
                }
            }
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    collect_strong_reference_items(nested, registry);
                }
            }
            _ => {}
        }
    }
}

fn strong_reference_domain(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        let segment = attr.path().segments.last()?;
        if segment.ident != "strong_reference" {
            return None;
        }
        let metas = attr
            .parse_args_with(
                syn::punctuated::Punctuated::<MetaNameValue, Token![,]>::parse_terminated,
            )
            .ok()?;
        metas.into_iter().find_map(|meta| {
            if !meta.path.is_ident("domain") {
                return None;
            }
            match meta.value {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(value),
                    ..
                }) => Some(value.value()),
                _ => None,
            }
        })
    })
}

fn scan_weak_to_strong_identifier_upgrade(
    file: &Path,
    syntax: &File,
    strong_references: &StrongReferenceRegistry,
) -> Vec<String> {
    if !file.to_string_lossy().contains("crates/") {
        return Vec::new();
    }

    let mut violations = Vec::new();
    for item in &syntax.items {
        scan_weak_to_strong_item(file, item, strong_references, &mut violations);
    }
    violations
}

fn scan_weak_to_strong_item(
    file: &Path,
    item: &Item,
    strong_references: &StrongReferenceRegistry,
    violations: &mut Vec<String>,
) {
    match item {
        Item::Fn(item_fn) => {
            if has_cfg_test_attr(&item_fn.attrs) || has_test_attr(&item_fn.attrs) {
                return;
            }
            scan_weak_to_strong_signature(
                file,
                &item_fn.sig.ident.to_string(),
                item_fn.sig.ident.span().start().line,
                &item_fn.sig.inputs,
                &item_fn.sig.output,
                strong_references,
                violations,
            );
        }
        Item::Impl(item_impl) => {
            for impl_item in &item_impl.items {
                if let ImplItem::Fn(item_fn) = impl_item {
                    if has_cfg_test_attr(&item_fn.attrs) || has_test_attr(&item_fn.attrs) {
                        continue;
                    }
                    scan_weak_to_strong_signature(
                        file,
                        &item_fn.sig.ident.to_string(),
                        item_fn.sig.ident.span().start().line,
                        &item_fn.sig.inputs,
                        &item_fn.sig.output,
                        strong_references,
                        violations,
                    );
                }
            }
        }
        Item::Mod(item_mod) => {
            if let Some((_, nested)) = &item_mod.content {
                for nested_item in nested {
                    scan_weak_to_strong_item(file, nested_item, strong_references, violations);
                }
            }
        }
        _ => {}
    }
}

fn scan_weak_to_strong_signature(
    file: &Path,
    function_name: &str,
    line: usize,
    inputs: &syn::punctuated::Punctuated<FnArg, Token![,]>,
    output: &ReturnType,
    strong_references: &StrongReferenceRegistry,
    violations: &mut Vec<String>,
) {
    if !is_upgrade_shaped_function(function_name) {
        return;
    }

    let Some(domain) = strong_reference_return_domain(output, strong_references) else {
        return;
    };

    let weak_params = inputs
        .iter()
        .filter_map(|arg| weak_identifier_parameter(arg, &domain))
        .collect::<Vec<_>>();
    if weak_params.is_empty() {
        return;
    }

    violations.push(format!(
        "{}:{}: function `{}` upgrades weak {} input(s) [{}] into strong `{}` truth",
        file.display(),
        line,
        function_name,
        domain,
        weak_params.join(", "),
        domain
    ));
}

fn is_upgrade_shaped_function(function_name: &str) -> bool {
    function_name.starts_with("resolve_")
        || function_name.contains("_by_id")
        || function_name.starts_with("current_")
}

fn strong_reference_return_domain(
    output: &ReturnType,
    strong_references: &StrongReferenceRegistry,
) -> Option<String> {
    match output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => strong_reference_domain_for_type(ty, strong_references),
    }
}

fn strong_reference_domain_for_type(
    ty: &Type,
    strong_references: &StrongReferenceRegistry,
) -> Option<String> {
    match ty {
        Type::Path(type_path) => {
            let segment = type_path.path.segments.last()?;
            let ident = segment.ident.to_string();
            if let Some(domain) = strong_references.get(&ident) {
                return Some(domain.clone());
            }
            match &segment.arguments {
                syn::PathArguments::AngleBracketed(arguments) => {
                    arguments.args.iter().find_map(|arg| match arg {
                        syn::GenericArgument::Type(inner) => {
                            strong_reference_domain_for_type(inner, strong_references)
                        }
                        _ => None,
                    })
                }
                _ => None,
            }
        }
        Type::Reference(reference) => {
            strong_reference_domain_for_type(&reference.elem, strong_references)
        }
        Type::Paren(paren) => strong_reference_domain_for_type(&paren.elem, strong_references),
        Type::Group(group) => strong_reference_domain_for_type(&group.elem, strong_references),
        _ => None,
    }
}

fn weak_identifier_parameter(arg: &FnArg, domain: &str) -> Option<String> {
    let FnArg::Typed(pat_type) = arg else {
        return None;
    };
    let param_name = typed_pat_name(pat_type)?;
    if !is_weak_identifier_for_domain(domain, &param_name, &pat_type.ty) {
        return None;
    }
    Some(format!("{param_name}: {}", pat_type.ty.to_token_stream()))
}

fn typed_pat_name(pat_type: &syn::PatType) -> Option<String> {
    match pat_type.pat.as_ref() {
        Pat::Ident(ident) => Some(ident.ident.to_string()),
        _ => None,
    }
}

fn is_weak_identifier_for_domain(domain: &str, param_name: &str, ty: &Type) -> bool {
    match domain {
        "channel" => {
            param_name.contains("channel")
                && (type_mentions_ident(ty, "ChannelId") || type_is_string_like(ty))
        }
        "invitation" => {
            param_name.contains("invitation")
                && (type_mentions_ident(ty, "InvitationId") || type_is_string_like(ty))
        }
        "ceremony" => {
            param_name.contains("ceremony")
                && (type_mentions_ident(ty, "CeremonyId") || type_is_string_like(ty))
        }
        "home" => {
            param_name.contains("home")
                && (type_mentions_ident(ty, "ChannelId") || type_mentions_ident(ty, "HomeId"))
        }
        "home_scope" => {
            (param_name.contains("channel")
                || param_name.contains("home")
                || param_name.contains("hint"))
                && (type_mentions_ident(ty, "ChannelId")
                    || type_mentions_ident(ty, "HomeId")
                    || type_is_string_like(ty))
        }
        _ => false,
    }
}

fn type_mentions_ident(ty: &Type, expected: &str) -> bool {
    match ty {
        Type::Path(type_path) => type_path.path.segments.iter().any(|segment| {
            segment.ident == expected
                || match &segment.arguments {
                    syn::PathArguments::AngleBracketed(arguments) => {
                        arguments.args.iter().any(|arg| match arg {
                            syn::GenericArgument::Type(inner) => {
                                type_mentions_ident(inner, expected)
                            }
                            _ => false,
                        })
                    }
                    _ => false,
                }
        }),
        Type::Reference(reference) => type_mentions_ident(&reference.elem, expected),
        Type::Paren(paren) => type_mentions_ident(&paren.elem, expected),
        Type::Group(group) => type_mentions_ident(&group.elem, expected),
        _ => false,
    }
}

fn type_is_string_like(ty: &Type) -> bool {
    match ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "String"),
        Type::Reference(reference) => match reference.elem.as_ref() {
            Type::Path(type_path) => type_path.path.is_ident("str"),
            _ => false,
        },
        Type::Paren(paren) => type_is_string_like(&paren.elem),
        Type::Group(group) => type_is_string_like(&group.elem),
        _ => false,
    }
}

fn scan_item(mode: LintMode, file: &Path, source: &str, item: &Item, violations: &mut Vec<String>) {
    match item {
        Item::Fn(item_fn) => {
            if has_cfg_test_attr(&item_fn.attrs) {
                return;
            }
            let function = ScannedFunction {
                attrs: &item_fn.attrs,
                function_name: item_fn.sig.ident.to_string(),
                function_line: item_fn.sig.ident.span().start().line,
                block: &item_fn.block,
            };
            scan_function(mode, file, source, function, violations)
        }
        Item::Impl(item_impl) => {
            for impl_item in &item_impl.items {
                if let ImplItem::Fn(item_fn) = impl_item {
                    if has_cfg_test_attr(&item_fn.attrs) {
                        continue;
                    }
                    scan_impl_function(mode, file, source, item_fn, violations);
                }
            }
        }
        Item::Mod(item_mod) => {
            if has_cfg_test_attr(&item_mod.attrs) {
                return;
            }
            if let Some((_, items)) = &item_mod.content {
                for nested in items {
                    scan_item(mode, file, source, nested, violations);
                }
            }
        }
        _ => {}
    }
}

fn scan_impl_function(
    mode: LintMode,
    file: &Path,
    source: &str,
    item_fn: &ImplItemFn,
    violations: &mut Vec<String>,
) {
    let function = ScannedFunction {
        attrs: &item_fn.attrs,
        function_name: item_fn.sig.ident.to_string(),
        function_line: item_fn.sig.ident.span().start().line,
        block: &item_fn.block,
    };
    scan_function(mode, file, source, function, violations);
}

struct ScannedFunction<'a> {
    attrs: &'a [syn::Attribute],
    function_name: String,
    function_line: usize,
    block: &'a Block,
}

fn scan_proof_issuer_authoritative_source(file: &Path, syntax: &File) -> Vec<String> {
    if !file
        .to_string_lossy()
        .contains("crates/aura-app/src/workflows/")
    {
        return Vec::new();
    }

    let mut violations = Vec::new();
    for item in &syntax.items {
        match item {
            Item::Fn(item_fn) => collect_proof_issuer_violations(
                file,
                &item_fn.sig.ident.to_string(),
                &item_fn.sig.output,
                &item_fn.attrs,
                &mut violations,
            ),
            Item::Impl(item_impl) => {
                for impl_item in &item_impl.items {
                    if let ImplItem::Fn(method) = impl_item {
                        collect_proof_issuer_violations(
                            file,
                            &method.sig.ident.to_string(),
                            &method.sig.output,
                            &method.attrs,
                            &mut violations,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    violations
}

fn collect_proof_issuer_violations(
    file: &Path,
    function_name: &str,
    output: &ReturnType,
    attrs: &[syn::Attribute],
    violations: &mut Vec<String>,
) {
    if !looks_like_proof_issuer(function_name, output) || has_cfg_test_attr(attrs) {
        return;
    }
    if has_marker_attr(attrs, "authoritative_source") {
        return;
    }

    let start = match output {
        ReturnType::Default => attrs
            .last()
            .map(|attr| attr.span().start())
            .unwrap_or_else(|| Span::call_site().start()),
        ReturnType::Type(_, ty) => ty.span().start(),
    };
    violations.push(format!(
        "{}:{}:{}: proof issuer `{}` must be declared with #[authoritative_source(...)]",
        file.display(),
        start.line,
        start.column + 1,
        function_name
    ));
}

fn looks_like_proof_issuer(function_name: &str, output: &ReturnType) -> bool {
    if function_name.starts_with("issue_") && function_name.ends_with("_proof") {
        return true;
    }
    if function_name.starts_with("prove_") {
        return true;
    }

    match output {
        ReturnType::Default => false,
        ReturnType::Type(_, ty) => {
            let rendered = ty.to_token_stream().to_string();
            rendered.contains("Proof") && !rendered.contains("Capability")
        }
    }
}

fn scan_function(
    mode: LintMode,
    file: &Path,
    source: &str,
    function: ScannedFunction<'_>,
    violations: &mut Vec<String>,
) {
    let ScannedFunction {
        attrs,
        function_name,
        function_line,
        block,
    } = function;
    let contains_handoff = function_contains_call(block, "handoff_to_app_workflow");
    let should_scan = match mode {
        LintMode::WorkflowNoViewReadsForDecisions
        | LintMode::WorkflowNoViewWrites
        | LintMode::WorkflowNoFallbackDefaults
        | LintMode::WorkflowNoViewDerivedReadiness
        | LintMode::WorkflowNoViewDerivedRecipientResolution => {
            file.to_string_lossy()
                .contains("crates/aura-app/src/workflows/")
                && !has_marker_attr(attrs, "observed_only")
        }
        LintMode::WorkflowUnboundedRuntimeAwaits => {
            let file_path = file.to_string_lossy();
            let file_name = file.file_name().and_then(OsStr::to_str);
            ((file_path.contains("crates/aura-app/src/workflows/")
                && !matches!(file_name, Some("runtime.rs" | "time.rs")))
                || file_path.ends_with("crates/aura-app/src/core/app.rs")
                || (file_path.contains("crates/aura-terminal/src/tui/")
                    && !matches!(file_name, Some("runtime.rs" | "iocraft_adapter.rs")))
                || file_path.contains("crates/aura-web/src/")
                || file_path.contains("crates/aura-ui/src/"))
                && !has_marker_attr(attrs, "observed_only")
        }
        LintMode::SemanticOwnerBoundedAwaits => {
            has_marker_attr(attrs, "semantic_owner") || contains_handoff
        }
        LintMode::BestEffortSideEffectBoundary => {
            has_marker_attr(attrs, "best_effort_boundary")
                || function_name.starts_with("best_effort_")
        }
        LintMode::SemanticOwnerDetachedContinuation => has_marker_attr(attrs, "semantic_owner"),
        LintMode::SemanticOwnerNoSpawn => has_marker_attr(attrs, "semantic_owner"),
        LintMode::SemanticOwnerProofSuccess => semantic_owner_declares_proof(attrs),
        LintMode::WorkflowProofBearingSuccess => {
            file.to_string_lossy()
                .contains("crates/aura-app/src/workflows/")
                && file
                    .file_name()
                    .is_none_or(|name| name != "semantic_facts.rs")
        }
        LintMode::ProofIssuerAuthoritativeSource => false,
        LintMode::ParityCriticalIgnoredResults => {
            has_marker_attr(attrs, "semantic_owner")
                || has_marker_attr(attrs, "best_effort_boundary")
        }
        LintMode::AuthoritativeRefNoReresolution => file
            .to_string_lossy()
            .contains("crates/aura-app/src/workflows/"),
        LintMode::WeakToStrongIdentifierUpgrade => false,
        LintMode::ActorOwnedTaskSpawn
        | LintMode::AsyncSessionOwnership
        | LintMode::FrontendSemanticHandoffBoundary
        | LintMode::HarnessMoveOwnershipBoundary
        | LintMode::HarnessReadinessOwnership
        | LintMode::HarnessRecoveryOwnership
        | LintMode::OptionalOwnerBoundary
        | LintMode::TimeoutPolicyBoundary
        | LintMode::TimeDomainUsage => false,
    };
    if !should_scan {
        return;
    }

    let mut visitor = OwnershipVisitor {
        mode,
        file,
        source,
        function_name: &function_name,
        violations: Vec::new(),
        has_handoff: contains_handoff,
        bounded_runtime_wrapper_depth: 0,
        first_await_line: None,
        first_handoff_line: None,
        first_terminal_publication_line: None,
        best_effort_awaits: Vec::new(),
        declared_terminal_helpers: semantic_owner_terminal_helpers(attrs),
        requires_typed_success_proof: semantic_owner_declares_proof(attrs),
        found_proof_success_call: false,
        function_ownership_tags: ownership_tags_before_line(source, function_line),
    };
    visitor.visit_block(block);
    visitor.finish();
    violations.extend(visitor.violations);
}

fn has_marker_attr(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| {
        matches!(attr.style, AttrStyle::Outer)
            && attr
                .path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == name)
    })
}

fn has_cfg_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        matches!(attr.style, AttrStyle::Outer)
            && attr.path().is_ident("cfg")
            && attr.to_token_stream().to_string().contains("test")
    })
}

fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        matches!(attr.style, AttrStyle::Outer)
            && attr
                .path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "test")
    })
}

fn is_bounded_runtime_wrapper_call_name(call_name: &str) -> bool {
    matches!(
        call_name,
        "execute_with_runtime_timeout_budget"
            | "timeout_workflow_stage_with_deadline"
            | "with_runtime_timeout"
    )
}

fn ownership_tags_before_line(source: &str, line: usize) -> Vec<String> {
    if line == 0 {
        return Vec::new();
    }

    let lines = source.lines().collect::<Vec<_>>();
    let start = line.saturating_sub(8);
    let end = line.saturating_sub(1).min(lines.len());
    let mut tags = Vec::new();
    for candidate in &lines[start..end] {
        if let Some((_, tag)) = candidate.split_once("OWNERSHIP:") {
            tags.push(tag.trim().to_string());
        }
    }
    tags
}

fn ownership_tags_near_line(source: &str, line: usize) -> Vec<String> {
    if line == 0 {
        return Vec::new();
    }

    let lines = source.lines().collect::<Vec<_>>();
    let start = line.saturating_sub(4);
    let end = line.min(lines.len());
    let mut tags = Vec::new();
    for candidate in &lines[start..end] {
        if let Some((_, tag)) = candidate.split_once("OWNERSHIP:") {
            tags.push(tag.trim().to_string());
        }
    }
    tags
}

struct OwnershipVisitor<'a> {
    mode: LintMode,
    file: &'a Path,
    source: &'a str,
    function_name: &'a str,
    violations: Vec<String>,
    has_handoff: bool,
    bounded_runtime_wrapper_depth: usize,
    first_await_line: Option<usize>,
    first_handoff_line: Option<usize>,
    first_terminal_publication_line: Option<usize>,
    best_effort_awaits: Vec<(Span, String)>,
    declared_terminal_helpers: Vec<String>,
    requires_typed_success_proof: bool,
    found_proof_success_call: bool,
    function_ownership_tags: Vec<String>,
}

impl OwnershipVisitor<'_> {
    fn is_finally_classified_exception(&self, span: Span) -> bool {
        let mut tags = self.function_ownership_tags.clone();
        tags.extend(ownership_tags_near_line(self.source, span.start().line));
        if tags.is_empty() {
            return false;
        }

        match self.mode {
            LintMode::WorkflowNoViewReadsForDecisions => tags.iter().any(|tag| {
                matches!(
                    tag.as_str(),
                    "observed"
                        | "observed-display-update"
                        | "authoritative-source"
                        | "first-run-default"
                        | "fact-backed"
                )
            }),
            LintMode::WorkflowNoViewWrites => tags
                .iter()
                .any(|tag| matches!(tag.as_str(), "observed-display-update" | "fact-backed")),
            LintMode::WorkflowNoFallbackDefaults => tags
                .iter()
                .any(|tag| matches!(tag.as_str(), "first-run-default")),
            LintMode::WorkflowNoViewDerivedReadiness
            | LintMode::WorkflowNoViewDerivedRecipientResolution => false,
            LintMode::WorkflowUnboundedRuntimeAwaits => false,
            _ => false,
        }
    }

    fn push_violation(&mut self, span: Span, message: String) {
        if self.is_finally_classified_exception(span) {
            return;
        }
        let start = span.start();
        self.violations.push(format!(
            "{}:{}:{}: {}",
            self.file.display(),
            start.line,
            start.column + 1,
            message
        ));
    }

    fn note_terminal_publication(&mut self, span: Span, call_name: &str, tokens: &str) {
        if is_terminal_publication_call(call_name, tokens, &self.declared_terminal_helpers) {
            let line = span.start().line;
            self.first_terminal_publication_line = Some(
                self.first_terminal_publication_line
                    .map_or(line, |existing| existing.min(line)),
            );
        }
    }

    fn finish(&mut self) {
        if self.mode == LintMode::SemanticOwnerBoundedAwaits && self.has_handoff {
            if let (Some(first_await_line), Some(handoff_line)) =
                (self.first_await_line, self.first_handoff_line)
            {
                if first_await_line < handoff_line {
                    self.violations.push(format!(
                        "{}:{}:{}: callback/workflow boundary `{}` awaits before canonical handoff",
                        self.file.display(),
                        first_await_line,
                        1,
                        self.function_name
                    ));
                }
            }
        }

        if self.mode == LintMode::SemanticOwnerBoundedAwaits {
            for (span, call_name) in self.best_effort_awaits.clone() {
                let await_line = span.start().line;
                let published_terminal = self
                    .first_terminal_publication_line
                    .is_some_and(|line| line <= await_line);
                if !published_terminal {
                    self.push_violation(
                        span,
                        format!(
                            "semantic owner `{}` awaits best-effort helper before terminal publication: {}",
                            self.function_name, call_name
                        ),
                    );
                }
            }
        }

        if self.mode == LintMode::SemanticOwnerProofSuccess
            && self.requires_typed_success_proof
            && !self.found_proof_success_call
        {
            self.violations.push(format!(
                "{}:1:1: proof-bound semantic owner `{}` never publishes success through publish_success_with(...)",
                self.file.display(),
                self.function_name
            ));
        }

        if self.mode == LintMode::WorkflowNoViewDerivedReadiness
            && self.function_name.contains("readiness")
            && self.violations.is_empty()
            && self.function_name.contains("authoritative")
        {}
    }

    fn check_post_terminal_call(&mut self, span: Span, call_name: &str, rendered: String) {
        if self.mode != LintMode::SemanticOwnerDetachedContinuation {
            return;
        }
        let line = span.start().line;
        let Some(terminal_line) = self.first_terminal_publication_line else {
            return;
        };
        if line <= terminal_line {
            return;
        }
        let normalized = call_name.replace(' ', "");
        let is_detached_continuation = normalized.starts_with("launch_")
            || normalized.starts_with("spawn_")
            || matches!(
                normalized.as_str(),
                "tokio::spawn" | "std::thread::spawn" | "thread::spawn" | "spawn_local"
            );
        if is_detached_continuation {
            self.push_violation(
                span,
                format!(
                    "semantic owner `{}` launches detached continuation after terminal publication: {}",
                    self.function_name, rendered
                ),
            );
        }
    }
}

impl<'ast> Visit<'ast> for OwnershipVisitor<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "handoff_to_app_workflow" {
            let line = node.span().start().line;
            self.first_handoff_line = Some(
                self.first_handoff_line
                    .map_or(line, |existing| existing.min(line)),
            );
        }

        let method_name = node.method.to_string();
        let tokens = node.to_token_stream().to_string();
        match self.mode {
            LintMode::WorkflowNoViewReadsForDecisions => {
                if method_name == "snapshot" {
                    self.push_violation(
                        node.span(),
                        format!(
                            "workflow `{}` reads snapshot() for semantic decisions: {}",
                            self.function_name, tokens
                        ),
                    );
                }
            }
            LintMode::WorkflowNoViewWrites => {
                if method_name == "views_mut" {
                    self.push_violation(
                        node.span(),
                        format!(
                            "workflow `{}` mutates projection state directly: {}",
                            self.function_name, tokens
                        ),
                    );
                }
            }
            LintMode::WorkflowNoFallbackDefaults => {
                if method_name == "unwrap_or_default"
                    || (method_name == "unwrap_or" && tokens.contains("( 0"))
                {
                    self.push_violation(
                        node.span(),
                        format!(
                            "workflow `{}` masks missing authoritative state with fallback default: {}",
                            self.function_name, tokens
                        ),
                    );
                }
            }
            LintMode::WorkflowNoViewDerivedReadiness => {
                if self.function_name.contains("readiness")
                    && (method_name == "snapshot" || method_name == "views_mut")
                {
                    self.push_violation(
                        node.span(),
                        format!(
                            "readiness workflow `{}` depends on projected state: {}",
                            self.function_name, tokens
                        ),
                    );
                }
            }
            LintMode::WorkflowNoViewDerivedRecipientResolution => {
                if (self.function_name.contains("recipient")
                    || self.function_name.contains("delivery"))
                    && (method_name == "snapshot" || method_name == "views_mut")
                {
                    self.push_violation(
                        node.span(),
                        format!(
                            "recipient/delivery workflow `{}` depends on projected state: {}",
                            self.function_name, tokens
                        ),
                    );
                }
            }
            LintMode::AuthoritativeRefNoReresolution => {
                if self
                    .function_ownership_tags
                    .iter()
                    .any(|tag| tag == "authoritative-ref-only")
                    && (method_name.contains("_or_fallback") || method_name.contains("fallback"))
                {
                    self.push_violation(
                        node.span(),
                        format!(
                            "authoritative-ref-only workflow `{}` may not use fallback helpers: {}",
                            self.function_name, tokens
                        ),
                    );
                }
            }
            _ => {}
        }
        self.note_terminal_publication(node.span(), &method_name, &tokens);
        self.check_post_terminal_call(node.span(), &method_name, tokens.clone());
        if self.mode == LintMode::SemanticOwnerProofSuccess {
            if method_name == "publish_success_with" {
                self.found_proof_success_call = true;
            }
            if method_name == "publish_phase"
                && tokens.contains("SemanticOperationPhase :: Succeeded")
            {
                self.push_violation(
                    node.span(),
                    format!(
                        "proof-bound semantic owner `{}` publishes plain success instead of publish_success_with(...): {}",
                        self.function_name, tokens
                    ),
                );
            }
        }
        if self.mode == LintMode::WorkflowProofBearingSuccess
            && method_name == "publish_phase"
            && tokens.contains("SemanticOperationPhase :: Succeeded")
        {
            self.push_violation(
                node.span(),
                format!(
                    "workflow `{}` publishes plain success directly instead of proof-bearing success: {}",
                    self.function_name, tokens
                ),
            );
        }
        if self.mode == LintMode::BestEffortSideEffectBoundary {
            if is_primary_lifecycle_publication_name(
                &method_name,
                &tokens,
                &self.declared_terminal_helpers,
            ) {
                self.push_violation(
                    node.span(),
                    format!(
                        "best-effort function `{}` publishes primary lifecycle directly: {}",
                        self.function_name, tokens
                    ),
                );
            } else if is_forbidden_best_effort_call_name(
                &method_name,
                &tokens,
                &self.declared_terminal_helpers,
            ) {
                self.push_violation(
                    node.span(),
                    format!(
                        "best-effort function `{}` performs parity-critical work directly: {}",
                        self.function_name, tokens
                    ),
                );
            }
        }

        if self.mode == LintMode::WorkflowUnboundedRuntimeAwaits
            && is_bounded_runtime_wrapper_call_name(&method_name)
        {
            visit::visit_expr(&mut *self, &node.receiver);
            self.bounded_runtime_wrapper_depth += 1;
            for arg in &node.args {
                visit::visit_expr(&mut *self, arg);
            }
            self.bounded_runtime_wrapper_depth =
                self.bounded_runtime_wrapper_depth.saturating_sub(1);
            return;
        }

        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Some(call_name) = expr_call_name(&node.func) {
            let tokens = node.to_token_stream().to_string();
            match self.mode {
                LintMode::WorkflowNoViewReadsForDecisions => {
                    if is_view_read_call_name(&call_name) {
                        self.push_violation(
                            node.span(),
                            format!(
                                "workflow `{}` reads projection helper for semantic decisions: {}",
                                self.function_name, tokens
                            ),
                        );
                    }
                }
                LintMode::WorkflowNoViewWrites => {
                    if is_view_write_call_name(&call_name) {
                        self.push_violation(
                            node.span(),
                            format!(
                                "workflow `{}` mutates projection helper directly: {}",
                                self.function_name, tokens
                            ),
                        );
                    }
                }
                LintMode::WorkflowNoFallbackDefaults => {
                    if is_fallback_heuristic_call_name(&call_name)
                        || tokens.contains("unwrap_or_else ( Vec :: new")
                    {
                        self.push_violation(
                            node.span(),
                            format!(
                                "workflow `{}` masks missing authoritative state with fallback heuristic: {}",
                                self.function_name, tokens
                            ),
                        );
                    }
                }
                LintMode::WorkflowNoViewDerivedReadiness => {
                    if self.function_name.contains("readiness")
                        && (is_view_read_call_name(&call_name)
                            || is_view_write_call_name(&call_name))
                    {
                        self.push_violation(
                            node.span(),
                            format!(
                                "readiness workflow `{}` depends on projected state: {}",
                                self.function_name, tokens
                            ),
                        );
                    }
                }
                LintMode::WorkflowNoViewDerivedRecipientResolution => {
                    if (self.function_name.contains("recipient")
                        || self.function_name.contains("delivery"))
                        && (is_view_read_call_name(&call_name)
                            || is_view_write_call_name(&call_name))
                    {
                        self.push_violation(
                            node.span(),
                            format!(
                                "recipient/delivery workflow `{}` depends on projected state: {}",
                                self.function_name, tokens
                            ),
                        );
                    }
                }
                LintMode::AuthoritativeRefNoReresolution => {
                    if self
                        .function_ownership_tags
                        .iter()
                        .any(|tag| tag == "authoritative-ref-only")
                        && (is_authority_downgrade_call_name(&call_name)
                            || call_name.contains("_or_fallback")
                            || call_name.contains("fallback"))
                    {
                        self.push_violation(
                            node.span(),
                            format!(
                                "authoritative-ref-only workflow `{}` re-derives stronger truth from weaker inputs: {}",
                                self.function_name, tokens
                            ),
                        );
                    }
                }
                LintMode::WorkflowUnboundedRuntimeAwaits => {}
                _ => {}
            }
            self.note_terminal_publication(node.span(), &call_name, &tokens);
            self.check_post_terminal_call(node.span(), &call_name, tokens.clone());

            if self.mode == LintMode::BestEffortSideEffectBoundary {
                if is_primary_lifecycle_publication_name(
                    &call_name,
                    &tokens,
                    &self.declared_terminal_helpers,
                ) {
                    self.push_violation(
                        node.span(),
                        format!(
                            "best-effort function `{}` publishes primary lifecycle directly: {}",
                            self.function_name,
                            node.to_token_stream()
                        ),
                    );
                } else if is_forbidden_best_effort_call_name(
                    &call_name,
                    &tokens,
                    &self.declared_terminal_helpers,
                ) {
                    self.push_violation(
                        node.span(),
                        format!(
                            "best-effort function `{}` performs parity-critical work directly: {}",
                            self.function_name,
                            node.to_token_stream()
                        ),
                    );
                }
            }
            if self.mode == LintMode::WorkflowProofBearingSuccess
                && call_name.starts_with("publish_")
                && tokens.contains("SemanticOperationPhase :: Succeeded")
            {
                self.push_violation(
                    node.span(),
                    format!(
                        "workflow `{}` publishes plain success directly instead of proof-bearing success: {}",
                        self.function_name, tokens
                    ),
                );
            }

            if self.mode == LintMode::WorkflowUnboundedRuntimeAwaits
                && is_bounded_runtime_wrapper_call_name(&call_name)
            {
                visit::visit_expr(&mut *self, &node.func);
                self.bounded_runtime_wrapper_depth += 1;
                for arg in &node.args {
                    visit::visit_expr(&mut *self, arg);
                }
                self.bounded_runtime_wrapper_depth =
                    self.bounded_runtime_wrapper_depth.saturating_sub(1);
                return;
            }
        }

        visit::visit_expr_call(self, node);
    }

    fn visit_expr_await(&mut self, node: &'ast ExprAwait) {
        let line = node.span().start().line;
        self.first_await_line = Some(
            self.first_await_line
                .map_or(line, |existing| existing.min(line)),
        );

        match self.mode {
            LintMode::SemanticOwnerBoundedAwaits => {
                if let Some(method_call) = method_call_on_ident(&node.base, "runtime") {
                    self.push_violation(
                        node.span(),
                        format!(
                            "raw runtime await inside semantic owner `{}`: {}",
                            self.function_name,
                            method_call.to_token_stream()
                        ),
                    );
                }
                if let Some(method_call) = method_call_on_ident(&node.base, "effects") {
                    self.push_violation(
                        node.span(),
                        format!(
                            "raw effects await inside semantic owner `{}`: {}",
                            self.function_name,
                            method_call.to_token_stream()
                        ),
                    );
                }
                if let Some(call_name) = awaited_call_name(&node.base) {
                    if call_name.starts_with("best_effort_") {
                        self.best_effort_awaits.push((node.span(), call_name));
                    }
                }
            }
            LintMode::BestEffortSideEffectBoundary => {
                if let Some(method_call) = method_call_on_ident(&node.base, "effects") {
                    let method_name = method_call.method.to_string();
                    if matches!(
                        method_name.as_str(),
                        "send_envelope" | "join_channel" | "create_channel"
                    ) {
                        self.push_violation(
                            node.span(),
                            format!(
                                "raw awaited side effect inside best-effort function `{}`: {}",
                                self.function_name,
                                method_call.to_token_stream()
                            ),
                        );
                    }
                }
                if let Some(call_name) = awaited_call_name(&node.base) {
                    if is_primary_lifecycle_publication_name(
                        &call_name,
                        &node.base.to_token_stream().to_string(),
                        &self.declared_terminal_helpers,
                    ) {
                        self.push_violation(
                            node.span(),
                            format!(
                                "best-effort function `{}` awaits primary lifecycle publication directly: {}",
                                self.function_name, call_name
                            ),
                        );
                    }
                }
            }
            LintMode::SemanticOwnerNoSpawn => {
                if let Some(call_name) = awaited_call_name(&node.base) {
                    if is_forbidden_owner_spawn_name(&call_name) {
                        self.push_violation(
                            node.span(),
                            format!(
                                "semantic owner `{}` spawns directly instead of using explicit child operations: {}",
                                self.function_name, call_name
                            ),
                        );
                    }
                }
            }
            LintMode::WorkflowUnboundedRuntimeAwaits => {
                if self.bounded_runtime_wrapper_depth == 0 {
                    if let Some(method_call) = method_call_on_ident(&node.base, "runtime") {
                        if method_call.method != "sleep_ms" {
                            self.push_violation(
                                node.span(),
                                format!(
                                    "workflow/app function `{}` awaits runtime directly without an explicit timeout wrapper: {}",
                                    self.function_name,
                                    method_call.to_token_stream()
                                ),
                            );
                        }
                    }
                }
            }
            LintMode::WorkflowNoViewReadsForDecisions
            | LintMode::WorkflowNoViewWrites
            | LintMode::WorkflowNoFallbackDefaults
            | LintMode::WorkflowNoViewDerivedReadiness
            | LintMode::WorkflowNoViewDerivedRecipientResolution
            | LintMode::SemanticOwnerProofSuccess
            | LintMode::WorkflowProofBearingSuccess
            | LintMode::ProofIssuerAuthoritativeSource
            | LintMode::ParityCriticalIgnoredResults => {}
            LintMode::ActorOwnedTaskSpawn
            | LintMode::SemanticOwnerDetachedContinuation
            | LintMode::AsyncSessionOwnership
            | LintMode::FrontendSemanticHandoffBoundary
            | LintMode::HarnessMoveOwnershipBoundary
            | LintMode::HarnessReadinessOwnership
            | LintMode::HarnessRecoveryOwnership
            | LintMode::OptionalOwnerBoundary
            | LintMode::TimeoutPolicyBoundary
            | LintMode::TimeDomainUsage
            | LintMode::AuthoritativeRefNoReresolution
            | LintMode::WeakToStrongIdentifierUpgrade => {}
        }

        visit::visit_expr_await(self, node);
    }

    fn visit_local(&mut self, node: &'ast Local) {
        if self.mode == LintMode::ParityCriticalIgnoredResults
            && matches!(&node.pat, Pat::Wild(_))
            && node
                .init
                .as_ref()
                .and_then(|init| ignored_result_call_name(&init.expr))
                .is_some_and(|call_name| is_parity_critical_call_name(&call_name))
        {
            let call_name = node
                .init
                .as_ref()
                .and_then(|init| ignored_result_call_name(&init.expr))
                .unwrap_or_else(|| "<unknown>".to_string());
            self.push_violation(
                node.span(),
                format!(
                    "parity-critical result discarded in `{}`: {}",
                    self.function_name, call_name
                ),
            );
        }
        visit::visit_local(self, node);
    }

    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        if self.mode == LintMode::ParityCriticalIgnoredResults {
            let rendered = node.cond.to_token_stream().to_string();
            if rendered.contains("if let Err")
                && ignored_result_call_name(&node.cond)
                    .is_some_and(|call_name| is_parity_critical_call_name(&call_name))
            {
                let call_name =
                    ignored_result_call_name(&node.cond).unwrap_or_else(|| "<unknown>".to_string());
                self.push_violation(
                    node.span(),
                    format!(
                        "parity-critical error discarded in `{}`: {}",
                        self.function_name, call_name
                    ),
                );
            }
        }
        visit::visit_expr_if(self, node);
    }
}

fn function_contains_call(block: &Block, call_name: &str) -> bool {
    struct CallFinder<'a> {
        call_name: &'a str,
        found: bool,
    }

    impl<'ast> Visit<'ast> for CallFinder<'_> {
        fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
            if node.method == self.call_name {
                self.found = true;
            }
            visit::visit_expr_method_call(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if expr_call_name(&node.func).as_deref() == Some(self.call_name) {
                self.found = true;
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut finder = CallFinder {
        call_name,
        found: false,
    };
    finder.visit_block(block);
    finder.found
}

fn method_call_on_ident<'a>(expr: &'a Expr, receiver_ident: &str) -> Option<&'a ExprMethodCall> {
    let expr = strip_expression(expr);
    let Expr::MethodCall(method_call) = expr else {
        return None;
    };
    receiver_is_ident(&method_call.receiver, receiver_ident).then_some(method_call)
}

fn receiver_is_ident(expr: &Expr, expected_ident: &str) -> bool {
    match strip_expression(expr) {
        Expr::Path(path) => path.path.is_ident(expected_ident),
        _ => false,
    }
}

fn awaited_call_name(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Call(expr_call) => expr_call_name(&expr_call.func),
        Expr::MethodCall(method_call) => Some(method_call.method.to_string()),
        _ => None,
    }
}

fn ignored_result_call_name(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Await(expr_await) => awaited_call_name(&expr_await.base),
        Expr::Call(expr_call) => expr_call_name(&expr_call.func),
        Expr::MethodCall(method_call) => Some(method_call.method.to_string()),
        _ => None,
    }
}

fn expr_call_name(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn is_view_read_call_name(call_name: &str) -> bool {
    matches!(
        call_name,
        "chat_snapshot"
            | "contacts_snapshot"
            | "recovery_snapshot"
            | "fallback_home"
            | "homes_state_signal_fallback"
    )
}

fn is_view_write_call_name(call_name: &str) -> bool {
    matches!(
        call_name,
        "with_chat_state"
            | "with_homes_state"
            | "with_contacts_state"
            | "with_recovery_state"
            | "with_neighborhood_state"
    )
}

fn is_fallback_heuristic_call_name(call_name: &str) -> bool {
    matches!(call_name, "fallback_home" | "homes_state_signal_fallback")
}

fn is_authority_downgrade_call_name(call_name: &str) -> bool {
    matches!(
        call_name,
        "resolve_authoritative_context_id_for_channel"
            | "resolve_chat_channel_id_from_state_or_input"
            | "matching_chat_channel_ids"
            | "context_id_for_channel"
    ) || call_name.starts_with("resolve_")
}

fn semantic_owner_terminal_helpers(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| {
            matches!(attr.style, AttrStyle::Outer)
                && attr
                    .path()
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "semantic_owner")
        })
        .filter_map(|attr| {
            let metas = attr
                .parse_args_with(
                    syn::punctuated::Punctuated::<MetaNameValue, Token![,]>::parse_terminated,
                )
                .ok()?;
            metas.into_iter().find_map(|meta| {
                if !meta.path.is_ident("terminal") {
                    return None;
                }
                match meta.value {
                    Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(value),
                        ..
                    }) => Some(value.value()),
                    _ => None,
                }
            })
        })
        .collect()
}

fn semantic_owner_declares_proof(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        matches!(attr.style, AttrStyle::Outer)
            && attr
                .path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "semantic_owner")
            && attr
                .parse_args_with(
                    syn::punctuated::Punctuated::<MetaNameValue, Token![,]>::parse_terminated,
                )
                .ok()
                .is_some_and(|metas| metas.into_iter().any(|meta| meta.path.is_ident("proof")))
    })
}

fn is_primary_lifecycle_publication_name(
    name: &str,
    tokens: &str,
    declared_terminal_helpers: &[String],
) -> bool {
    if declared_terminal_helpers
        .iter()
        .any(|helper| helper == name)
    {
        return true;
    }

    matches!(name, "publish_phase" | "publish_failure")
        || (name.starts_with("publish_")
            && (tokens.contains("SemanticOperationPhase ::")
                || tokens.contains("SemanticOperationError")
                || name.contains("failure")))
}

fn is_terminal_publication_call(
    name: &str,
    tokens: &str,
    declared_terminal_helpers: &[String],
) -> bool {
    is_primary_lifecycle_publication_name(name, tokens, declared_terminal_helpers)
        && (name.contains("failure")
            || tokens.contains("SemanticOperationPhase :: Succeeded")
            || tokens.contains("SemanticOperationPhase :: Failed")
            || tokens.contains("SemanticOperationPhase :: Cancelled"))
}

fn is_forbidden_owner_spawn_name(name: &str) -> bool {
    let normalized = name.replace(' ', "");
    matches!(
        normalized.as_str(),
        "spawn"
            | "spawn_local"
            | "spawn_cancellable"
            | "spawn_local_cancellable"
            | "tokio::spawn"
            | "std::thread::spawn"
            | "thread::spawn"
    ) || normalized.starts_with("launch_")
}

fn is_forbidden_best_effort_call_name(
    name: &str,
    tokens: &str,
    declared_terminal_helpers: &[String],
) -> bool {
    let normalized = name.replace(' ', "");
    if normalized.starts_with("best_effort_") {
        return false;
    }

    is_primary_lifecycle_publication_name(name, tokens, declared_terminal_helpers)
        || is_parity_critical_call_name(name)
}

fn is_parity_critical_call_name(name: &str) -> bool {
    let normalized = name.replace(' ', "");
    let prefixes = [
        "accept_",
        "apply_authoritative_",
        "commit_",
        "create_",
        "ensure_",
        "join_",
        "leave_",
        "mark_",
        "materialize_",
        "project_",
        "publish_",
        "reconcile_",
        "refresh_authoritative_",
        "register_",
        "require_",
        "resolve_",
        "wait_for_",
        "warm_",
    ];
    prefixes.iter().any(|prefix| normalized.starts_with(prefix))
        || matches!(normalized.as_str(), "publish_phase" | "publish_failure")
}

fn strip_expression(mut expr: &Expr) -> &Expr {
    loop {
        expr = match expr {
            Expr::Group(ExprGroup { expr, .. })
            | Expr::Paren(ExprParen { expr, .. })
            | Expr::Reference(ExprReference { expr, .. }) => expr,
            _ => return expr,
        };
    }
}

const ACTOR_SPAWN_APPROVED_SUFFIXES: &[&str] = &[
    "crates/aura-agent/src/task_registry.rs",
    "crates/aura-agent/src/runtime/services/service_actor.rs",
    "crates/aura-effects/src/reactive/handler.rs",
    "crates/aura-effects/src/reactive/graph.rs",
    "crates/aura-harness/src/backend/local_pty.rs",
    "crates/aura-harness/src/backend/playwright_browser.rs",
    "crates/aura-harness/src/bin/tool_repl.rs",
    "crates/aura-harness/src/coordinator.rs",
    "crates/aura-harness/src/executor.rs",
    "crates/aura-terminal/src/tui/tasks.rs",
    "crates/aura-testkit/src/infrastructure/time.rs",
    "crates/aura-ui/src/task_owner.rs",
    "crates/aura-web/src/harness_bridge.rs",
    "crates/aura-web/src/main.rs",
    "crates/aura-web/src/task_owner.rs",
    "crates/aura-web/src/web_clipboard.rs",
];
const HARNESS_MOVE_APPROVED_SUFFIXES: &[&str] = &[
    "crates/aura-app/src/ui_contract.rs",
    "crates/aura-app/src/scenario_contract.rs",
    "crates/aura-app/src/workflows/harness_determinism.rs",
    "crates/aura-app/tests/ui/ui_operation_handle_private_fields.rs",
    "crates/aura-app/tests/ui/harness_ui_operation_handle_private_fields.rs",
    "crates/aura-harness/src/backend/local_pty.rs",
    "crates/aura-harness/src/backend/mod.rs",
    "crates/aura-harness/src/executor.rs",
    "crates/aura-terminal/src/tui/harness_state.rs",
    "crates/aura-terminal/src/tui/harness_state/mod.rs",
    "crates/aura-terminal/src/tui/semantic_lifecycle.rs",
    "crates/aura-terminal/src/tui/screens/app/shell.rs",
];

const FRONTEND_INTERNAL_OWNER_SUFFIXES: &[&str] =
    &["crates/aura-terminal/src/tui/semantic_lifecycle.rs"];
const FRONTEND_SUBMIT_SUFFIXES: &[&str] = &[
    "crates/aura-terminal/src/tui/screens/app/shell.rs",
    "crates/aura-terminal/src/tui/semantic_lifecycle.rs",
];
const FRONTEND_HANDOFF_SUFFIXES: &[&str] = &[
    "crates/aura-terminal/src/tui/callbacks/factories/chat.rs",
    "crates/aura-terminal/src/tui/callbacks/factories/contacts.rs",
    "crates/aura-terminal/src/tui/callbacks/factories/mod.rs",
    "crates/aura-terminal/src/tui/semantic_lifecycle.rs",
];
const FRONTEND_AUTHORITATIVE_STATE_SUFFIXES: &[&str] = &[
    "crates/aura-terminal/src/tui/screens/app/shell.rs",
    "crates/aura-terminal/src/tui/state/mod.rs",
    "crates/aura-terminal/src/tui/harness_state/mod.rs",
];

fn file_matches_suffix(file: &Path, suffixes: &[&str]) -> bool {
    let display = file.to_string_lossy();
    suffixes.iter().any(|suffix| display.ends_with(suffix))
}

fn call_path_string(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Path(path) => Some(path.path.to_token_stream().to_string()),
        _ => None,
    }
}

fn is_pub(vis: &Visibility) -> bool {
    !matches!(vis, Visibility::Inherited)
}

fn type_contains_join_handle(ty: &Type) -> bool {
    ty.to_token_stream().to_string().contains("JoinHandle")
}

fn return_type_contains_join_handle(return_type: &ReturnType) -> bool {
    match return_type {
        ReturnType::Default => false,
        ReturnType::Type(_, ty) => type_contains_join_handle(ty),
    }
}

fn scan_actor_owned_task_spawn(file: &Path, syntax: &File) -> Vec<String> {
    struct Visitor<'a> {
        file: &'a Path,
        violations: Vec<String>,
        approved_file: bool,
    }

    impl Visitor<'_> {
        fn push_violation(&mut self, span: Span, message: String) {
            let start = span.start();
            self.violations.push(format!(
                "{}:{}:{}: {}",
                self.file.display(),
                start.line,
                start.column + 1,
                message
            ));
        }

        fn visit_public_fn(&mut self, item_fn: &ItemFn) {
            if is_pub(&item_fn.vis) && return_type_contains_join_handle(&item_fn.sig.output) {
                self.push_violation(
                    item_fn.sig.output.span(),
                    "public parity-critical API exposes raw JoinHandle".to_string(),
                );
            }
        }

        fn visit_public_impl_fn(&mut self, item_fn: &ImplItemFn) {
            if is_pub(&item_fn.vis) && return_type_contains_join_handle(&item_fn.sig.output) {
                self.push_violation(
                    item_fn.sig.output.span(),
                    "public parity-critical API exposes raw JoinHandle".to_string(),
                );
            }
        }

        fn visit_public_struct(&mut self, item_struct: &ItemStruct) {
            for field in &item_struct.fields {
                if is_pub(&field.vis) && type_contains_join_handle(&field.ty) {
                    self.push_violation(
                        field.ty.span(),
                        "public parity-critical API exposes raw JoinHandle field".to_string(),
                    );
                }
            }
        }
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_item_fn(&mut self, node: &'ast ItemFn) {
            self.visit_public_fn(node);
            visit::visit_item_fn(self, node);
        }

        fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
            self.visit_public_impl_fn(node);
            visit::visit_impl_item_fn(self, node);
        }

        fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
            self.visit_public_struct(node);
            visit::visit_item_struct(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Some(path) = call_path_string(&node.func) {
                let path = path.replace(' ', "");
                let is_raw_spawn = matches!(
                    path.as_str(),
                    "tokio::spawn"
                        | "std::thread::spawn"
                        | "thread::spawn"
                        | "spawn_local"
                        | "spawn"
                );
                let is_unbounded = matches!(
                    path.as_str(),
                    "mpsc::unbounded_channel"
                        | "tokio::sync::mpsc::unbounded_channel"
                        | "mpsc::unbounded"
                );
                if !self.approved_file && is_raw_spawn {
                    self.push_violation(
                        node.span(),
                        format!("raw task spawn outside sanctioned owner module: {path}"),
                    );
                }
                if is_unbounded {
                    self.push_violation(
                        node.span(),
                        format!("unbounded channel in parity-critical module: {path}"),
                    );
                }
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut visitor = Visitor {
        file,
        violations: Vec::new(),
        approved_file: file_matches_suffix(file, ACTOR_SPAWN_APPROVED_SUFFIXES),
    };
    visitor.visit_file(syntax);
    visitor.violations
}

fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*')
}

fn source_line_violations(
    file: &Path,
    source: &str,
    patterns: &[&str],
    approved_suffixes: &[&str],
) -> Vec<String> {
    let approved_file = file_matches_suffix(file, approved_suffixes);
    let mut violations = Vec::new();

    for (index, line) in source.lines().enumerate() {
        if is_comment_line(line) {
            continue;
        }
        if patterns.iter().any(|pattern| line.contains(pattern)) && !approved_file {
            violations.push(format!(
                "{}:{}:1: forbidden ownership escape hatch: {}",
                file.display(),
                index + 1,
                line.trim()
            ));
        }
    }

    violations
}

fn scan_async_session_ownership(file: &Path, source: &str) -> Vec<String> {
    source_line_violations(
        file,
        source,
        &[
            "open_manifest_vm_session_admitted",
            "ChoreographicEffects::start_session(",
            "ChoreographicEffects::end_session(",
            "inject_vm_receive",
            "effects.start_session(",
            "self.effects.start_session(",
            "effects.end_session(",
            "self.effects.end_session(",
        ],
        &[],
    )
}

fn scan_harness_move_ownership_boundary(file: &Path, source: &str) -> Vec<String> {
    source_line_violations(
        file,
        source,
        &[
            "UiOperationHandle::new(",
            "HarnessUiOperationHandle::new(",
            "record_submission_handle(",
            "HarnessUiCommandReceipt::Accepted",
            "instance_id = Some(",
        ],
        HARNESS_MOVE_APPROVED_SUFFIXES,
    )
}

fn scan_harness_readiness_ownership(file: &Path, source: &str) -> Vec<String> {
    let mut violations = source_line_violations(
        file,
        source,
        &[
            "refresh_authoritative_invitation_readiness",
            "refresh_authoritative_contact_link_readiness",
            "refresh_authoritative_channel_membership_readiness",
            "refresh_authoritative_recipient_resolution_readiness",
            "refresh_authoritative_delivery_readiness",
            "publish_authoritative_semantic_fact(",
            "replace_authoritative_semantic_facts_of_kind(",
        ],
        &[],
    );
    violations.extend(scan_browser_shell_mutation_snapshot_boundary(file, source));
    violations.extend(scan_agent_channel_metadata_ownership(file, source));
    violations
}

fn scan_harness_recovery_ownership(file: &Path, source: &str) -> Vec<String> {
    source_line_violations(
        file,
        source,
        &[
            "std::thread::sleep",
            "thread::sleep",
            "tokio::time::sleep",
            "run_registered_recovery",
            "retry",
            "fallback",
        ],
        &[],
    )
}

fn scan_optional_owner_boundary(file: &Path, source: &str) -> Vec<String> {
    source_line_violations(
        file,
        source,
        &[
            "RefCell<Option<FrontendTaskOwner>>",
            "RefCell<Option<WebTaskOwner>>",
            "-> Option<OwnedTaskSpawner>",
            "-> Option<OwnedShutdownToken>",
            "Option<&SemanticWorkflowOwner>",
        ],
        &[],
    )
}

fn scan_browser_shell_mutation_snapshot_boundary(file: &Path, source: &str) -> Vec<String> {
    if !file_matches_suffix(
        file,
        &[
            "crates/aura-web/src/harness_bridge.rs",
            "crates/aura-web/src/main.rs",
        ],
    ) {
        return Vec::new();
    }

    let lines = source.lines().collect::<Vec<_>>();
    let mut violations = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if !trimmed.contains(".set_screen(") && !trimmed.contains(".set_settings_section(") {
            continue;
        }

        let start = index.saturating_sub(4);
        let end = usize::min(index + 5, lines.len());
        let window = &lines[start..end];
        let has_snapshot_publication = window.iter().any(|candidate| {
            candidate.contains("publish_current_ui_snapshot(")
                || candidate.contains(".set_ui_snapshot(")
        });
        let goes_through_owned_mutation_helper = window
            .iter()
            .any(|candidate| candidate.contains("schedule_browser_ui_mutation("));

        if !has_snapshot_publication && !goes_through_owned_mutation_helper {
            violations.push(format!(
                "{}:{}:1: browser shell mutation must publish the post-mutation UiSnapshot or go through schedule_browser_ui_mutation",
                file.display(),
                index + 1
            ));
        }
    }

    violations
}

fn scan_agent_channel_metadata_ownership(file: &Path, source: &str) -> Vec<String> {
    if !file_matches_suffix(
        file,
        &["crates/aura-agent/src/reactive/app_signal_views.rs"],
    ) {
        return Vec::new();
    }

    source_line_violations(
        file,
        source,
        &[
            "name.unwrap_or_else(|| channel_id.to_string())",
            "name: channel_id.to_string()",
        ],
        &[],
    )
}

fn scan_frontend_semantic_handoff_boundary(file: &Path, syntax: &File) -> Vec<String> {
    struct Visitor<'a> {
        file: &'a Path,
        violations: Vec<String>,
    }

    impl Visitor<'_> {
        fn push_violation(&mut self, span: Span, message: String) {
            let start = span.start();
            self.violations.push(format!(
                "{}:{}:{}: {}",
                self.file.display(),
                start.line,
                start.column + 1,
                message
            ));
        }
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_expr_path(&mut self, node: &'ast ExprPath) {
            let path = node.path.to_token_stream().to_string().replace(' ', "");
            if path.contains("SubmittedOperationOwner::")
                && !file_matches_suffix(self.file, FRONTEND_INTERNAL_OWNER_SUFFIXES)
            {
                self.push_violation(
                    node.span(),
                    "internal submitted-owner access escaped the sanctioned boundary".to_string(),
                );
            }
            visit::visit_expr_path(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Some(path) = call_path_string(&node.func) {
                let path = path.replace(' ', "");
                if path.contains("LocalTerminalOperationOwner::submit")
                    && !file_matches_suffix(self.file, FRONTEND_SUBMIT_SUFFIXES)
                {
                    self.push_violation(
                        node.span(),
                        "local semantic owner allocation escaped the sanctioned boundary"
                            .to_string(),
                    );
                }
                if path.contains("WorkflowHandoffOperationOwner::submit")
                    && !file_matches_suffix(self.file, FRONTEND_SUBMIT_SUFFIXES)
                {
                    self.push_violation(
                        node.span(),
                        "workflow handoff owner allocation escaped the sanctioned boundary"
                            .to_string(),
                    );
                }
            }
            visit::visit_expr_call(self, node);
        }

        fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
            if node.method == "handoff_to_app_workflow"
                && !file_matches_suffix(self.file, FRONTEND_HANDOFF_SUFFIXES)
            {
                self.push_violation(
                    node.span(),
                    "frontend handoff escaped the sanctioned callback boundary".to_string(),
                );
            }
            if node.method == "set_authoritative_operation_state"
                && !file_matches_suffix(self.file, FRONTEND_AUTHORITATIVE_STATE_SUFFIXES)
            {
                self.push_violation(
                    node.span(),
                    "authoritative operation state mutation escaped the sanctioned boundary"
                        .to_string(),
                );
            }
            visit::visit_expr_method_call(self, node);
        }
    }

    let mut visitor = Visitor {
        file,
        violations: Vec::new(),
    };
    visitor.visit_file(syntax);
    visitor.violations
}

fn scan_timeout_policy_boundary(file: &Path, syntax: &File) -> Vec<String> {
    struct Visitor<'a> {
        file: &'a Path,
        violations: Vec<String>,
    }

    impl Visitor<'_> {
        fn push_violation(&mut self, span: Span, message: String) {
            let start = span.start();
            self.violations.push(format!(
                "{}:{}:{}: {}",
                self.file.display(),
                start.line,
                start.column + 1,
                message
            ));
        }
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_item_fn(&mut self, node: &'ast ItemFn) {
            if has_cfg_test_attr(&node.attrs) {
                return;
            }
            visit::visit_item_fn(self, node);
        }

        fn visit_item_mod(&mut self, node: &'ast ItemMod) {
            if has_cfg_test_attr(&node.attrs) {
                return;
            }
            visit::visit_item_mod(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Some(path) = call_path_string(&node.func) {
                let path = path.replace(' ', "");
                if matches!(
                    path.as_str(),
                    "tokio::time::timeout"
                        | "tokio::time::sleep"
                        | "tokio::time::interval"
                        | "std::thread::sleep"
                        | "thread::sleep"
                ) {
                    self.push_violation(
                        node.span(),
                        format!("raw time primitive outside shared timeout model: {path}"),
                    );
                }
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut visitor = Visitor {
        file,
        violations: Vec::new(),
    };
    visitor.visit_file(syntax);
    visitor.violations
}

fn scan_time_domain_usage(file: &Path, syntax: &File) -> Vec<String> {
    struct Visitor<'a> {
        file: &'a Path,
        violations: Vec<String>,
    }

    impl Visitor<'_> {
        fn push_violation(&mut self, span: Span, message: String) {
            let start = span.start();
            self.violations.push(format!(
                "{}:{}:{}: {}",
                self.file.display(),
                start.line,
                start.column + 1,
                message
            ));
        }
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_item_fn(&mut self, node: &'ast ItemFn) {
            if has_cfg_test_attr(&node.attrs) {
                return;
            }
            visit::visit_item_fn(self, node);
        }

        fn visit_item_mod(&mut self, node: &'ast ItemMod) {
            if has_cfg_test_attr(&node.attrs) {
                return;
            }
            visit::visit_item_mod(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Some(path) = call_path_string(&node.func) {
                let path = path.replace(' ', "");
                let segments = path.split("::").collect::<Vec<_>>();
                let direct_clock_now = matches!(
                    segments.as_slice(),
                    [.., "SystemTime", "now"] | [.., "Instant", "now"]
                );
                if matches!(path.as_str(), "tokio::time::timeout" | "tokio::time::sleep")
                    || direct_clock_now
                {
                    self.push_violation(
                        node.span(),
                        format!("direct wall-clock primitive in semantic layer: {path}"),
                    );
                }
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut visitor = Visitor {
        file,
        violations: Vec::new(),
    };
    visitor.visit_file(syntax);
    visitor.violations
}
