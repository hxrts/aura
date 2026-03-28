use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use proc_macro2::Span;
use quote::ToTokens;
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprLit, File, ImplItem, Item, ItemConst, ItemFn, ItemImpl, ItemStruct, Lit,
    Meta,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum LintMode {
    LayerPolicy,
    EffectBoundaries,
    ImpureEscapes,
    Concurrency,
    FrontendPortability,
    SemanticBridgeContracts,
    CryptoBoundaries,
    CapabilityBoundaries,
    Style,
}

impl LintMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "layer-policy" => Ok(Self::LayerPolicy),
            "effect-boundaries" => Ok(Self::EffectBoundaries),
            "impure-escapes" => Ok(Self::ImpureEscapes),
            "concurrency" => Ok(Self::Concurrency),
            "frontend-portability" => Ok(Self::FrontendPortability),
            "semantic-bridge-contracts" => Ok(Self::SemanticBridgeContracts),
            "crypto-boundaries" => Ok(Self::CryptoBoundaries),
            "capability-boundaries" => Ok(Self::CapabilityBoundaries),
            "style" => Ok(Self::Style),
            other => Err(format!("unknown lint mode: {other}")),
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::LayerPolicy => "layer-policy",
            Self::EffectBoundaries => "effect-boundaries",
            Self::ImpureEscapes => "impure-escapes",
            Self::Concurrency => "concurrency",
            Self::FrontendPortability => "frontend-portability",
            Self::SemanticBridgeContracts => "semantic-bridge-contracts",
            Self::CryptoBoundaries => "crypto-boundaries",
            Self::CapabilityBoundaries => "capability-boundaries",
            Self::Style => "style",
        }
    }
}

const INFRA_EFFECT_TRAITS: &[&str] = &[
    "CryptoEffects",
    "NetworkEffects",
    "StorageEffects",
    "PhysicalTimeEffects",
    "LogicalClockEffects",
    "OrderClockEffects",
    "TimeAttestationEffects",
    "RandomEffects",
    "ConsoleEffects",
    "ConfigurationEffects",
    "LeakageEffects",
];

const APP_EFFECT_TRAITS: &[&str] = &[
    "JournalEffects",
    "AuthorityEffects",
    "FlowBudgetEffects",
    "AuthorizationEffects",
    "RelationalContextEffects",
    "GuardianEffects",
    "ChoreographicEffects",
    "EffectApiEffects",
    "SyncEffects",
];

const L4_LIBS: &[&str] = &[
    "crates/aura-guards/src/lib.rs",
    "crates/aura-consensus/src/lib.rs",
    "crates/aura-amp/src/lib.rs",
    "crates/aura-anti-entropy/src/lib.rs",
    "crates/aura-protocol/src/lib.rs",
];

const L4_ALLOWLIST: &[&str] = &[
    "crates/aura-amp/src/lib.rs",
    "crates/aura-anti-entropy/src/lib.rs",
    "crates/aura-consensus/src/lib.rs",
];

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
        .ok_or_else(|| "usage: arch_lints <lint-mode> <path> [<path> ...]".to_string())
        .and_then(|value| LintMode::parse(&value))?;
    let paths = args.map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("expected at least one path to scan".to_string());
    }

    let mut rust_files = collect_tracked_rust_files(&paths)?;
    if rust_files.is_empty() {
        for path in &paths {
            collect_rust_files(path, &mut rust_files)?;
        }
    }
    rust_files.sort();
    rust_files.dedup();

    let mut violations = Vec::new();
    for file in &rust_files {
        let source = fs::read_to_string(file)
            .map_err(|error| format!("failed to read {}: {error}", file.display()))?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("failed to parse {}: {error}", file.display()))?;
        violations.extend(scan_file(mode, file, &source, &syntax));
    }

    if !violations.is_empty() {
        for violation in violations {
            eprintln!("{violation}");
        }
        return Err(format!("{} violations remain", mode.display_name()));
    }

    println!("{}: clean", mode.display_name());
    Ok(())
}

fn collect_tracked_rust_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut command = Command::new("git");
    command.arg("ls-files").arg("--cached").arg("--");
    for path in paths {
        command.arg(path);
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("git ls-files output was not valid utf-8: {error}"))?;

    Ok(stdout
        .lines()
        .map(PathBuf::from)
        .filter(|path| path.extension() == Some(OsStr::new("rs")))
        .filter(|path| path.exists())
        .collect())
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

