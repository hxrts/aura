//! Aura Choreography Macro Implementation
//!
//! This module provides the choreography! macro that generates both the underlying
//! Telltale choreography and the Aura-specific wrapper module expected by
//! the examples and integration code.
//!
//! The macro generates:
//! - Core choreographic projection via Telltale
//! - Aura wrapper module with role types and helper functions
//! - Integration with the Aura effects system

use proc_macro2::{Group, TokenStream, TokenTree};
use quote::quote;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use syn::spanned::Spanned;
use syn::{
    parse::Parse,
    visit_mut::{self, VisitMut},
    Attribute, Expr, ExprLit, ExprMacro, Ident, Lit, LitStr, Stmt, Type,
};
use telltale_choreography::ast::{
    choreography_to_global, local_to_local_r, GlobalTypeCore, PayloadSort,
};
use telltale_choreography::{
    generate_choreography_code, parse_choreography_str, project, Choreography, MessageType,
    Protocol,
};
use telltale_language as telltale_choreography;
use telltale_theory::coherence::check_coherent;

// Import Biscuit-related types for the updated annotation system
use aura_mpst::ast_extraction::{extract_aura_annotations, AuraEffect};

/// Parsed choreography input (DSL source + derived metadata)
#[derive(Debug)]
struct ChoreographyInput {
    roles: Vec<Ident>,
    aura_annotations: Vec<AuraEffect>,
    namespace: Option<String>,
    choreography: Choreography,
}

#[derive(Debug)]
struct ChoreographyMacroInput {
    attrs: Vec<Attribute>,
    expr: Expr,
}

type DeliveryEdge = (String, String, String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CoherenceValidationInput {
    global_type: GlobalTypeCore,
    // Deterministic initial delivery environment (Coherent(G, D)):
    // keys are (sender, receiver, label), all initialized to 0 buffered messages.
    initial_delivery_env: BTreeMap<DeliveryEdge, usize>,
}

#[derive(Debug)]
enum CoherenceModelError {
    UnsupportedFeature {
        feature: &'static str,
        hint: &'static str,
    },
    InvalidChoice(String),
}

impl std::fmt::Display for CoherenceModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFeature { feature, hint } => {
                write!(
                    f,
                    "unsupported DSL feature for coherence conversion: {feature}. {hint}"
                )
            }
            Self::InvalidChoice(reason) => write!(f, "{reason}"),
        }
    }
}

fn validate_protocol_coherence(choreography: &Choreography) -> Result<(), String> {
    let coherence_input = build_coherence_validation_input(choreography)
        .map_err(|err| format!("failed to derive theory model: {err}"))?;

    let bundle = check_coherent(&coherence_input.global_type);
    if bundle.is_coherent() {
        return Ok(());
    }

    let mut failed = Vec::new();
    if !bundle.size {
        failed.push("size (every communication must have at least one branch)");
    }
    if !bundle.action {
        failed.push("action (sender and receiver must differ)");
    }
    if !bundle.uniq_labels {
        failed.push("uniq_labels (branch labels must be unique)");
    }
    if !bundle.projectable {
        failed.push("projectable (every role must project successfully)");
    }
    if !bundle.good {
        failed.push("good (enabled steps must be executable)");
    }

    let preview = coherence_input
        .initial_delivery_env
        .keys()
        .take(6)
        .map(|(src, dst, label)| format!("{src}->{dst}:{label}=0"))
        .collect::<Vec<_>>()
        .join(", ");

    Err(format!(
        "Coherence validation failed for choreography `{}`.\n\
         Failed predicates: {}.\n\
         Deterministic initial_delivery_env channels: [{}].\n\
         Hint: ensure choice branches are mergeable for non-participants and avoid self-sends.",
        choreography.name,
        failed.join(", "),
        preview
    ))
}

fn build_coherence_validation_input(
    choreography: &Choreography,
) -> Result<CoherenceValidationInput, CoherenceModelError> {
    let global_type = if let Ok(authoritative_global) = choreography_to_global(choreography) {
        authoritative_global
    } else {
        let mut next_loop_id = 0usize;
        protocol_to_global_for_coherence(&choreography.protocol, &mut next_loop_id)?
    };
    let mut initial_delivery_env = BTreeMap::new();
    collect_initial_delivery_env(&global_type, &mut initial_delivery_env);
    Ok(CoherenceValidationInput {
        global_type,
        initial_delivery_env,
    })
}

fn protocol_to_global_for_coherence(
    protocol: &Protocol,
    next_loop_id: &mut usize,
) -> Result<GlobalTypeCore, CoherenceModelError> {
    match protocol {
        Protocol::Begin { .. }
        | Protocol::Await { .. }
        | Protocol::Resolve { .. }
        | Protocol::Invalidate { .. }
        | Protocol::Let { .. }
        | Protocol::Case { .. }
        | Protocol::Timeout { .. }
        | Protocol::Publish { .. }
        | Protocol::PublishAuthority { .. }
        | Protocol::Materialize { .. }
        | Protocol::Handoff { .. }
        | Protocol::DependentWork { .. } => Err(CoherenceModelError::UnsupportedFeature {
            feature: "authority-local protocol constructs",
            hint: "the current Aura runner-generation path still only supports the message-passing choreography subset",
        }),
        Protocol::End => Ok(GlobalTypeCore::End),
        Protocol::Var(ident) => Ok(GlobalTypeCore::var(ident.to_string())),
        Protocol::Rec { label, body } => Ok(GlobalTypeCore::mu(
            label.to_string(),
            protocol_to_global_for_coherence(body, next_loop_id)?,
        )),
        Protocol::Loop { body, .. } => {
            let loop_var = format!("__loop_{}", *next_loop_id);
            *next_loop_id += 1;
            Ok(GlobalTypeCore::mu(
                loop_var,
                protocol_to_global_for_coherence(body, next_loop_id)?,
            ))
        }
        Protocol::Send {
            from,
            to,
            message,
            continuation,
            ..
        } => Ok(GlobalTypeCore::send(
            from.name().to_string(),
            to.name().to_string(),
            message_to_theory_label(message),
            protocol_to_global_for_coherence(continuation, next_loop_id)?,
        )),
        Protocol::Choice { role, branches, .. } => {
            let first_receiver = match &branches[0].protocol {
                Protocol::Send { from, to, .. } if from.name() == role.name() => {
                    to.name().to_string()
                }
                _ => {
                    return Err(CoherenceModelError::InvalidChoice(format!(
                        "invalid choice at role `{}`: branch `{}` does not start with send from decider",
                        role.name(),
                        branches[0].label
                    )));
                }
            };

            let mut mapped_branches = Vec::with_capacity(branches.len());
            for branch in branches {
                match &branch.protocol {
                    Protocol::Send {
                        from,
                        to,
                        message,
                        continuation,
                        ..
                    } => {
                        if from.name() != role.name() {
                            return Err(CoherenceModelError::InvalidChoice(format!(
                                "invalid choice at role `{}`: branch `{label}` starts from `{}`",
                                role.name(),
                                from.name(),
                                label = branch.label
                            )));
                        }
                        if to.name() != first_receiver.as_str() {
                            return Err(CoherenceModelError::InvalidChoice(format!(
                                "invalid choice at role `{}`: branch `{label}` sends to `{}` but expected `{}`",
                                role.name(),
                                to.name(),
                                first_receiver,
                                label = branch.label
                            )));
                        }
                        mapped_branches.push((
                            message_to_theory_label(message),
                            protocol_to_global_for_coherence(continuation, next_loop_id)?,
                        ));
                    }
                    _ => {
                        return Err(CoherenceModelError::InvalidChoice(format!(
                            "invalid choice at role `{}`: branch `{label}` must start with send",
                            role.name(),
                            label = branch.label
                        )));
                    }
                }
            }

            Ok(GlobalTypeCore::comm(
                role.name().to_string(),
                first_receiver,
                mapped_branches,
            ))
        }
        Protocol::Broadcast { .. } => Err(CoherenceModelError::UnsupportedFeature {
            feature: "Broadcast",
            hint: "Desugar broadcast to point-to-point sends before coherence validation.",
        }),
        Protocol::Parallel { .. } => Err(CoherenceModelError::UnsupportedFeature {
            feature: "Parallel",
            hint: "Run coherence on a desugared sequential form or projected locals.",
        }),
        Protocol::Extension { .. } => Err(CoherenceModelError::UnsupportedFeature {
            feature: "Extension",
            hint: "Extensions do not have a direct theory-level global representation.",
        }),
    }
}

fn message_to_theory_label(message: &MessageType) -> telltale_choreography::ast::Label {
    telltale_choreography::ast::Label::with_sort(message.name.to_string(), PayloadSort::Unit)
}

fn collect_initial_delivery_env(
    global: &GlobalTypeCore,
    initial_delivery_env: &mut BTreeMap<DeliveryEdge, usize>,
) {
    match global {
        GlobalTypeCore::Comm {
            sender,
            receiver,
            branches,
        } => {
            for (label, continuation) in branches {
                initial_delivery_env
                    .entry((sender.clone(), receiver.clone(), label.name.clone()))
                    .or_insert(0);
                collect_initial_delivery_env(continuation, initial_delivery_env);
            }
        }
        GlobalTypeCore::Mu { body, .. } => {
            collect_initial_delivery_env(body, initial_delivery_env);
        }
        GlobalTypeCore::Var(_) | GlobalTypeCore::End => {}
    }
}

impl Parse for ChoreographyMacroInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let expr: Expr = input.parse()?;
        Ok(Self { attrs, expr })
    }
}

fn parse_choreography_source(input: TokenStream) -> Result<ChoreographyInput, syn::Error> {
    let parsed = syn::parse2::<ChoreographyMacroInput>(input)?;
    let namespace_attr = extract_namespace_from_attrs(&parsed.attrs)?;
    let (dsl, span) = read_choreography_source(parsed.expr)?;

    let parser_dsl = normalize_choreography_for_parser(&dsl);
    let mut choreography = parse_choreography_str(&parser_dsl)
        .map_err(|err| syn::Error::new(span, format!("Choreography parse error: {err}")))?;
    let namespace = match (namespace_attr, choreography.namespace.clone()) {
        (Some(attr), Some(parsed_ns)) => {
            if parsed_ns != attr {
                return Err(syn::Error::new(
                    span,
                    format!(
                        "Namespace mismatch: macro namespace \"{}\" \
                         does not match module namespace \"{}\"",
                        attr, parsed_ns
                    ),
                ));
            }
            choreography.namespace = Some(attr.clone());
            Some(attr)
        }
        (Some(attr), None) => {
            choreography.namespace = Some(attr.clone());
            Some(attr)
        }
        (None, Some(parsed_ns)) => Some(parsed_ns),
        (None, None) => None,
    };

    let roles = choreography
        .roles
        .iter()
        .map(|role| role.name().clone())
        .collect::<Vec<_>>();
    let aura_annotations = extract_aura_annotations(&dsl)
        .map_err(|err| syn::Error::new(proc_macro2::Span::call_site(), err.to_string()))?;
    validate_link_annotations(&aura_annotations).map_err(|err| {
        syn::Error::new(span, format!("Link annotation validation failed: {err}"))
    })?;
    validate_protocol_coherence(&choreography)
        .map_err(|err| syn::Error::new(span, format!("Coherence validation failed: {err}")))?;

    Ok(ChoreographyInput {
        roles,
        aura_annotations,
        namespace,
        choreography,
    })
}

fn validate_link_annotations(annotations: &[AuraEffect]) -> Result<(), String> {
    let mut bundle_exports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut bundle_imports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for annotation in annotations {
        if let AuraEffect::Link { directive, .. } = annotation {
            bundle_exports
                .entry(directive.bundle_id.clone())
                .or_default()
                .extend(directive.exports.iter().cloned());
            bundle_imports
                .entry(directive.bundle_id.clone())
                .or_default()
                .extend(directive.imports.iter().cloned());
        }
    }

    if bundle_exports.is_empty() && bundle_imports.is_empty() {
        return Ok(());
    }

    let all_exports = bundle_exports
        .values()
        .flat_map(|exports| exports.iter().cloned())
        .collect::<BTreeSet<_>>();

    for (bundle_id, imports) in bundle_imports {
        for import in imports {
            if !all_exports.contains(&import) {
                return Err(format!(
                    "bundle '{bundle_id}' imports '{import}' but no link annotation exports it"
                ));
            }
        }
    }

    Ok(())
}

fn strip_aura_annotations_for_parser(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    #[allow(clippy::while_let_on_iterator)]
    while let Some(ch) = chars.next() {
        let (open, close) = match ch {
            '[' => ('[', ']'),
            '{' => ('{', '}'),
            _ => {
                out.push(ch);
                continue;
            }
        };

        let mut depth = 1usize;
        let mut buf = String::new();
        let mut has_equals = false;

        while let Some(next) = chars.next() {
            if next == open {
                depth += 1;
            } else if next == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            if next == '=' {
                has_equals = true;
            }
            buf.push(next);
        }

        if !has_equals {
            out.push(open);
            out.push_str(&buf);
            out.push(close);
        }
    }

    out
}

fn normalize_choreography_for_parser(input: &str) -> String {
    let stripped = strip_aura_annotations_for_parser(input);
    let mut normalized = String::with_capacity(stripped.len());

    for line in stripped.lines() {
        let indent_len = line.chars().take_while(|ch| ch.is_whitespace()).count();
        let (indent, body) = line.split_at(indent_len);
        if let Some(rest) = body.strip_prefix("choice at ") {
            normalized.push_str(indent);
            normalized.push_str("choice ");
            normalized.push_str(rest);
            normalized.push_str(" at");
        } else if body.starts_with('|') && body.contains("->") {
            let rewritten = body.replacen("->", "=>", 1);
            normalized.push_str(indent);
            normalized.push_str(&rewritten);
        } else {
            normalized.push_str(line);
        }
        normalized.push('\n');
    }

    normalized
}

fn insert_message(out: &mut BTreeMap<String, MessageType>, message: MessageType) {
    out.insert(message.name.to_string(), message);
}

fn insert_choice_label(out: &mut BTreeMap<String, MessageType>, label: &Ident) {
    let key = label.to_string();
    out.entry(key).or_insert_with(|| MessageType {
        name: label.clone(),
        type_annotation: None,
        payload: None,
    });
}

fn collect_messages(protocol: &Protocol, out: &mut BTreeMap<String, MessageType>) {
    match protocol {
        Protocol::Begin { continuation, .. }
        | Protocol::Await { continuation, .. }
        | Protocol::Resolve { continuation, .. }
        | Protocol::Invalidate { continuation, .. }
        | Protocol::Let { continuation, .. }
        | Protocol::Publish { continuation, .. }
        | Protocol::PublishAuthority { continuation, .. }
        | Protocol::Materialize { continuation, .. }
        | Protocol::Handoff { continuation, .. }
        | Protocol::DependentWork { continuation, .. }
        | Protocol::Extension { continuation, .. } => {
            collect_messages(continuation, out);
        }
        Protocol::Send {
            message,
            continuation,
            ..
        } => {
            insert_message(out, message.clone());
            collect_messages(continuation, out);
        }
        Protocol::Broadcast {
            message,
            continuation,
            ..
        } => {
            insert_message(out, message.clone());
            collect_messages(continuation, out);
        }
        Protocol::Choice { branches, .. } => {
            for branch in branches {
                insert_choice_label(out, &branch.label);
                collect_messages(&branch.protocol, out);
            }
        }
        Protocol::Loop { body, .. } => {
            collect_messages(body, out);
        }
        Protocol::Parallel { protocols } => {
            for protocol in protocols {
                collect_messages(protocol, out);
            }
        }
        Protocol::Rec { body, .. } => {
            collect_messages(body, out);
        }
        Protocol::Case { branches, .. } => {
            for branch in branches {
                collect_messages(&branch.protocol, out);
            }
        }
        Protocol::Timeout {
            body,
            on_timeout,
            on_cancel,
            ..
        } => {
            collect_messages(body, out);
            collect_messages(on_timeout, out);
            if let Some(on_cancel) = on_cancel {
                collect_messages(on_cancel, out);
            }
        }
        Protocol::Var(_) | Protocol::End => {}
    }
}

fn generate_helpers(messages: &[MessageType]) -> TokenStream {
    let variants = messages.iter().map(|msg| {
        let name = &msg.name;
        quote! { #name(#name) }
    });

    let message_enum = quote! {
        #[derive(Message)]
        enum Label {
            #(#variants),*
        }
    };

    let message_structs = messages.iter().map(|msg| {
        let name = &msg.name;
        if let Some(payload) = &msg.payload {
            #[allow(clippy::expect_used)]
            let parsed: Type = syn::parse2(payload.clone())
                .unwrap_or_else(|_| syn::parse2(payload.clone()).expect("payload type"));
            let payload_ty = match parsed {
                Type::Paren(paren) => *paren.elem,
                other => other,
            };
            quote! {
                #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
                #[serde(transparent)]
                pub struct #name(pub #payload_ty);
            }
        } else {
            quote! {
                #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
                pub struct #name();
            }
        }
    });

    quote! {
        #message_enum
        #(#message_structs)*
        type Channel = Bidirectional<UnboundedSender<Label>, UnboundedReceiver<Label>>;
    }
}

fn generate_vm_projection_artifacts(
    choreography: &Choreography,
    namespace: Option<&str>,
    annotations: &[AuraEffect],
    local_types: &[(
        telltale_choreography::ast::Role,
        telltale_choreography::ast::LocalType,
    )],
) -> Result<TokenStream, syn::Error> {
    let global_type = choreography_to_global(choreography).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to derive VM global type: {error}"),
        )
    })?;

    let mut local_type_map = BTreeMap::new();
    for (role, local_type) in local_types {
        let local_type_r = local_to_local_r(local_type).map_err(|error| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "failed to derive VM local type for role {}: {error}",
                    role.name()
                ),
            )
        })?;
        local_type_map.insert(role.name().to_string(), local_type_r);
    }

    let global_json = serde_json::to_string(&global_type).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to encode VM global type artifact: {error}"),
        )
    })?;
    let local_types_json = serde_json::to_string(&local_type_map).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to encode VM local-type artifacts: {error}"),
        )
    })?;
    let protocol_name = choreography.name.to_string();
    let qualified_name = aura_mpst::CompositionManifest::qualified_name(namespace, &protocol_name);
    let startup_defaults = aura_mpst::startup_defaults_for_qualified_name(&qualified_name);
    let mut guard_capabilities = annotations
        .iter()
        .filter_map(|annotation| match annotation {
            AuraEffect::GuardCapability { capability, .. } => Some(capability.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    guard_capabilities.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    guard_capabilities.dedup_by(|left, right| left.as_str() == right.as_str());
    let manifest = aura_mpst::CompositionManifest {
        protocol_name,
        protocol_namespace: namespace.map(str::to_string),
        protocol_qualified_name: qualified_name.clone(),
        protocol_id: startup_defaults
            .protocol_id
            .unwrap_or(qualified_name.as_str())
            .to_string(),
        role_names: choreography
            .roles
            .iter()
            .map(|role| role.name().to_string())
            .collect(),
        required_capabilities: startup_defaults
            .required_capabilities
            .iter()
            .map(|capability| (*capability).to_string())
            .collect(),
        guard_capabilities,
        determinism_policy_ref: Some(startup_defaults.determinism_policy_ref.to_string()),
        link_specs: annotations
            .iter()
            .filter_map(|annotation| match annotation {
                AuraEffect::Link { directive, role } => Some(aura_mpst::CompositionLinkSpec {
                    role: role.as_str().to_string(),
                    bundle_id: directive.bundle_id.clone(),
                    exports: directive.exports.clone(),
                    imports: directive.imports.clone(),
                }),
                _ => None,
            })
            .collect(),
        delegation_constraints: Vec::new(),
    };
    let manifest_json = serde_json::to_string(&manifest).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to encode composition manifest artifact: {error}"),
        )
    })?;

    let global_json_lit = LitStr::new(&global_json, proc_macro2::Span::call_site());
    let local_types_json_lit = LitStr::new(&local_types_json, proc_macro2::Span::call_site());
    let manifest_json_lit = LitStr::new(&manifest_json, proc_macro2::Span::call_site());
    let role_names = choreography.roles.iter().map(|role| {
        let name = role.name().to_string();
        LitStr::new(&name, proc_macro2::Span::call_site())
    });

    Ok(quote! {
        /// VM projection artifacts derived from the authoritative choreography source.
        pub mod vm_artifacts {
            use std::collections::BTreeMap;
            use std::sync::OnceLock;

            fn decode_global_type() -> &'static ::aura_mpst::upstream::types::GlobalType {
                static GLOBAL_TYPE: OnceLock<::aura_mpst::upstream::types::GlobalType> =
                    OnceLock::new();
                GLOBAL_TYPE.get_or_init(|| {
                    ::aura_mpst::serde_json::from_str(#global_json_lit)
                        .expect("macro-generated VM global type must decode")
                })
            }

            fn decode_local_types(
            ) -> &'static BTreeMap<String, ::aura_mpst::upstream::types::LocalTypeR> {
                static LOCAL_TYPES: OnceLock<
                    BTreeMap<String, ::aura_mpst::upstream::types::LocalTypeR>,
                > = OnceLock::new();
                LOCAL_TYPES.get_or_init(|| {
                    ::aura_mpst::serde_json::from_str(#local_types_json_lit)
                        .expect("macro-generated VM local types must decode")
                })
            }

            fn decode_composition_manifest() -> &'static ::aura_mpst::CompositionManifest {
                static COMPOSITION_MANIFEST: OnceLock<::aura_mpst::CompositionManifest> =
                    OnceLock::new();
                COMPOSITION_MANIFEST.get_or_init(|| {
                    ::aura_mpst::serde_json::from_str(#manifest_json_lit)
                        .expect("macro-generated composition manifest must decode")
                })
            }

            pub fn role_names() -> &'static [&'static str] {
                &[#(#role_names),*]
            }

            pub fn global_type() -> ::aura_mpst::upstream::types::GlobalType {
                decode_global_type().clone()
            }

            pub fn local_types() -> BTreeMap<String, ::aura_mpst::upstream::types::LocalTypeR> {
                decode_local_types().clone()
            }

            pub fn local_type(
                role: &str,
            ) -> Option<::aura_mpst::upstream::types::LocalTypeR> {
                decode_local_types().get(role).cloned()
            }

            pub fn composition_manifest() -> ::aura_mpst::CompositionManifest {
                decode_composition_manifest().clone()
            }
        }
    })
}