fn scan_file(mode: LintMode, file: &Path, source: &str, syntax: &File) -> Vec<String> {
    match mode {
        LintMode::LayerPolicy => scan_layer_policy(file, syntax),
        LintMode::EffectBoundaries => scan_effect_boundaries(file, source, syntax),
        LintMode::ImpureEscapes => scan_impure_escapes(file, source, syntax),
        LintMode::Concurrency => scan_concurrency(file, source, syntax),
        LintMode::FrontendPortability => scan_frontend_portability(file, source, syntax),
        LintMode::SemanticBridgeContracts => scan_semantic_bridge_contracts(file, syntax),
        LintMode::CryptoBoundaries => scan_crypto_boundaries(file, source, syntax),
        LintMode::CapabilityBoundaries => scan_capability_boundaries(file, source, syntax),
        LintMode::Style => scan_style(file, source, syntax),
    }
}

fn scan_layer_policy(file: &Path, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    if !L4_LIBS.contains(&path.as_str()) || L4_ALLOWLIST.contains(&path.as_str()) {
        return Vec::new();
    }

    syntax
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("allow"))
        .map(|attr| {
            format_violation(
                file,
                attr.span(),
                "Layer 4 crate lib.rs may not use crate-level #![allow(...)]".to_string(),
            )
        })
        .collect()
}

fn scan_effect_boundaries(file: &Path, source: &str, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    let mut violations = Vec::new();
    let ignored_lines = ignored_test_lines(syntax);

    let is_test_like = is_test_like_path(&path);
    let in_aura_core = path.starts_with("crates/aura-core/");
    let in_aura_effects = path.starts_with("crates/aura-effects/");
    if !in_aura_core {
        for item in &syntax.items {
            if let Item::Trait(item_trait) = item {
                let name = item_trait.ident.to_string();
                if INFRA_EFFECT_TRAITS.contains(&name.as_str()) {
                    violations.push(format_violation(
                        file,
                        item_trait.ident.span(),
                        format!(
                            "infrastructure effect trait `{name}` must be defined in aura-core"
                        ),
                    ));
                }
            }
        }
    }

    for item in &syntax.items {
        if let Item::Impl(item_impl) = item {
            if let Some(trait_name) = impl_trait_name(item_impl) {
                if INFRA_EFFECT_TRAITS.contains(&trait_name.as_str())
                    && !in_aura_core
                    && !infra_impl_allowed(&path)
                    && !is_test_like
                {
                    violations.push(format_violation(
                        file,
                        item_impl.impl_token.span,
                        format!("infrastructure effect impl `{trait_name}` must live in aura-effects or testkit"),
                    ));
                }

                if APP_EFFECT_TRAITS.contains(&trait_name.as_str())
                    && in_aura_effects
                    && !is_test_like
                {
                    violations.push(format_violation(
                        file,
                        item_impl.impl_token.span,
                        format!(
                            "application effect impl `{trait_name}` must not live in aura-effects"
                        ),
                    ));
                }
            }
        }
    }

    if in_aura_effects
        && !is_test_like
        && path != "crates/aura-effects/src/reactive/handler.rs"
        && path != "crates/aura-effects/src/query/handler.rs"
    {
        scan_line_patterns(
            file,
            source,
            &[
                (
                    "Arc<Mutex",
                    "stateful construct `Arc<Mutex>` is forbidden in aura-effects",
                ),
                (
                    "Arc<RwLock",
                    "stateful construct `Arc<RwLock>` is forbidden in aura-effects",
                ),
                (
                    "Rc<RefCell",
                    "stateful construct `Rc<RefCell>` is forbidden in aura-effects",
                ),
            ],
            &ignored_lines,
            &mut violations,
        );
    }

    if !runtime_usage_allowed(&path) {
        scan_line_patterns(
            file,
            source,
            &[(
                "tokio::",
                "direct tokio usage is outside the allowed runtime-aware layers",
            )],
            &ignored_lines,
            &mut violations,
        );
        scan_line_patterns(
            file,
            source,
            &[(
                "async_std::",
                "direct async-std usage is outside the allowed runtime-aware layers",
            )],
            &ignored_lines,
            &mut violations,
        );
    }

    if path.starts_with("crates/aura-app/src/") {
        scan_line_patterns(
            file,
            source,
            &[
                (
                    "tokio::",
                    "aura-app runtime-neutral source may not use tokio directly",
                ),
                (
                    "async_std::",
                    "aura-app runtime-neutral source may not use async-std directly",
                ),
            ],
            &ignored_lines,
            &mut violations,
        );
    }

    if path.starts_with("crates/aura-sync/src/") {
        scan_line_patterns(
            file,
            source,
            &[
                (
                    "tokio::",
                    "aura-sync runtime-neutral source may not use tokio directly",
                ),
                (
                    "async_std::",
                    "aura-sync runtime-neutral source may not use async-std directly",
                ),
            ],
            &ignored_lines,
            &mut violations,
        );
    }

    if in_aura_effects && !is_test_like {
        for item in &syntax.items {
            flag_mock_handler_items(file, item, &mut violations);
        }
    }

    violations
}

fn scan_impure_escapes(file: &Path, source: &str, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    let mut violations = Vec::new();
    let ignored_lines = ignored_test_lines(syntax);
    if impure_usage_allowed(&path) {
        return violations;
    }

    scan_line_patterns(
        file,
        source,
        &[
            (
                "std::fs::",
                "direct std::fs usage must go through storage effects",
            ),
            (
                "std::io::File",
                "direct file I/O must go through storage effects",
            ),
            (
                "std::io::BufReader",
                "direct file I/O must go through storage effects",
            ),
            (
                "std::io::BufWriter",
                "direct file I/O must go through storage effects",
            ),
            (
                "std::net::",
                "direct std::net usage must go through network effects",
            ),
            (
                "TcpStream",
                "direct TcpStream usage must go through network effects",
            ),
            (
                "TcpListener",
                "direct TcpListener usage must go through network effects",
            ),
            (
                "UdpSocket",
                "direct UdpSocket usage must go through network effects",
            ),
            (
                "SystemTime::now",
                "wall-clock access must go through time effects",
            ),
            (
                "Instant::now",
                "monotonic time access must go through time effects",
            ),
            (
                "chrono::Utc::now",
                "wall-clock access must go through time effects",
            ),
            (
                "chrono::Local::now",
                "wall-clock access must go through time effects",
            ),
            (
                "tokio::time::sleep",
                "sleep must go through shared time helpers or effect traits",
            ),
            (
                "std::thread::sleep",
                "sleep must go through shared time helpers or effect traits",
            ),
            (
                "async_std::task::sleep",
                "sleep must go through shared time helpers or effect traits",
            ),
            (
                "rand::thread_rng",
                "randomness must go through randomness effects",
            ),
            (
                "thread_rng()",
                "randomness must go through randomness effects",
            ),
            (
                "rand::random",
                "randomness must go through randomness effects",
            ),
            (
                "uuid::Uuid::new_v4",
                "UUID generation must go through the approved wrapper/effect",
            ),
        ],
        &ignored_lines,
        &mut violations,
    );

    violations
}

fn scan_concurrency(file: &Path, source: &str, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    let mut violations = Vec::new();
    let ignored_lines = ignored_test_lines(syntax);
    if concurrency_allowed(&path) {
        return violations;
    }

    scan_line_patterns(
        file,
        source,
        &[
            (
                "tokio::task::block_in_place",
                "blocking bridge is forbidden in protected modules",
            ),
            (
                "Handle::current().block_on",
                "Handle::current().block_on is forbidden in protected modules",
            ),
            (
                "mpsc::unbounded_channel(",
                "unbounded channels are forbidden in protected modules",
            ),
            (
                "async_channel::unbounded(",
                "unbounded channels are forbidden in protected modules",
            ),
            (
                "mpsc::unbounded(",
                "unbounded channels are forbidden in protected modules",
            ),
        ],
        &ignored_lines,
        &mut violations,
    );

    violations
}