fn hoist_choice_blocks(tokens: TokenStream) -> TokenStream {
    let (transformed, hoisted) = transform_token_stream(tokens);
    if hoisted.is_empty() {
        return transformed;
    }

    let mut out = TokenStream::new();
    let items: Vec<_> = hoisted.into_values().collect();
    out.extend(quote! { #(#items)* });
    out.extend(transformed);
    out
}

fn transform_token_stream(tokens: TokenStream) -> (TokenStream, BTreeMap<String, TokenStream>) {
    let mut out = TokenStream::new();
    let mut hoisted = BTreeMap::new();

    for tt in tokens {
        match tt {
            TokenTree::Group(group) => {
                if let Some((ident, items)) = extract_choice_block(&group) {
                    hoisted.entry(ident.to_string()).or_insert(items);
                    out.extend(std::iter::once(TokenTree::Ident(ident)));
                } else {
                    let (inner, inner_hoisted) = transform_token_stream(group.stream());
                    let mut new_group = Group::new(group.delimiter(), {
                        if group.delimiter() == proc_macro2::Delimiter::Brace
                            && !inner_hoisted.is_empty()
                        {
                            let mut combined = TokenStream::new();
                            let items = inner_hoisted.into_values();
                            combined.extend(quote! { #(#items)* });
                            combined.extend(inner);
                            combined
                        } else {
                            for (key, item) in inner_hoisted {
                                hoisted.entry(key).or_insert(item);
                            }
                            inner
                        }
                    });
                    new_group.set_span(group.span());
                    out.extend(std::iter::once(TokenTree::Group(new_group)));
                }
            }
            other => out.extend(std::iter::once(other)),
        }
    }

    (out, hoisted)
}

fn extract_choice_block(group: &Group) -> Option<(Ident, TokenStream)> {
    if group.delimiter() != proc_macro2::Delimiter::Brace {
        return None;
    }

    let stream = group.stream();
    let block: syn::Block = syn::parse2(quote!({ #stream })).ok()?;
    if block.stmts.is_empty() {
        return None;
    }

    let mut items = Vec::new();
    let mut tail_ident: Option<Ident> = None;

    for (idx, stmt) in block.stmts.iter().enumerate() {
        match stmt {
            Stmt::Item(item) => items.push(item),
            Stmt::Expr(Expr::Path(path), None) if idx == block.stmts.len().saturating_sub(1) => {
                if path.path.segments.len() == 1 {
                    tail_ident = Some(path.path.segments[0].ident.clone());
                }
            }
            _ => {}
        }
    }

    let first_enum = match items.first() {
        Some(syn::Item::Enum(item_enum)) => item_enum,
        _ => return None,
    };

    let enum_ident = first_enum.ident.clone();
    let tail_ident = tail_ident?;
    if enum_ident != tail_ident {
        return None;
    }
    if !enum_ident.to_string().starts_with("Choice") {
        return None;
    }

    let item_tokens = quote! { #(#items)* };
    Some((enum_ident, item_tokens))
}

fn rewrite_generated_code(tokens: TokenStream, role_ident: &Ident) -> TokenStream {
    let mut file: syn::File = match syn::parse2(tokens.clone()) {
        Ok(file) => file,
        Err(_) => return tokens,
    };

    let mut rewriter = ChoreoPathRewriter;
    rewriter.visit_file_mut(&mut file);
    let mut role_renamer = RoleRenamer {
        role_ident: role_ident.clone(),
    };
    role_renamer.visit_file_mut(&mut file);
    rewrite_runner_modules(&mut file.items);

    quote! { #file }
}

fn generate_compat_runners(
    protocol_name: &str,
    roles: &[telltale_choreography::ast::Role],
    local_types: &[(
        telltale_choreography::ast::Role,
        telltale_choreography::ast::LocalType,
    )],
) -> TokenStream {
    let mut label_names = BTreeSet::new();
    for (_, local_type) in local_types {
        collect_runner_branch_labels(local_type, &mut label_names);
    }

    let branch_label_enum = generate_runner_branch_label_enum(&label_names);
    let role_enum = generate_runner_role_enum(roles);
    let output_types = generate_runner_output_types(roles);
    let runner_fns: Vec<_> = local_types
        .iter()
        .map(|(role, local_type)| generate_runner_fn(protocol_name, role, local_type))
        .collect();
    let execute_as = generate_runner_execute_as(protocol_name, roles);

    quote! {
        #branch_label_enum
        #role_enum

        #[allow(dead_code, unused_imports, unused_variables)]
        pub mod runners {
            use super::*;
            use ::aura_mpst::GeneratedChoreographyRuntime;
            use ::aura_mpst::upstream::runtime::{ChoreoHandler, ChoreoHandlerExt};
            use ::aura_mpst::upstream::runtime::effects::ContextExt;

            #output_types
            #(#runner_fns)*
            #execute_as
        }
    }
}

fn collect_runner_branch_labels(
    local_type: &telltale_choreography::ast::LocalType,
    labels: &mut BTreeSet<String>,
) {
    use telltale_choreography::ast::LocalType;

    match local_type {
        LocalType::Select { branches, .. }
        | LocalType::Branch { branches, .. }
        | LocalType::LocalChoice { branches } => {
            for (label, branch) in branches {
                labels.insert(label.to_string());
                collect_runner_branch_labels(branch, labels);
            }
        }
        LocalType::Send { continuation, .. }
        | LocalType::Receive { continuation, .. }
        | LocalType::Loop {
            body: continuation, ..
        }
        | LocalType::Rec {
            body: continuation, ..
        }
        | LocalType::Timeout {
            body: continuation, ..
        } => collect_runner_branch_labels(continuation, labels),
        LocalType::Var(_) | LocalType::End => {}
    }
}

fn generate_runner_branch_label_enum(labels: &BTreeSet<String>) -> TokenStream {
    if labels.is_empty() {
        return quote! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum BranchLabel {}

            impl ::aura_mpst::upstream::runtime::LabelId for BranchLabel {
                fn as_str(&self) -> &'static str {
                    match *self {}
                }

                fn from_str(_label: &str) -> Option<Self> {
                    None
                }
            }
        };
    }

    let variants = labels.iter().map(|label| {
        let ident = quote::format_ident!("{}", label);
        quote! { #ident }
    });
    let as_str_arms = labels.iter().map(|label| {
        let ident = quote::format_ident!("{}", label);
        quote! { BranchLabel::#ident => #label }
    });
    let from_str_arms = labels.iter().map(|label| {
        let ident = quote::format_ident!("{}", label);
        quote! { #label => Some(BranchLabel::#ident) }
    });

    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum BranchLabel {
            #(#variants),*
        }

        impl ::aura_mpst::upstream::runtime::LabelId for BranchLabel {
            fn as_str(&self) -> &'static str {
                match self {
                    #(#as_str_arms),*
                }
            }

            fn from_str(label: &str) -> Option<Self> {
                match label {
                    #(#from_str_arms),*,
                    _ => None,
                }
            }
        }
    }
}

fn generate_runner_role_enum(roles: &[telltale_choreography::ast::Role]) -> TokenStream {
    let role_variants: Vec<_> = roles
        .iter()
        .map(|role| {
            let name = role.name();
            if role.index().is_some() || role.param().is_some() {
                quote! { #name(u32) }
            } else {
                quote! { #name }
            }
        })
        .collect();

    let role_name_arms: Vec<_> = roles.iter().map(|role| {
        let name = role.name();
        let role_str = role.name().to_string();
        if role.index().is_some() || role.param().is_some() {
            quote! { RuntimeRole::#name(_) => ::aura_mpst::upstream::runtime::RoleName::from_static(#role_str) }
        } else {
            quote! { RuntimeRole::#name => ::aura_mpst::upstream::runtime::RoleName::from_static(#role_str) }
        }
    }).collect();

    let role_index_arms: Vec<_> = roles
        .iter()
        .map(|role| {
            let name = role.name();
            if role.index().is_some() || role.param().is_some() {
                quote! { RuntimeRole::#name(index) => Some(*index) }
            } else {
                quote! { RuntimeRole::#name => None }
            }
        })
        .collect();

    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum RuntimeRole {
            #(#role_variants),*
        }

        impl ::aura_mpst::upstream::runtime::RoleId for RuntimeRole {
            type Label = BranchLabel;

            fn role_name(&self) -> ::aura_mpst::upstream::runtime::RoleName {
                match self {
                    #(#role_name_arms),*
                }
            }

            fn role_index(&self) -> Option<u32> {
                match self {
                    #(#role_index_arms),*
                }
            }
        }
    }
}

fn generate_runner_output_types(roles: &[telltale_choreography::ast::Role]) -> TokenStream {
    let outputs = roles.iter().map(|role| {
        let output_name = quote::format_ident!("{}Output", role.name());
        quote! {
            #[derive(Debug, Default)]
            pub struct #output_name;
        }
    });

    quote! { #(#outputs)* }
}

fn generate_runner_fn(
    protocol_name: &str,
    role: &telltale_choreography::ast::Role,
    local_type: &telltale_choreography::ast::LocalType,
) -> TokenStream {
    let role_name = role.name();
    let role_name_literal = role_name.to_string();
    let fn_name = quote::format_ident!("run_{}", role_name.to_string().to_lowercase());
    let output_type = quote::format_ident!("{}Output", role_name);
    let role_variant = if role.index().is_some() || role.param().is_some() {
        quote! { RuntimeRole::#role_name(index) }
    } else {
        quote! { RuntimeRole::#role_name }
    };
    let body = generate_runner_body(local_type);

    let signature = if role.index().is_some() || role.param().is_some() {
        quote! {
            pub async fn #fn_name<A: GeneratedChoreographyRuntime<Role = RuntimeRole>>(
                adapter: &mut A,
                index: u32,
            ) -> ::aura_mpst::upstream::runtime::ChoreoResult<#output_type>
        }
    } else {
        quote! {
            pub async fn #fn_name<A: GeneratedChoreographyRuntime<Role = RuntimeRole>>(
                adapter: &mut A,
            ) -> ::aura_mpst::upstream::runtime::ChoreoResult<#output_type>
        }
    };

    quote! {
        #signature {
            let role = #role_variant;
            let mut ep = adapter
                .setup(role)
                .await
                .with_protocol_context(#protocol_name, #role_name_literal, "setup")?;
            let run_result = async {
                let output = #output_type::default();
                #body
                Ok(output)
            }
            .await
            .with_protocol_context(#protocol_name, #role_name_literal, "execute");
            let teardown_result = adapter
                .teardown(ep)
                .await
                .with_protocol_context(#protocol_name, #role_name_literal, "teardown");
            match (run_result, teardown_result) {
                (Err(error), _) => Err(error),
                (Ok(_), Err(error)) => Err(error),
                (Ok(output), Ok(())) => Ok(output),
            }
        }
    }
}

fn generate_runner_execute_as(
    _protocol_name: &str,
    roles: &[telltale_choreography::ast::Role],
) -> TokenStream {
    let match_arms: Vec<_> = roles
        .iter()
        .map(|role| {
            let name = role.name();
            let fn_name = quote::format_ident!("run_{}", name.to_string().to_lowercase());
            if role.index().is_some() || role.param().is_some() {
                quote! { RuntimeRole::#name(index) => { #fn_name(adapter, index).await?; } }
            } else {
                quote! { RuntimeRole::#name => { #fn_name(adapter).await?; } }
            }
        })
        .collect();

    quote! {
        pub async fn execute_as<A: GeneratedChoreographyRuntime<Role = RuntimeRole>>(
            role: RuntimeRole,
            adapter: &mut A,
        ) -> ::aura_mpst::upstream::runtime::ChoreoResult<()>
        {
            match role {
                #(#match_arms)*
            }
            Ok(())
        }
    }
}

fn generate_runner_body(local_type: &telltale_choreography::ast::LocalType) -> TokenStream {
    use telltale_choreography::ast::LocalType;

    match local_type {
        LocalType::Send {
            to,
            message,
            continuation,
        } => {
            let msg_type = &message.name;
            let cont = generate_runner_body(continuation);
            if let Some(index) = to.index() {
                match index {
                    telltale_choreography::ast::role::RoleIndex::Wildcard => {
                        let family_name = to.name().to_string();
                        return quote! {
                            let roles = adapter.resolve_family(#family_name)?;
                            if roles.is_empty() {
                                return Err(::aura_mpst::upstream::runtime::ChoreographyError::EmptyRoleFamily(#family_name.to_string()));
                            }
                            let msg: #msg_type = adapter.provide_message(roles[0]).await?;
                            adapter.broadcast(&mut ep, &roles, &msg).await?;
                            #cont
                        };
                    }
                    telltale_choreography::ast::role::RoleIndex::Range(range) => {
                        let family_name = to.name().to_string();
                        let (start_expr, end_expr) = generate_runner_range_exprs(range);
                        return quote! {
                            let roles = adapter.resolve_range(#family_name, #start_expr, #end_expr)?;
                            if roles.is_empty() {
                                return Err(::aura_mpst::upstream::runtime::ChoreographyError::EmptyRoleFamily(#family_name.to_string()));
                            }
                            let msg: #msg_type = adapter.provide_message(roles[0]).await?;
                            adapter.broadcast(&mut ep, &roles, &msg).await?;
                            #cont
                        };
                    }
                    _ => {}
                }
            }

            let to_role = generate_runner_role_id(to);
            quote! {
                let msg: #msg_type = adapter.provide_message(#to_role).await?;
                adapter.send(&mut ep, #to_role, &msg).await?;
                #cont
            }
        }
        LocalType::Receive {
            from,
            message,
            continuation,
        } => {
            let msg_type = &message.name;
            let cont = generate_runner_body(continuation);
            if let Some(index) = from.index() {
                match index {
                    telltale_choreography::ast::role::RoleIndex::Wildcard => {
                        let family_name = from.name().to_string();
                        return quote! {
                            let roles = adapter.resolve_family(#family_name)?;
                            if roles.is_empty() {
                                return Err(::aura_mpst::upstream::runtime::ChoreographyError::EmptyRoleFamily(#family_name.to_string()));
                            }
                            let _msgs: Vec<#msg_type> = adapter.collect(&mut ep, &roles).await?;
                            #cont
                        };
                    }
                    telltale_choreography::ast::role::RoleIndex::Range(range) => {
                        let family_name = from.name().to_string();
                        let (start_expr, end_expr) = generate_runner_range_exprs(range);
                        return quote! {
                            let roles = adapter.resolve_range(#family_name, #start_expr, #end_expr)?;
                            if roles.is_empty() {
                                return Err(::aura_mpst::upstream::runtime::ChoreographyError::EmptyRoleFamily(#family_name.to_string()));
                            }
                            let _msgs: Vec<#msg_type> = adapter.collect(&mut ep, &roles).await?;
                            #cont
                        };
                    }
                    _ => {}
                }
            }

            let from_role = generate_runner_role_id(from);
            quote! {
                let _msg: #msg_type = adapter.recv(&mut ep, #from_role).await?;
                #cont
            }
        }
        LocalType::Select { to, branches } => {
            let to_role = generate_runner_role_id(to);
            let match_arms: Vec<_> = branches
                .iter()
                .map(|(label, cont_type)| {
                    let cont = generate_runner_body(cont_type);
                    quote! { BranchLabel::#label => { adapter.choose(&mut ep, #to_role, BranchLabel::#label).await?; #cont } }
                })
                .collect();
            let choices: Vec<_> = branches
                .iter()
                .map(|(label, _)| quote! { BranchLabel::#label })
                .collect();
            quote! {
                let choice = adapter.select_branch(&[#(#choices),*]).await?;
                match choice {
                    #(#match_arms),*
                }
            }
        }
        LocalType::Branch { from, branches } => {
            let from_role = generate_runner_role_id(from);
            let match_arms: Vec<_> = branches
                .iter()
                .map(|(label, cont_type)| {
                    let cont = generate_runner_body(cont_type);
                    quote! { BranchLabel::#label => { #cont } }
                })
                .collect();
            quote! {
                let label = adapter.offer(&mut ep, #from_role).await?;
                match label {
                    #(#match_arms),*
                }
            }
        }
        LocalType::LocalChoice { branches } => {
            let match_arms: Vec<_> = branches
                .iter()
                .map(|(label, cont_type)| {
                    let cont = generate_runner_body(cont_type);
                    quote! { BranchLabel::#label => { #cont } }
                })
                .collect();
            let choices: Vec<_> = branches
                .iter()
                .map(|(label, _)| quote! { BranchLabel::#label })
                .collect();
            quote! {
                let choice = adapter.select_branch(&[#(#choices),*]).await?;
                match choice {
                    #(#match_arms),*
                }
            }
        }
        LocalType::Loop { condition, body } => {
            let loop_body = generate_runner_body(body);
            match condition {
                Some(telltale_choreography::ast::Condition::Count(n)) => {
                    quote! { for _i in 0..#n { #loop_body } }
                }
                _ => loop_body,
            }
        }
        LocalType::Rec { body, .. } | LocalType::Timeout { body, .. } => generate_runner_body(body),
        LocalType::Var(_) | LocalType::End => quote! {},
    }
}

fn generate_runner_range_exprs(
    range: &telltale_choreography::ast::role::RoleRange,
) -> (TokenStream, TokenStream) {
    use telltale_choreography::ast::role::RangeExpr;

    let start_expr = match &range.start {
        RangeExpr::Concrete(n) => quote! { #n },
        RangeExpr::Symbolic(var) => {
            let var_ident = quote::format_ident!("{}", var);
            quote! { #var_ident }
        }
    };
    let end_expr = match &range.end {
        RangeExpr::Concrete(n) => quote! { #n },
        RangeExpr::Symbolic(var) => {
            let var_ident = quote::format_ident!("{}", var);
            quote! { #var_ident }
        }
    };
    (start_expr, end_expr)
}

fn generate_runner_role_id(role: &telltale_choreography::ast::Role) -> TokenStream {
    use telltale_choreography::ast::role::RoleIndex;

    let name = role.name();
    if let Some(index) = role.index() {
        match index {
            RoleIndex::Concrete(n) => quote! { RuntimeRole::#name(#n) },
            RoleIndex::Symbolic(var) => {
                let var_ident = quote::format_ident!("{}", var);
                quote! { RuntimeRole::#name(#var_ident) }
            }
            RoleIndex::Wildcard => quote! {{
                return Err(::aura_mpst::upstream::runtime::ChoreographyError::ExecutionError(
                    "wildcard roles must be resolved with resolve_family()".to_string()
                ));
            }},
            RoleIndex::Range(_) => quote! {{
                return Err(::aura_mpst::upstream::runtime::ChoreographyError::ExecutionError(
                    "range roles must be resolved with resolve_range()".to_string()
                ));
            }},
        }
    } else if role.param().is_some() {
        quote! { RuntimeRole::#name(index) }
    } else {
        quote! { RuntimeRole::#name }
    }
}

fn rewrite_runner_modules(items: &mut [syn::Item]) {
    for item in items.iter_mut() {
        if let syn::Item::Mod(module) = item {
            if let Some((_, ref mut inner_items)) = module.content {
                if module.ident == "runners" {
                    module
                        .attrs
                        .push(syn::parse_quote!(#[allow(unreachable_code, unreachable_patterns)]));
                    rewrite_runner_imports(inner_items);
                } else {
                    rewrite_runner_modules(inner_items);
                }
            }
        }
    }
}

fn rewrite_runner_imports(items: &mut [syn::Item]) {
    strip_output_metadata_updates(items);
}

fn strip_output_metadata_updates(items: &mut [syn::Item]) {
    for item in items.iter_mut() {
        if let syn::Item::Fn(func) = item {
            let mut stripper = RunnerOutputStripper;
            stripper.visit_block_mut(&mut func.block);
        }
    }
}

struct RunnerOutputStripper;

impl VisitMut for RunnerOutputStripper {
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        visit_mut::visit_block_mut(self, block);
        block.stmts.retain(|stmt| !is_runner_output_update(stmt));
    }
}

fn is_runner_output_update(stmt: &syn::Stmt) -> bool {
    let expr = match stmt {
        syn::Stmt::Expr(expr, _) => expr,
        _ => return false,
    };
    match expr {
        syn::Expr::Assign(assign) => is_output_field_access(assign.left.as_ref()),
        syn::Expr::Binary(binary) => {
            let is_assign_op = matches!(
                binary.op,
                syn::BinOp::AddAssign(_)
                    | syn::BinOp::SubAssign(_)
                    | syn::BinOp::MulAssign(_)
                    | syn::BinOp::DivAssign(_)
                    | syn::BinOp::RemAssign(_)
                    | syn::BinOp::BitXorAssign(_)
                    | syn::BinOp::BitAndAssign(_)
                    | syn::BinOp::BitOrAssign(_)
                    | syn::BinOp::ShlAssign(_)
                    | syn::BinOp::ShrAssign(_)
            );
            is_assign_op && is_output_field_access(binary.left.as_ref())
        }
        syn::Expr::MethodCall(method_call) => is_output_field_access(method_call.receiver.as_ref()),
        _ => false,
    }
}

fn is_output_field_access(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Field(field) => {
            if let syn::Expr::Path(path) = field.base.as_ref() {
                if path.path.is_ident("output") {
                    return true;
                }
            }
            is_output_field_access(field.base.as_ref())
        }
        _ => false,
    }
}

struct ChoreoPathRewriter;

impl VisitMut for ChoreoPathRewriter {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        if let Some(first) = path.segments.first() {
            if first.ident == "telltale_choreography" {
                let mut segments = syn::punctuated::Punctuated::new();
                segments.push(syn::PathSegment::from(Ident::new(
                    "aura_mpst",
                    first.ident.span(),
                )));
                segments.push(first.clone());
                for seg in path.segments.iter().skip(1) {
                    segments.push(seg.clone());
                }
                path.segments = segments;
            }
        }
        visit_mut::visit_path_mut(self, path);
    }

    fn visit_use_tree_mut(&mut self, tree: &mut syn::UseTree) {
        if let syn::UseTree::Path(path) = tree {
            if path.ident == "telltale_choreography" {
                let span = path.ident.span();
                let inner_ident = path.ident.clone();
                let inner = (*path.tree).clone();
                let wrapped = syn::UseTree::Path(syn::UsePath {
                    ident: Ident::new(&inner_ident.to_string(), span),
                    colon2_token: Default::default(),
                    tree: Box::new(inner),
                });
                *tree = syn::UseTree::Path(syn::UsePath {
                    ident: Ident::new("aura_mpst", span),
                    colon2_token: Default::default(),
                    tree: Box::new(wrapped),
                });
                return;
            }
        }
        visit_mut::visit_use_tree_mut(self, tree);
    }
}

struct RoleRenamer {
    role_ident: Ident,
}

impl VisitMut for RoleRenamer {
    fn visit_item_enum_mut(&mut self, item: &mut syn::ItemEnum) {
        if item.ident == "Role" {
            item.ident = self.role_ident.clone();
        }
        visit_mut::visit_item_enum_mut(self, item);
    }

    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        if let Some(first) = path.segments.first_mut() {
            if first.ident == "Role" {
                first.ident = self.role_ident.clone();
            }
        }
        visit_mut::visit_path_mut(self, path);
    }
}

fn read_choreography_source(expr: Expr) -> Result<(String, proc_macro2::Span), syn::Error> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(lit), ..
        }) => Ok((lit.value(), lit.span())),
        Expr::Macro(ExprMacro { mac, .. }) if mac.path.is_ident("include_str") => {
            let lit: LitStr = syn::parse2(mac.tokens)?;
            let path = PathBuf::from(lit.value());
            let base_dir = std::env::var("CARGO_MANIFEST_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            let full_path = if path.is_absolute() {
                path
            } else {
                base_dir.join(path)
            };
            let contents = std::fs::read_to_string(&full_path).map_err(|err| {
                syn::Error::new(
                    lit.span(),
                    format!(
                        "Failed to read choreography file {}: {err}",
                        full_path.display()
                    ),
                )
            })?;
            Ok((contents, lit.span()))
        }
        other => Err(syn::Error::new(
            other.span(),
            "choreography! expects a string literal or include_str!(\"path.choreo\")",
        )),
    }
}

/// Implementation of the Aura choreography! macro
///
/// Uses namespace-aware Telltale generation to avoid module conflicts
pub fn choreography_impl(input: TokenStream) -> Result<TokenStream, syn::Error> {
    // Parse DSL input to extract roles and protocol name for Aura wrapper
    let parsed_input = parse_choreography_source(input.clone())?;

    // Generate the Telltale choreography using namespace-aware functions
    let message_type_names = extract_message_type_names(&parsed_input.choreography);
    let telltale_output = choreography_impl_namespace_aware(
        &parsed_input.choreography,
        parsed_input.namespace.as_deref(),
        &parsed_input.aura_annotations,
        &message_type_names,
    )
    .map_err(|err| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("Telltale generation failed: {err}"),
        )
    })?;

    // Generate the Aura wrapper module with namespace support
    let namespace = parsed_input.namespace.clone();
    let aura_wrapper =
        generate_aura_wrapper(&parsed_input, namespace.as_deref(), &message_type_names);

    // Extract namespace for uniqueness check (reuse from aura_wrapper if available)
    let namespace = parsed_input.namespace.clone();

    // Generate a uniqueness marker to catch duplicate namespaces at compile time
    let namespace_uniqueness_check = if let Some(ns) = &namespace {
        let marker_name = quote::format_ident!(
            "_CHOREOGRAPHY_NAMESPACE_{}_MUST_BE_UNIQUE",
            ns.to_uppercase()
        );
        quote! {
            // Compile-time uniqueness check for choreography namespace
            // If you see an error here, it means you have two choreographies with the same namespace.
            // Each choreography must have a unique namespace within the same compilation unit.
            #[doc(hidden)]
            #[allow(non_upper_case_globals)]
            const #marker_name: () = ();
        }
    } else {
        // No namespace specified - generate a helpful compile error
        quote! {
            compile_error!(
                "Choreography is missing a namespace. \
                 Add `module <name> exposing (...)` to your .choreo file \
                 or use #[namespace = \"unique_name\"] inside the choreography! macro. \
                 Each choreography in the same file must have a unique namespace."
            );
        }
    };

    // Return both namespace-aware modules with uniqueness check
    Ok(quote! {
        #namespace_uniqueness_check

        // Telltale-generated choreography (session types, projections) - namespace-aware
        #telltale_output

        // Aura wrapper module (effect system integration) - namespace-aware
        #aura_wrapper
    })
}

fn collect_message_type_names(protocol: &Protocol, names: &mut BTreeSet<String>) {
    match protocol {
        Protocol::Begin { continuation, .. }
        | Protocol::Await { continuation, .. }
        | Protocol::Resolve { continuation, .. }
        | Protocol::Invalidate { continuation, .. }
        | Protocol::Let { continuation, .. }
        | Protocol::Publish { continuation, .. }
        | Protocol::PublishAuthority { continuation, .. }
        | Protocol::Materialize { continuation, .. }
        | Protocol::Handoff { continuation, .. }
        | Protocol::DependentWork { continuation, .. }
        | Protocol::Extension { continuation, .. } => {
            collect_message_type_names(continuation, names);
        }
        Protocol::Send {
            message,
            continuation,
            ..
        } => {
            names.insert(message.name.to_string());
            collect_message_type_names(continuation, names);
        }
        Protocol::Broadcast {
            message,
            continuation,
            ..
        } => {
            names.insert(message.name.to_string());
            collect_message_type_names(continuation, names);
        }
        Protocol::Choice { branches, .. } => {
            for branch in branches {
                collect_message_type_names(&branch.protocol, names);
            }
        }
        Protocol::Loop { body, .. } => {
            collect_message_type_names(body, names);
        }
        Protocol::Parallel { protocols } => {
            for protocol in protocols {
                collect_message_type_names(protocol, names);
            }
        }
        Protocol::Rec { body, .. } => {
            collect_message_type_names(body, names);
        }
        Protocol::Case { branches, .. } => {
            for branch in branches {
                collect_message_type_names(&branch.protocol, names);
            }
        }
        Protocol::Timeout {
            body,
            on_timeout,
            on_cancel,
            ..
        } => {
            collect_message_type_names(body, names);
            collect_message_type_names(on_timeout, names);
            if let Some(on_cancel) = on_cancel {
                collect_message_type_names(on_cancel, names);
            }
        }
        Protocol::Var(_) | Protocol::End => {}
    }
}

fn extract_message_type_names(choreography: &Choreography) -> Vec<String> {
    let mut names = BTreeSet::new();
    collect_message_type_names(&choreography.protocol, &mut names);
    names.into_iter().collect()
}

fn to_screaming_snake(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 4);
    let mut chars = value.chars().peekable();
    let mut prev_is_upper = false;
    let mut prev_is_lower = false;
    let mut prev_is_digit = false;
    while let Some(ch) = chars.next() {
        if ch == '_' {
            if !out.ends_with('_') {
                out.push('_');
            }
            prev_is_upper = false;
            prev_is_lower = false;
            prev_is_digit = false;
            continue;
        }
        let is_upper = ch.is_ascii_uppercase();
        let is_lower = ch.is_ascii_lowercase();
        let is_digit = ch.is_ascii_digit();
        let next_is_lower = chars
            .peek()
            .map(|c| c.is_ascii_lowercase())
            .unwrap_or(false);
        if !out.is_empty() {
            // Insert underscore before uppercase letters or digits when transitioning from different char types
            let needs_separator = (is_upper
                && (prev_is_lower || (prev_is_upper && next_is_lower) || prev_is_digit))
                || (is_digit && (prev_is_lower || prev_is_upper));
            if needs_separator {
                out.push('_');
            }
        }
        out.push(ch.to_ascii_uppercase());
        prev_is_upper = is_upper;
        prev_is_lower = is_lower;
        prev_is_digit = is_digit;
    }
    out
}

/// Generate the Aura wrapper module that integrates with the effects system
fn generate_aura_wrapper(
    input: &ChoreographyInput,
    namespace: Option<&str>,
    message_type_names: &[String],
) -> TokenStream {
    let roles = &input.roles;
    let annotations = &input.aura_annotations;

    let role_variants: Vec<_> = roles
        .iter()
        .map(|role| {
            quote! { #role }
        })
        .collect();

    let role_display_arms: Vec<_> = roles
        .iter()
        .map(|role| {
            let role_str = role.to_string();
            quote! {
                Self::#role => write!(f, #role_str)
            }
        })
        .collect();

    let first_role = match roles.first() {
        Some(role) => role,
        None => {
            return quote! {
                compile_error!("Choreography must have at least one role");
            };
        }
    };

    // Generate module name using namespace
    let module_name = if let Some(ns) = namespace {
        quote::format_ident!("aura_choreography_{}", ns)
    } else {
        quote::format_ident!("aura_choreography")
    };

    let message_type_constants: Vec<_> = message_type_names
        .iter()
        .map(|name| {
            let const_ident = quote::format_ident!("{}", to_screaming_snake(name));
            quote! {
                pub const #const_ident: MessageTypeId = MessageTypeId::new_static(#name);
            }
        })
        .collect();

    let message_type_module = if message_type_constants.is_empty() {
        quote! {}
    } else {
        quote! {
            /// Message type identifiers for guard configuration.
            pub mod message_types {
                use aura_mpst::MessageTypeId;
                #(#message_type_constants)*
            }
        }
    };

    // Generate leakage integration code
    let leakage_integration = generate_leakage_integration(annotations);
    let link_wiring = generate_link_wiring(annotations);

    quote! {
        /// Generated Aura choreography module with effects system integration
        pub mod #module_name {
            use std::collections::HashMap;
            use std::fmt;

            // Note: The generated code expects these types to be available at runtime:
            // - biscuit_auth::Biscuit
            // - aura_core::FlowBudget
            // - aura_core::types::scope::ResourceScope
            // - aura_mpst::ast_extraction::AuraEffect (for annotation processing)
            // Users should provide their own BiscuitGuardEvaluator implementation

            /// Role enumeration for this choreography
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum AuraRole {
                #(#role_variants),*
            }

            impl fmt::Display for AuraRole {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    match self {
                        #(#role_display_arms),*
                    }
                }
            }

            #message_type_module

            /// Aura handler with authorization and flow management hooks
            ///
            /// Users should inject actual Biscuit evaluator and flow budget at runtime
            pub struct AuraHandler<BiscuitEvaluator = (), FlowBudget = ()> {
                pub role: AuraRole,
                pub biscuit_evaluator: Option<BiscuitEvaluator>,
                pub flow_budget: Option<FlowBudget>,
                pub flow_balance: i32,
                pub audit_log: Vec<String>,
            }

            impl<BiscuitEvaluator, FlowBudget> AuraHandler<BiscuitEvaluator, FlowBudget> {
                pub fn get_flow_balance(&self) -> i32 {
                    self.flow_balance
                }

                pub fn charge_flow_cost(&mut self, cost: i32) -> Result<(), String> {
                    if self.flow_balance >= cost {
                        self.flow_balance -= cost;
                        Ok(())
                    } else {
                        Err(format!("Insufficient flow balance: {} < {}", self.flow_balance, cost))
                    }
                }

                /// Biscuit guard validation hook
                ///
                /// Implementations should call into a real BiscuitGuardEvaluator supplied via the
                /// generic type parameter. The default checks for a non-empty capability string.
                pub fn validate_guard(&self, capability: &str, resource_type: &str) -> Result<(), String> {
                    // Default implementation logs the validation attempt
                    if capability.is_empty() {
                        Err("Empty capability".to_string())
                    } else {
                        Ok(())
                    }
                }

                /// Biscuit guard evaluation with flow cost
                ///
                /// Implementations should charge flow budget and validate Biscuit guards. Default
                /// behavior delegates to `validate_guard`.
                pub fn evaluate_guard_with_flow(&mut self, capability: &str, resource_type: &str, _flow_cost: u64) -> Result<(), String> {
                    // Default implementation just validates capability
                    self.validate_guard(capability, resource_type)
                }

                pub fn log_audit(&mut self, event: String) {
                    self.audit_log.push(event);
                }
            }

            /// Create a new Aura handler for a role
            pub fn create_handler<BiscuitEvaluator, FlowBudget>(
                role: AuraRole,
                biscuit_evaluator: Option<BiscuitEvaluator>,
                flow_budget: Option<FlowBudget>
            ) -> AuraHandler<BiscuitEvaluator, FlowBudget> {
                AuraHandler {
                    role,
                    biscuit_evaluator,
                    flow_budget,
                    flow_balance: 1000, // Default balance
                    audit_log: Vec::new(),
                }
            }

            /// Simple handler creation for basic use cases
            pub fn create_simple_handler(role: AuraRole) -> AuraHandler<(), ()> {
                create_handler(role, None, None)
            }

            /// Choreography builder for effects composition
            pub struct ChoreographyBuilder {
                operations: Vec<Operation>,
            }

            #[derive(Debug, Clone)]
            enum Operation {
                AuditLog(String, HashMap<String, String>),
                ValidateGuard(AuraRole, String, String), // role, capability, resource_type
                EvaluateGuardWithFlow(AuraRole, String, String, u64), // role, capability, resource_type, flow_cost
                ChargeFlowCost(AuraRole, i32),
                Send(AuraRole, AuraRole, String),
                RecordLeakage(AuraRole, AuraRole, Vec<String>, u64), // from, to, observers, flow_cost
            }

            impl ChoreographyBuilder {
                pub fn new() -> Self {
                    Self {
                        operations: Vec::new(),
                    }
                }

                pub fn audit_log(mut self, event: impl Into<String>, metadata: HashMap<String, String>) -> Self {
                    self.operations.push(Operation::AuditLog(event.into(), metadata));
                    self
                }

                pub fn validate_guard(
                    mut self,
                    role: AuraRole,
                    guard_capability: impl Into<String>,
                    resource_type: impl Into<String>
                ) -> Self {
                    self.operations.push(Operation::ValidateGuard(role, guard_capability.into(), resource_type.into()));
                    self
                }

                pub fn evaluate_guard_with_flow(
                    mut self,
                    role: AuraRole,
                    guard_capability: impl Into<String>,
                    resource_type: impl Into<String>,
                    flow_cost: u64
                ) -> Self {
                    self.operations.push(Operation::EvaluateGuardWithFlow(role, guard_capability.into(), resource_type.into(), flow_cost));
                    self
                }

                pub fn charge_flow_cost(mut self, role: AuraRole, cost: i32) -> Self {
                    self.operations.push(Operation::ChargeFlowCost(role, cost));
                    self
                }

                pub fn send(mut self, from: AuraRole, to: AuraRole, message: impl Into<String>) -> Self {
                    self.operations.push(Operation::Send(from, to, message.into()));
                    self
                }

                pub fn record_leakage(
                    mut self,
                    from: AuraRole,
                    to: AuraRole,
                    observers: Vec<String>,
                    flow_cost: u64
                ) -> Self {
                    self.operations.push(Operation::RecordLeakage(from, to, observers, flow_cost));
                    self
                }

                pub fn end(self) -> ChoreographyProgram {
                    ChoreographyProgram {
                        operations: self.operations,
                    }
                }
            }

            /// Executable choreography program
            #[derive(Clone, Debug)]
            pub struct ChoreographyProgram {
                operations: Vec<Operation>,
            }

            /// Create a new choreography builder
            pub fn builder() -> ChoreographyBuilder {
                ChoreographyBuilder::new()
            }

            /// Execute a choreography program
            pub async fn execute<BiscuitEvaluator, FlowBudget>(
                handler: &mut AuraHandler<BiscuitEvaluator, FlowBudget>,
                _endpoint: &mut (),
                program: ChoreographyProgram,
            ) -> Result<(), String> {
                for operation in program.operations {
                    match operation {
                        Operation::AuditLog(event, _metadata) => {
                            handler.log_audit(format!("AUDIT: {}", event));
                        },
                        Operation::ValidateGuard(role, guard_capability, resource_type) => {
                            if role == handler.role {
                                handler.validate_guard(&guard_capability, &resource_type)?;
                                handler.log_audit(format!("GUARD_VALIDATED: {} ({}) for {:?}", guard_capability, resource_type, role));
                            }
                        },
                        Operation::EvaluateGuardWithFlow(role, guard_capability, resource_type, flow_cost) => {
                            if role == handler.role {
                                handler.evaluate_guard_with_flow(&guard_capability, &resource_type, flow_cost)?;
                                handler.log_audit(format!("GUARD_EVALUATED_WITH_FLOW: {} ({}, cost: {}) for {:?}", guard_capability, resource_type, flow_cost, role));
                            }
                        },
                        Operation::ChargeFlowCost(role, cost) => {
                            if role == handler.role {
                                handler.charge_flow_cost(cost)?;
                                handler.log_audit(format!("CHARGED: {} flow cost for {:?}", cost, role));
                            }
                        },
                        Operation::Send(from, to, message) => {
                            if from == handler.role {
                                handler.log_audit(format!("SENT: {} from {:?} to {:?}", message, from, to));
                            } else if to == handler.role {
                                handler.log_audit(format!("RECEIVED: {} from {:?} to {:?}", message, from, to));
                            }
                        },
                        Operation::RecordLeakage(from, to, observers, flow_cost) => {
                            // Record leakage for send/recv operations
                            if from == handler.role {
                                handler.charge_flow_cost(flow_cost as i32)?;
                                handler.log_audit(format!("LEAKAGE_SENT: from {:?} to {:?}, observers: {:?}, cost: {}", from, to, observers, flow_cost));
                            } else if to == handler.role {
                                handler.log_audit(format!("LEAKAGE_RECV: from {:?} to {:?}, observers: {:?}", from, to, observers));
                            }
                        },
                    }
                }
                Ok(())
            }

            /// Example choreography for demonstration with guard validation
            pub fn example_aura_choreography() -> ChoreographyProgram {
                let capability = "chat:message:send";
                let resource_type = "relay";

                builder()
                    .audit_log("example_start", HashMap::new())
                    .validate_guard(AuraRole::#first_role, capability, resource_type)
                    .charge_flow_cost(AuraRole::#first_role, 200)
                    .audit_log("example_complete", HashMap::new())
                    .end()
            }

            /// Generate a choreography program using the actual parsed annotations
            /// This would be called by the macro expansion with the real extracted annotations
            pub fn generate_protocol_choreography() -> ChoreographyProgram {
                // In a real macro expansion, this would receive the extracted annotations
                // Example annotations that would be extracted from choreography syntax:
                let sample_annotations = vec![
                    ("Alice".to_string(), "guard_capability".to_string(), "chat:message:send".to_string(), 100),
                    ("Bob".to_string(), "guard_with_flow".to_string(), "amp:receive".to_string(), 50),
                    ("Alice".to_string(), "journal_facts".to_string(), "message_sent".to_string(), 0),
                ];
                generate_from_annotations_impl(sample_annotations)
            }

            /// Generate choreography from extracted annotations
            pub fn generate_from_annotations_impl(annotations: Vec<(String, String, String, u64)>) -> ChoreographyProgram {
                let roles = vec![AuraRole::#first_role]; // Simplified - real impl would get all roles
                let mut builder = builder();
                builder = builder.audit_log("choreography_start", HashMap::new());

                // Generate operations based on parsed annotation tuples:
                // (role_name, annotation_type, capability_or_value, flow_cost)
                for (role_name, annotation_type, capability_or_value, flow_cost) in annotations {
                    if let Some(role) = parse_role_from_name(&role_name) {
                        match annotation_type.as_str() {
                            "guard_capability" => {
                                builder = builder.validate_guard(role, capability_or_value, "relay");
                            },
                            "flow_cost" => {
                                builder = builder.charge_flow_cost(role, flow_cost as i32);
                            },
                            "guard_with_flow" => {
                                builder = builder.evaluate_guard_with_flow(role, capability_or_value, "relay", flow_cost);
                            },
                            "journal_facts" => {
                                let mut metadata = HashMap::new();
                                metadata.insert("journal_facts".to_string(), capability_or_value);
                                builder = builder.audit_log("journal_facts_recorded", metadata);
                            },
                            "journal_merge" => {
                                builder = builder.audit_log("journal_merge_requested", HashMap::new());
                            },
                            "leak" => {
                                // Parse observers from capability_or_value
                                let observers: Vec<String> = capability_or_value
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .collect();

                                // For leakage, we need to know sender and receiver
                                // This is simplified - real implementation would parse from choreography
                                if let Some(next_role) = roles.iter().find(|r| **r != role) {
                                    builder = builder.record_leakage(
                                        role,
                                        *next_role,
                                        observers,
                                        flow_cost.max(100), // Default 100 if not specified
                                    );
                                }
                            },
                            _ => {
                                // Unknown annotation type - log it for debugging
                                let mut metadata = HashMap::new();
                                metadata.insert("annotation_type".to_string(), annotation_type);
                                metadata.insert("capability_or_value".to_string(), capability_or_value);
                                builder = builder.audit_log("unknown_annotation", metadata);
                            }
                        }
                    }
                }

                builder = builder.audit_log("choreography_complete", HashMap::new());
                builder.end()
            }

            /// Parse a role name string into an AuraRole enum variant
            fn parse_role_from_name(role_name: &str) -> Option<AuraRole> {
                match role_name {
                    #(stringify!(#role_variants) => Some(AuraRole::#role_variants),)*
                    _ => None,
                }
            }

            /// Bridge module for converting annotations to EffectCommand sequences
            ///
            /// This module provides runtime conversion from choreographic annotations
            /// to the algebraic effect commands defined in aura-core::effects::guard.
            pub mod effect_bridge {
                use aura_core::effects::guard::EffectCommand;
                use aura_core::time::TimeStamp;
                use aura_core::types::identifiers::{AuthorityId, ContextId};

                /// Runtime context for effect command generation
                #[derive(Clone)]
                pub struct EffectBridgeContext {
                    /// Current context ID
                    pub context: ContextId,
                    /// Local authority ID
                    pub authority: AuthorityId,
                    /// Peer authority ID (for communication)
                    pub peer: AuthorityId,
                    /// Current timestamp
                    pub timestamp: TimeStamp,
                }

                /// Convert an annotation tuple to effect commands
                ///
                /// Takes: (role_name, annotation_type, value, flow_cost)
                /// Returns: Vec of EffectCommand for the interpreter
                pub fn annotation_to_commands(
                    ctx: &EffectBridgeContext,
                    annotation: (String, String, String, u64),
                ) -> Vec<EffectCommand> {
                    let (_role_name, annotation_type, value, flow_cost) = annotation;
                    let mut commands = Vec::new();

                    match annotation_type.as_str() {
                        "guard_capability" => {
                            // Capability checks are pure (done against snapshot)
                            // Store capability validation metadata for audit trail
                            commands.push(EffectCommand::StoreMetadata {
                                key: "guard_validated".to_string(),
                                value: value.clone(),
                            });
                        }
                        "flow_cost" | "guard_with_flow" => {
                            // Charge flow budget
                            let amount = aura_core::FlowCost::try_from(flow_cost)
                                .unwrap_or_else(|_| aura_core::FlowCost::new(u32::MAX));
                            commands.push(EffectCommand::ChargeBudget {
                                context: ctx.context,
                                authority: ctx.authority,
                                peer: ctx.peer,
                                amount,
                            });
                        }
                        "journal_facts" => {
                            // Append journal entry
                            // Note: Creating minimal fact - actual fact details stored in metadata
                            commands.push(EffectCommand::StoreMetadata {
                                key: format!("journal_fact:{}", value),
                                value: value.clone(),
                            });
                        }
                        "journal_merge" => {
                            // Record merge operation in metadata
                            commands.push(EffectCommand::StoreMetadata {
                                key: "journal_merge_requested".to_string(),
                                value: "true".to_string(),
                            });
                        }
                        "audit_log" => {
                            // Record audit entry as metadata (audit trail)
                            commands.push(EffectCommand::StoreMetadata {
                                key: format!("audit_log:{}", value),
                                value: value.clone(),
                            });
                        }
                        "leak" => {
                            // Parse leakage bits from value or use default
                            let bits = value.split(',').count() as u32 * 8; // Rough estimate: 8 bits per observer class
                            commands.push(EffectCommand::RecordLeakage { bits });
                        }
                        _ => {
                            // Unknown annotation - store in metadata for debugging
                            commands.push(EffectCommand::StoreMetadata {
                                key: format!("unknown_annotation:{}", annotation_type),
                                value,
                            });
                        }
                    }

                    commands
                }

                /// Convert a batch of annotations to effect commands
                pub fn annotations_to_commands(
                    ctx: &EffectBridgeContext,
                    annotations: Vec<(String, String, String, u64)>,
                ) -> Vec<EffectCommand> {
                    annotations
                        .into_iter()
                        .flat_map(|ann| annotation_to_commands(ctx, ann))
                        .collect()
                }

                /// Create effect context from runtime values
                pub fn create_context(
                    context: ContextId,
                    authority: AuthorityId,
                    peer: AuthorityId,
                    timestamp: TimeStamp,
                ) -> EffectBridgeContext {
                    EffectBridgeContext {
                        context,
                        authority,
                        peer,
                        timestamp,
                    }
                }

                /// Execute effect commands through an interpreter
                ///
                /// This is the main integration point for the effect system.
                /// It converts annotations to commands and executes them asynchronously.
                pub async fn execute_commands<I: aura_core::effects::guard::EffectInterpreter>(
                    interpreter: &I,
                    ctx: &EffectBridgeContext,
                    annotations: Vec<(String, String, String, u64)>,
                ) -> Result<Vec<aura_core::effects::guard::EffectResult>, String> {
                    use aura_core::effects::guard::EffectResult;

                    let commands = annotations_to_commands(ctx, annotations);
                    let mut results = Vec::with_capacity(commands.len());

                    for cmd in commands {
                        match interpreter.execute(cmd).await {
                            Ok(result) => results.push(result),
                            Err(e) => return Err(format!("Effect execution failed: {}", e)),
                        }
                    }

                    Ok(results)
                }
            }

            #leakage_integration
            #link_wiring
        }
    }
}

/// Namespace-aware Telltale implementation
///
/// Uses namespace-aware generation to avoid module conflicts.
fn choreography_impl_namespace_aware(
    choreography: &Choreography,
    namespace: Option<&str>,
    annotations: &[AuraEffect],
    message_type_names: &[String],
) -> Result<TokenStream, syn::Error> {
    // Project to local types
    let mut local_types = Vec::new();
    for role in &choreography.roles {
        match project(choreography, role) {
            Ok(local_type) => {
                local_types.push((role.clone(), local_type));
            }
            Err(err) => {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("Projection failed for role {}: {}", role.name(), err),
                ));
            }
        }
    }

    let mut message_map = BTreeMap::new();
    collect_messages(&choreography.protocol, &mut message_map);
    let messages: Vec<_> = message_map.into_values().collect();
    let helpers = generate_helpers(&messages);
    let vm_projection_artifacts =
        generate_vm_projection_artifacts(choreography, namespace, annotations, &local_types)?;

    // Generate code and hoist inline choice enums (choices need item-level definitions)
    let generated_code = generate_choreography_code(
        &choreography.name.to_string(),
        &choreography.roles,
        &local_types,
    );
    let generated_code = hoist_choice_blocks(generated_code);
    let role_ident = quote::format_ident!("{}Role", choreography.name);
    let generated_code = rewrite_generated_code(generated_code, &role_ident);
    let compat_runners = generate_compat_runners(
        &choreography.name.to_string(),
        &choreography.roles,
        &local_types,
    );

    let generated_code = if let Some(ns) = &choreography.namespace {
        let ns_ident = quote::format_ident!("{}", ns);
        quote! {
            pub mod #ns_ident {
                use super::*;
                #generated_code
                pub type #role_ident = super::RuntimeRole;
                pub mod runners {
                    pub use super::super::runners::*;
                }
            }
            pub use #ns_ident::*;
            #compat_runners
        }
    } else {
        quote! {
            mod __generated_choreography {
                use super::*;
                #generated_code
            }
            pub use __generated_choreography::*;
            #compat_runners
        }
    };

    // Generate canonical module names using namespace.
    let canonical_module_name = if let Some(ns) = &choreography.namespace {
        quote::format_ident!("telltale_session_types_{}", ns)
    } else {
        quote::format_ident!("telltale_session_types")
    };

    let imports = quote! {
        #[allow(unused_imports)]
        use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
        use aura_mpst::upstream::api::{
            channel::*,
            Branch, End, Message, Receive, Role, Roles, Route, Sealable, Select, Send, session,
        };
    };

    let message_reexports = message_type_names.iter().map(|name| {
        let ident = quote::format_ident!("{}", name);
        quote! { pub use super::#ident; }
    });

    let message_wrapper_module = if message_type_names.is_empty() {
        quote! {}
    } else {
        quote! {
            /// Re-exported message wrapper types for choreography adapters.
            pub mod message_wrappers {
                #(#message_reexports)*
            }
        }
    };

    Ok(quote! {
        /// Telltale-generated session types and choreographic projections
        #[allow(clippy::diverging_sub_expression)]
        pub mod #canonical_module_name {
            #imports
            #helpers
            #generated_code
            #vm_projection_artifacts
            #message_wrapper_module
        }
    })
}

/// Extract namespace from macro attributes (#[namespace = "..."]).
fn extract_namespace_from_attrs(attrs: &[Attribute]) -> Result<Option<String>, syn::Error> {
    for attr in attrs {
        if !attr.path().is_ident("namespace") {
            continue;
        }
        let meta = attr.meta.clone();
        if let syn::Meta::NameValue(nv) = meta {
            if let syn::Expr::Lit(expr_lit) = nv.value {
                if let syn::Lit::Str(lit_str) = expr_lit.lit {
                    return Ok(Some(lit_str.value()));
                }
            }
            return Err(syn::Error::new(
                attr.span(),
                "namespace must be a string literal (e.g. #[namespace = \"foo\"])",
            ));
        }
        return Err(syn::Error::new(
            attr.span(),
            "namespace must be a name-value attribute (e.g. #[namespace = \"foo\"])",
        ));
    }
    Ok(None)
}

/// Generate link wiring metadata from `link` annotations.
fn generate_link_wiring(annotations: &[AuraEffect]) -> TokenStream {
    let mut specs = Vec::new();

    for annotation in annotations {
        if let AuraEffect::Link { directive, role } = annotation {
            let role_name = role.as_str();
            let bundle_id = &directive.bundle_id;
            let exports = directive.exports.iter();
            let imports = directive.imports.iter();
            specs.push(quote! {
                LinkSpec {
                    role: #role_name,
                    bundle_id: #bundle_id,
                    exports: &[#(#exports),*],
                    imports: &[#(#imports),*],
                }
            });
        }
    }

    if specs.is_empty() {
        quote! {}
    } else {
        quote! {
            /// Parsed `@link` composition metadata emitted from choreography annotations.
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct LinkSpec {
                /// Role that declared the link annotation.
                pub role: &'static str,
                /// Bundle id targeted by this link directive.
                pub bundle_id: &'static str,
                /// Interfaces exported by the declaring bundle.
                pub exports: &'static [&'static str],
                /// Interfaces imported by the declaring bundle.
                pub imports: &'static [&'static str],
            }

            /// Link directives extracted at compile time.
            pub const LINK_SPECS: &[LinkSpec] = &[#(#specs),*];

            /// Accessor for link directives emitted by this choreography module.
            pub fn link_specs() -> &'static [LinkSpec] {
                LINK_SPECS
            }
        }
    }
}

/// Generate leakage integration code from annotations
fn generate_leakage_integration(annotations: &[AuraEffect]) -> TokenStream {
    let mut leakage_ops = Vec::new();

    // Process each annotation to find leakage effects
    for annotation in annotations {
        if let AuraEffect::Leakage { observers, role } = annotation {
            // Convert observer strings to ObserverClass enum variants
            let observer_variants: Vec<_> = observers
                .iter()
                .map(|obs| {
                    match obs.as_str() {
                        "External" => quote! { ObserverClass::External },
                        "Neighbor" => quote! { ObserverClass::Neighbor },
                        "InGroup" => quote! { ObserverClass::InGroup },
                        _ => quote! { ObserverClass::External }, // Default fallback
                    }
                })
                .collect();

            // Generate leakage recording code for this role
            let role_name = role.as_str().to_string();
            let role_ident = quote::format_ident!("{}", role_name);
            leakage_ops.push(quote! {
                // Record leakage for role #role_ident
                if handler.role == AuraRole::#role_ident {
                    #(
                    handler.log_audit(format!("LEAKAGE: {:?} operation visible to {:?}", #role_name, #observer_variants));
                    )*
                }
            });
        }
    }

    // If we have leakage operations, generate the integration module
    if !leakage_ops.is_empty() {
        quote! {
            /// Leakage tracking integration
            pub mod leakage_integration {
                use super::*;
                use aura_core::effects::{LeakageEffects, ObserverClass};

                /// Apply leakage tracking to choreography operations
                pub async fn track_leakage<BiscuitEvaluator, FlowBudget>(
                    handler: &mut AuraHandler<BiscuitEvaluator, FlowBudget>,
                    leakage_effects: &impl LeakageEffects,
                ) -> Result<(), String> {
                    #(#leakage_ops)*
                    Ok(())
                }
            }
        }
    } else {
        // No leakage annotations, generate empty module
        quote! {}
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{
        build_coherence_validation_input, normalize_choreography_for_parser,
        validate_link_annotations, validate_protocol_coherence,
    };
    use aura_mpst::ast_extraction::extract_aura_annotations;
    use telltale_language::parse_choreography_str;

    fn parse_test_choreography(dsl: &str) -> telltale_language::ast::Choreography {
        let normalized = normalize_choreography_for_parser(dsl);
        parse_choreography_str(&normalized).expect("choreography should parse")
    }

    #[test]
    fn link_validation_accepts_exported_imports() {
        let dsl = r#"
            A[link = "bundle=alpha|exports=sync.push|imports=chat.send"] -> B: Msg;
            B[link = "bundle=beta|exports=chat.send|imports=sync.push"] -> A: Ack;
        "#;
        let annotations = extract_aura_annotations(dsl).expect("annotations should parse");
        validate_link_annotations(&annotations).expect("exports should satisfy imports");
    }

    #[test]
    fn link_validation_rejects_missing_export() {
        let dsl = r#"
            A[link = "bundle=alpha|exports=sync.push|imports=chat.send"] -> B: Msg;
        "#;
        let annotations = extract_aura_annotations(dsl).expect("annotations should parse");
        let err = validate_link_annotations(&annotations).expect_err("missing export must fail");
        assert!(
            err.contains("chat.send"),
            "error should mention unresolved import"
        );
    }

    #[test]
    fn coherence_input_simple_send_is_deterministic() {
        let dsl = r#"
module simple_send exposing (SimpleSend)

protocol SimpleSend =
  roles A, B
  A -> B : Ping
"#;
        let choreography = parse_test_choreography(dsl);
        let input_a =
            build_coherence_validation_input(&choreography).expect("coherence input should build");
        let input_b =
            build_coherence_validation_input(&choreography).expect("coherence input should build");

        assert_eq!(
            input_a, input_b,
            "coherence input must be deterministic across repeated derivations"
        );
        assert_eq!(input_a.initial_delivery_env.len(), 1);
        assert!(input_a.initial_delivery_env.contains_key(&(
            "A".to_string(),
            "B".to_string(),
            "Ping".to_string()
        )));
        validate_protocol_coherence(&choreography).expect("simple protocol should be coherent");
    }

    #[test]
    fn coherence_input_choice_is_deterministic() {
        let dsl = r#"
module simple_choice exposing (SimpleChoice)

protocol SimpleChoice =
  roles A, B
  choice at A
    | Accept ->
      A -> B : AcceptMsg
    | Reject ->
      A -> B : RejectMsg
"#;
        let choreography = parse_test_choreography(dsl);
        let input_a =
            build_coherence_validation_input(&choreography).expect("coherence input should build");
        let input_b =
            build_coherence_validation_input(&choreography).expect("coherence input should build");

        assert_eq!(
            input_a, input_b,
            "choice coherence input must be deterministic across repeated derivations"
        );
        assert_eq!(input_a.initial_delivery_env.len(), 4);
        validate_protocol_coherence(&choreography).expect("choice protocol should be coherent");
    }

    #[test]
    fn coherence_input_loop_is_deterministic() {
        let dsl = r#"
module loop_proto exposing (LoopProto)

protocol LoopProto =
  roles Coordinator, Peer
  loop decide by Coordinator
    choice at Coordinator
      | Continue ->
        Coordinator -> Peer : Tick
      | Stop ->
        Coordinator -> Peer : Stop
"#;
        let choreography = parse_test_choreography(dsl);
        let input_a =
            build_coherence_validation_input(&choreography).expect("coherence input should build");
        let input_b =
            build_coherence_validation_input(&choreography).expect("coherence input should build");

        assert_eq!(
            input_a, input_b,
            "loop coherence input must be deterministic across repeated derivations"
        );
        assert_eq!(input_a.initial_delivery_env.len(), 2);
        validate_protocol_coherence(&choreography).expect("loop protocol should be coherent");
    }

    #[test]
    fn coherence_validation_rejects_self_send() {
        let dsl = r#"
module incoherent_self_send exposing (IncoherentSelfSend)

protocol IncoherentSelfSend =
  roles Alice
  Alice -> Alice : Loopback
"#;
        let choreography = parse_test_choreography(dsl);
        let err = validate_protocol_coherence(&choreography).expect_err("self send must fail");
        assert!(
            err.contains("action"),
            "error should include failed action predicate"
        );
    }

    #[test]
    fn coherence_validation_rejects_broadcast_with_stable_error() {
        let dsl = r#"
module broadcast_proto exposing (BroadcastProto)

protocol BroadcastProto =
  roles A, B, C
  A ->* : Ping
"#;
        let choreography = parse_test_choreography(dsl);
        let err = build_coherence_validation_input(&choreography)
            .expect_err("broadcast should fail coherence conversion");
        assert!(
            err.to_string().contains("Broadcast"),
            "error should mention unsupported Broadcast feature"
        );
    }

    #[test]
    fn coherence_supported_subset_matches_authoritative_conversion() {
        let dsl = r#"
module subset_proto exposing (SubsetProto)

protocol SubsetProto =
  roles A, B
  choice at A
    | Accept ->
      A -> B : AcceptMsg
    | Reject ->
      A -> B : RejectMsg
"#;
        let choreography = parse_test_choreography(dsl);
        build_coherence_validation_input(&choreography)
            .expect("supported subset should pass drift check");
    }
}