fn scan_frontend_portability(file: &Path, source: &str, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    if !path.starts_with("crates/aura-app/src/frontend_primitives/") {
        return Vec::new();
    }

    let mut violations = Vec::new();
    scan_line_patterns(
        file,
        source,
        &[
            (
                "std::sync::Mutex",
                "shared frontend task ownership must use the sanctioned frontend waiter abstraction instead of std::sync::Mutex",
            ),
            (
                "std::sync::RwLock",
                "shared frontend task ownership must use the sanctioned frontend waiter abstraction instead of std::sync::RwLock",
            ),
            (
                "parking_lot::Mutex",
                "shared frontend primitives must remain platform-neutral; do not use native-only parking_lot mutexes",
            ),
            (
                "parking_lot::RwLock",
                "shared frontend primitives must remain platform-neutral; do not use native-only parking_lot rwlocks",
            ),
            (
                "std::sync::Condvar",
                "shared frontend primitives must remain platform-neutral; do not use native-only condition variables",
            ),
            (
                "lock_blocking(",
                "shared frontend primitives must remain wasm-safe; do not use blocking async-lock APIs",
            ),
            (
                "std::thread::spawn",
                "shared frontend primitives must use the sanctioned frontend task owner instead of native thread spawning",
            ),
            (
                "tokio::task::spawn_blocking",
                "shared frontend primitives must remain wasm-safe; do not use spawn_blocking in shared frontend code",
            ),
            (
                "tokio::time::sleep",
                "shared frontend primitives must not own platform-specific sleep mechanics directly",
            ),
            (
                "gloo_timers::future::sleep",
                "shared frontend primitives must not own browser-specific sleep mechanics directly",
            ),
            (
                "wasm_bindgen_futures::spawn_local",
                "shared frontend primitives must use the sanctioned frontend task owner instead of direct platform-specific spawn_local",
            ),
            (
                "tokio::spawn",
                "shared frontend primitives must use the sanctioned frontend task owner instead of direct tokio::spawn",
            ),
            (
                "dioxus::prelude::spawn",
                "shared frontend primitives must use the sanctioned frontend task owner instead of direct dioxus spawn",
            ),
        ],
        &ignored_test_lines(syntax),
        &mut violations,
    );
    violations
}

fn scan_semantic_bridge_contracts(file: &Path, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    if !path.starts_with("crates/aura-web/src/harness/") {
        return Vec::new();
    }

    let mut violations = Vec::new();
    if path != "crates/aura-web/src/harness/commands.rs" {
        let source = syntax.to_token_stream().to_string();
        for (pattern, message) in [
            (
                "route_semantic_intent",
                "browser semantic routing must remain centralized in harness/commands.rs",
            ),
            (
                "execute_semantic_intent",
                "browser semantic execution must remain centralized in harness/commands.rs",
            ),
            (
                "SemanticCommandResponse :: accepted_without_value",
                "browser semantic bridge files must not construct raw semantic responses outside harness/commands.rs",
            ),
            (
                "begin_exact_handoff_operation",
                "browser semantic bridge files must not begin raw handoff operations outside harness/commands.rs",
            ),
            (
                "begin_exact_ui_operation",
                "browser semantic bridge files must not begin raw exact operations outside harness/commands.rs",
            ),
            (
                "WeakChannelSelection",
                "browser semantic bridge files must not emit weak channel fallback payloads outside harness/commands.rs",
            ),
        ] {
            if path == "crates/aura-web/src/harness/channel_selection.rs"
                && pattern == "WeakChannelSelection"
            {
                continue;
            }
            if source.contains(pattern) {
                violations.push(format_violation(
                    file,
                    Span::call_site(),
                    message.to_string(),
                ));
            }
        }

        if path == "crates/aura-web/src/harness/install.rs"
            && !source.contains("commands :: submit_semantic_command")
        {
            violations.push(format_violation(
                file,
                Span::call_site(),
                "browser harness install path must delegate semantic submission through harness/commands.rs".to_string(),
            ));
        }
        return violations;
    }

    for item in &syntax.items {
        let Item::Fn(function) = item else {
            continue;
        };
        if function.sig.ident != "execute_semantic_intent" {
            continue;
        }

        let body = function.block.to_token_stream().to_string();
        for (pattern, message) in [
            (
                "SemanticCommandResponse :: accepted_without_value",
                "web semantic command execution must use declared immediate-response helpers instead of raw accepted_without_value",
            ),
            (
                "begin_exact_handoff_operation",
                "web semantic command execution must use declared handoff helpers instead of raw begin_exact_handoff_operation",
            ),
            (
                "begin_exact_ui_operation",
                "web semantic command execution must use declared exact-handle helpers instead of raw begin_exact_ui_operation",
            ),
            (
                "semantic_response_with_handle",
                "web semantic command execution must use declared handle-response helpers instead of raw semantic_response_with_handle",
            ),
            (
                "semantic_unit_result_with_handle",
                "web semantic command execution must use declared handle-response helpers instead of raw semantic_unit_result_with_handle",
            ),
            (
                "semantic_channel_result",
                "web semantic command execution must use declared response helpers instead of raw semantic_channel_result",
            ),
            (
                "semantic_channel_result_with_handle",
                "web semantic command execution must use declared handle-response helpers instead of raw semantic_channel_result_with_handle",
            ),
        ] {
            if body.contains(pattern) {
                violations.push(format_violation(
                    file,
                    function.sig.ident.span(),
                    message.to_string(),
                ));
            }
        }
    }

    violations
}

fn scan_crypto_boundaries(file: &Path, source: &str, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    let mut violations = Vec::new();
    let ignored_lines = ignored_test_lines(syntax);
    if crypto_allowed(&path) {
        return violations;
    }

    scan_line_patterns(
        file,
        source,
        &[
            (
                "use ed25519_dalek",
                "ed25519_dalek must stay behind aura-core wrappers",
            ),
            (
                "OsRng",
                "OsRng must stay behind the approved randomness boundary",
            ),
            (
                "getrandom::",
                "getrandom must stay behind the approved randomness boundary",
            ),
        ],
        &ignored_lines,
        &mut violations,
    );

    violations
}

fn scan_style(file: &Path, source: &str, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    let mut violations = Vec::new();

    if !is_test_like_path(&path) && !path.starts_with("crates/aura-macros/") {
        scan_line_patterns(
            file,
            source,
            &[(
                "bincode::",
                "bincode usage is forbidden; use the canonical serialization helpers",
            )],
            &ignored_test_lines(syntax),
            &mut violations,
        );
    }

    if path.starts_with("crates/aura-core/src/") {
        for item in &syntax.items {
            match item {
                Item::Const(item_const) => {
                    maybe_flag_constant_without_units(file, item_const, &mut violations)
                }
                Item::Fn(item_fn) => {
                    maybe_flag_builder_without_must_use(file, item_fn, &mut violations)
                }
                Item::Impl(item_impl) => {
                    for impl_item in &item_impl.items {
                        if let ImplItem::Fn(method) = impl_item {
                            maybe_flag_builder_method_without_must_use(
                                file,
                                method,
                                &mut violations,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if !is_test_like_path(&path) && !path.starts_with("crates/aura-testkit/") {
        for item in &syntax.items {
            if let Item::Struct(item_struct) = item {
                maybe_flag_serialized_usize_fields(file, item_struct, &mut violations);
            }
        }
    }

    violations
}

fn scan_capability_boundaries(file: &Path, source: &str, syntax: &File) -> Vec<String> {
    let path = display_path(file);
    if is_test_like_path(&path) {
        return Vec::new();
    }

    let mut violations = Vec::new();
    let ignored_lines = ignored_test_lines(syntax);

    if path != "crates/aura-macros/src/bin/arch_lints.rs" {
        scan_line_patterns(
            file,
            source,
            &[(
                "CapabilityId::from(",
                "CapabilityId raw-string construction is forbidden; use typed capability families or an explicit validated boundary",
            )],
            &ignored_lines,
            &mut violations,
        );
    }

    if !capability_name_parse_allowed(&path) {
        scan_line_patterns(
            file,
            source,
            &[(
                "CapabilityName::parse(",
                "CapabilityName::parse is restricted to approved boundary modules; use typed capability families elsewhere",
            )],
            &ignored_lines,
            &mut violations,
        );
    }

    if !capability_literal_allowed(&path) {
        scan_line_patterns(
            file,
            source,
            &[(
                "capability_name!(",
                "direct capability_name! construction is forbidden in protected modules; use typed capability families or approved fixtures",
            )],
            &ignored_lines,
            &mut violations,
        );
    }

    if path.starts_with("crates/aura-agent/src/runtime/") {
        scan_line_patterns(
            file,
            source,
            &[(
                "\"module:",
                "host runtime module capability references must come from admitted descriptors, not handwritten strings",
            )],
            &ignored_lines,
            &mut violations,
        );
    }

    scan_line_patterns(
        file,
        source,
        &[
            (
                "capability_name!(\"message:send\")",
                "legacy `message:send` capability usage is forbidden; use the canonical owned capability family",
            ),
            (
                "\"sync_journal\"",
                "legacy `sync_journal` capability wording is forbidden; use canonical sync capability names",
            ),
            (
                "\"sync.permission\"",
                "stale sync capability placeholder is forbidden; use canonical sync capability names",
            ),
            (
                "\"invitation:create\"",
                "stale `invitation:create` capability wording is forbidden; use canonical invitation capability names",
            ),
            (
                "\"recovery_initiate\"",
                "legacy `recovery_initiate` capability wording is forbidden; use canonical recovery capability names",
            ),
            (
                "\"recovery_approve\"",
                "legacy `recovery_approve` capability wording is forbidden; use canonical recovery capability names",
            ),
        ],
        &ignored_lines,
        &mut violations,
    );

    for item in &syntax.items {
        if let Item::Struct(item_struct) = item {
            maybe_flag_raw_capability_string_fields(file, item_struct, &mut violations);
        }
    }

    violations
}

fn maybe_flag_raw_capability_string_fields(
    file: &Path,
    item_struct: &ItemStruct,
    violations: &mut Vec<String>,
) {
    for field in &item_struct.fields {
        let Some(ident) = &field.ident else {
            continue;
        };
        let field_name = ident.to_string();
        let expects_typed_capability_surface = matches!(
            field_name.as_str(),
            "guard_capabilities" | "recovery_capabilities"
        );
        if expects_typed_capability_surface && is_vec_of_string(&field.ty) {
            violations.push(format_violation(
                file,
                field.ty.span(),
                format!(
                    "field `{}` on struct `{}` may not use Vec<String> for authorization vocabulary; use a typed capability surface",
                    field_name, item_struct.ident
                ),
            ));
        }
    }
}

fn is_vec_of_string(ty: &syn::Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    let Some(segment) = type_path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Vec" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    let Some(syn::GenericArgument::Type(syn::Type::Path(inner_type))) = args.args.first() else {
        return false;
    };
    inner_type.path.is_ident("String")
}

fn maybe_flag_serialized_usize_fields(
    file: &Path,
    item_struct: &ItemStruct,
    violations: &mut Vec<String>,
) {
    if !has_serialize_derive(&item_struct.attrs) {
        return;
    }

    for field in &item_struct.fields {
        if let syn::Type::Path(type_path) = &field.ty {
            if type_path.path.is_ident("usize") {
                violations.push(format_violation(
                    file,
                    field.ty.span(),
                    format!(
                        "serialized struct `{}` may not use `usize` fields; use a fixed-width integer",
                        item_struct.ident
                    ),
                ));
            }
        }
    }
}

fn has_serialize_derive(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("derive") {
            return false;
        }
        match &attr.meta {
            Meta::List(list) => list.tokens.to_string().contains("Serialize"),
            _ => false,
        }
    })
}

fn maybe_flag_constant_without_units(
    file: &Path,
    item_const: &ItemConst,
    violations: &mut Vec<String>,
) {
    let name = item_const.ident.to_string();
    if !name
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return;
    }

    if !is_integer_type(&item_const.ty) || !is_integer_literal(&item_const.expr) {
        return;
    }

    if has_allowed_constant_suffix(&name) {
        return;
    }

    violations.push(format_violation(
        file,
        item_const.ident.span(),
        format!("constant `{name}` must include an explicit unit/count suffix"),
    ));
}

fn maybe_flag_builder_without_must_use(
    file: &Path,
    item_fn: &ItemFn,
    violations: &mut Vec<String>,
) {
    if is_public_builder(item_fn.sig.ident.to_string().as_str(), &item_fn.vis)
        && !has_must_use(&item_fn.attrs)
    {
        violations.push(format_violation(
            file,
            item_fn.sig.ident.span(),
            format!("builder `{}` must be marked #[must_use]", item_fn.sig.ident),
        ));
    }
}

fn maybe_flag_builder_method_without_must_use(
    file: &Path,
    method: &syn::ImplItemFn,
    violations: &mut Vec<String>,
) {
    if is_public_builder(method.sig.ident.to_string().as_str(), &method.vis)
        && !has_must_use(&method.attrs)
    {
        violations.push(format_violation(
            file,
            method.sig.ident.span(),
            format!("builder `{}` must be marked #[must_use]", method.sig.ident),
        ));
    }
}

fn has_must_use(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("must_use"))
}

fn is_public_builder(name: &str, vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_)) && name.starts_with("with_")
}

fn is_integer_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(type_path) => type_path.path.segments.last().is_some_and(|segment| {
            matches!(
                segment.ident.to_string().as_str(),
                "u8" | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
            )
        }),
        _ => false,
    }
}

fn is_integer_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Lit(ExprLit {
            lit: Lit::Int(_),
            ..
        })
    )
}

fn has_allowed_constant_suffix(name: &str) -> bool {
    [
        "_MS", "_BYTES", "_COUNT", "_SIZE", "_MAX", "_MIN", "_LEN", "_LIMIT", "_DEPTH", "_HEIGHT",
        "_BITS", "_SECS", "_NANOS",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
        || [
            "VERSION",
            "MAGIC",
            "EPOCH",
            "THRESHOLD",
            "FACTOR",
            "RATIO",
            "WIRE_FORMAT",
            "DEFAULT_",
        ]
        .iter()
        .any(|token| name.contains(token))
}

fn impl_trait_name(item_impl: &ItemImpl) -> Option<String> {
    item_impl.trait_.as_ref().and_then(|(_, path, _)| {
        path.segments
            .last()
            .map(|segment| segment.ident.to_string())
    })
}

fn scan_line_patterns(
    file: &Path,
    source: &str,
    patterns: &[(&str, &str)],
    ignored_lines: &[(usize, usize)],
    violations: &mut Vec<String>,
) {
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        if ignored_lines
            .iter()
            .any(|(start, end)| line_number >= *start && line_number <= *end)
        {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        for (needle, message) in patterns {
            if let Some(column) = line.find(needle) {
                violations.push(format!(
                    "{}:{}:{}: {}",
                    file.display(),
                    line_number,
                    column + 1,
                    message
                ));
            }
        }
    }
}

fn format_violation(file: &Path, span: Span, message: String) -> String {
    let start = span.start();
    format!(
        "{}:{}:{}: {}",
        file.display(),
        start.line,
        start.column + 1,
        message
    )
}

fn display_path(file: &Path) -> String {
    file.to_string_lossy().replace('\\', "/")
}

fn is_test_like_path(path: &str) -> bool {
    path.contains("/tests/")
        || path.contains("/benches/")
        || path.contains("/examples/")
        || path.ends_with("_test.rs")
}

fn runtime_usage_allowed(path: &str) -> bool {
    path.starts_with("crates/aura-effects/")
        || path.starts_with("crates/aura-agent/")
        || path.starts_with("crates/aura-simulator/")
        || path.starts_with("crates/aura-terminal/")
        || path.starts_with("crates/aura-composition/")
        || path.starts_with("crates/aura-testkit/")
        || path.starts_with("crates/aura-harness/")
        || path.starts_with("crates/aura-macros/")
        || path == "crates/aura-authorization/src/storage_authorization.rs"
        || path == "crates/aura-rendezvous/src/service.rs"
        || path == "crates/aura-core/src/effects/reactive.rs"
        || path == "crates/aura-app/src/core/app.rs"
        || path == "crates/aura-app/src/core/signal_sync.rs"
        || is_test_like_path(path)
}

fn impure_usage_allowed(path: &str) -> bool {
    path.starts_with("crates/aura-effects/")
        || path.starts_with("crates/aura-testkit/")
        || path.starts_with("crates/aura-simulator/")
        || path.starts_with("crates/aura-harness/")
        || path.starts_with("crates/aura-terminal/")
        || path.starts_with("crates/aura-agent/src/runtime/")
        || path == "crates/aura-agent/src/runtime_bridge_impl.rs"
        || path.starts_with("crates/aura-agent/src/builder/")
        || path == "crates/aura-app/src/core/app.rs"
        || path == "crates/aura-app/src/core/signal_sync.rs"
        || path == "crates/aura-terminal/src/handlers/tui.rs"
        || path == "crates/aura-terminal/src/tui/fullscreen_stdio.rs"
        || path.starts_with("crates/aura-macros/")
        || is_test_like_path(path)
}

fn concurrency_allowed(path: &str) -> bool {
    path.starts_with("crates/aura-effects/")
        || path.starts_with("crates/aura-simulator/")
        || path.starts_with("crates/aura-testkit/")
        || path.starts_with("crates/aura-harness/")
        || path.starts_with("crates/aura-agent/src/runtime/")
        || path == "crates/aura-agent/src/runtime_bridge_impl.rs"
        || path.starts_with("crates/aura-agent/src/builder/")
        || path == "crates/aura-terminal/src/handlers/tui.rs"
        || path == "crates/aura-terminal/src/tui/fullscreen_stdio.rs"
        || path == "crates/aura-app/src/core/app.rs"
        || path == "crates/aura-app/src/core/signal_sync.rs"
        || path.starts_with("crates/aura-macros/")
        || is_test_like_path(path)
}

fn crypto_allowed(path: &str) -> bool {
    path.starts_with("crates/aura-core/src/crypto/")
        || path == "crates/aura-core/src/types/authority.rs"
        || path.starts_with("crates/aura-effects/")
        || path.starts_with("crates/aura-testkit/")
        || path.starts_with("crates/aura-macros/")
        || is_test_like_path(path)
}

fn capability_name_parse_allowed(path: &str) -> bool {
    path == "crates/aura-core/src/capability_name.rs"
        || path == "crates/aura-authorization/src/biscuit_authorization.rs"
        || path == "crates/aura-guards/src/authorization.rs"
        || path == "crates/aura-mpst/src/ast_extraction.rs"
        || path == "crates/aura-mpst/src/composition.rs"
        || path == "crates/aura-macros/src/capability_family.rs"
        || path == "crates/aura-macros/src/bin/arch_lints.rs"
}

fn capability_literal_allowed(path: &str) -> bool {
    path == "crates/aura-core/src/capability_name.rs"
        || path == "crates/aura-macros/src/capability_family.rs"
        || path.starts_with("crates/aura-testkit/")
        || path == "crates/aura-macros/src/bin/arch_lints.rs"
}

fn infra_impl_allowed(path: &str) -> bool {
    path.starts_with("crates/aura-effects/")
        || path.starts_with("crates/aura-testkit/")
        || path.starts_with("crates/aura-core/")
        || path.starts_with("crates/aura-agent/")
        || path.starts_with("crates/aura-simulator/")
        || path == "crates/aura-protocol/src/handlers/timeout_coordinator.rs"
}

fn ignored_test_lines(syntax: &File) -> Vec<(usize, usize)> {
    let mut ignored = Vec::new();
    for item in &syntax.items {
        collect_ignored_test_lines(item, &mut ignored);
    }
    ignored
}

fn collect_ignored_test_lines(item: &Item, ignored: &mut Vec<(usize, usize)>) {
    if item_is_test_scoped(item) {
        let span = item.span();
        ignored.push((span.start().line, span.end().line));
        return;
    }

    if let Item::Mod(item_mod) = item {
        if let Some((_, items)) = &item_mod.content {
            for nested in items {
                collect_ignored_test_lines(nested, ignored);
            }
        }
    }
}

fn item_is_test_scoped(item: &Item) -> bool {
    match item {
        Item::Fn(item_fn) => attrs_mark_test_scope(&item_fn.attrs),
        Item::Mod(item_mod) => item_mod.ident == "tests" || attrs_mark_test_scope(&item_mod.attrs),
        Item::Impl(_) => false,
        Item::Struct(item_struct) => attrs_mark_test_scope(&item_struct.attrs),
        Item::Enum(item_enum) => attrs_mark_test_scope(&item_enum.attrs),
        Item::Const(item_const) => attrs_mark_test_scope(&item_const.attrs),
        Item::Trait(item_trait) => attrs_mark_test_scope(&item_trait.attrs),
        _ => false,
    }
}

fn attrs_mark_test_scope(attrs: &[Attribute]) -> bool {
    attrs.iter().any(attr_marks_test_scope)
}

fn attr_marks_test_scope(attr: &Attribute) -> bool {
    let path = attr.path();
    if path.is_ident("test") {
        return true;
    }
    if path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "test")
    {
        return true;
    }
    if path.is_ident("cfg") {
        return attr.meta.to_token_stream().to_string().contains("test");
    }
    false
}

fn flag_mock_handler_items(file: &Path, item: &Item, violations: &mut Vec<String>) {
    match item {
        Item::Struct(item_struct) => {
            let name = item_struct.ident.to_string();
            if is_mock_handler_name(&name) {
                violations.push(format_violation(
                    file,
                    item_struct.ident.span(),
                    format!(
                        "mock/in-memory handler `{name}` belongs in aura-testkit, not aura-effects"
                    ),
                ));
            }
        }
        Item::Enum(item_enum) => {
            let name = item_enum.ident.to_string();
            if is_mock_handler_name(&name) {
                violations.push(format_violation(
                    file,
                    item_enum.ident.span(),
                    format!(
                        "mock/in-memory handler `{name}` belongs in aura-testkit, not aura-effects"
                    ),
                ));
            }
        }
        Item::Fn(item_fn) => {
            let name = item_fn.sig.ident.to_string();
            if is_mock_handler_name(&name) {
                violations.push(format_violation(
                    file,
                    item_fn.sig.ident.span(),
                    format!(
                        "mock/in-memory handler `{name}` belongs in aura-testkit, not aura-effects"
                    ),
                ));
            }
        }
        Item::Mod(item_mod) => {
            if let Some((_, items)) = &item_mod.content {
                for nested in items {
                    flag_mock_handler_items(file, nested, violations);
                }
            }
        }
        _ => {}
    }
}

fn is_mock_handler_name(name: &str) -> bool {
    (name.contains("Mock") || name.contains("InMemory")) && name.contains("Handler")
}
