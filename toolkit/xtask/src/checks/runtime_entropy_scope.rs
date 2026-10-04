//! Narrow typed origin proof for the private runtime nonproduction seed owner.
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;
use syn::{spanned::Spanned, visit::Visit};

const OWNER: &str = "NonProductionEntropySeed";
fn named(ty: &syn::Type, name: &str) -> bool {
    matches!(ty, syn::Type::Path(ty) if ty.qself.is_none() && ty.path.is_ident(name))
}
fn path(expr: &syn::Expr, name: &str) -> bool {
    matches!(expr, syn::Expr::Path(expr) if expr.qself.is_none() && expr.path.is_ident(name))
}
fn call(expr: &syn::Expr, name: &str) -> bool {
    matches!(expr, syn::Expr::Call(expr) if path(&expr.func,name))
}
fn method(expr: &syn::Expr, receiver: &str, name: &str) -> bool {
    matches!(expr, syn::Expr::MethodCall(expr) if expr.method == name && path(&expr.receiver,receiver) && expr.args.is_empty())
}
fn declaration(attrs: &[syn::Attribute], family: &str, receiver: bool) -> bool {
    attrs.iter().any(|attr| {
        if !attr
            .path()
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .eq(["aura_macros", "capability_boundary"].map(str::to_owned))
        {
            return false;
        }
        let mut category = false;
        let mut capability = false;
        let mut actual_family = false;
        let mut ty = false;
        let mut actual_receiver = !receiver;
        let parsed = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("capability_type") || meta.path.is_ident("receiver_type") {
                let value: syn::Type = meta.value()?.parse()?;
                if meta.path.is_ident("capability_type") {
                    ty = named(&value, OWNER);
                } else {
                    actual_receiver = named(&value, OWNER);
                }
            } else {
                let value: syn::LitStr = meta.value()?.parse()?;
                if meta.path.is_ident("category") {
                    category = value.value() == "capability_gated";
                }
                if meta.path.is_ident("capability") {
                    capability = value.value() == OWNER;
                }
                if meta.path.is_ident("family") {
                    actual_family = value.value() == family;
                }
            }
            Ok(())
        });
        parsed.is_ok() && category && capability && actual_family && ty && actual_receiver
    })
}
fn proof_source(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr
            .path()
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .eq(["aura_macros", "authoritative_source"].map(str::to_owned))
        {
            return false;
        }
        let mut proved = false;
        let parsed = attr.parse_nested_meta(|meta| {
            let value: syn::LitStr = meta.value()?.parse()?;
            proved = meta.path.is_ident("kind") && value.value() == "proof_issuer";
            Ok(())
        });
        parsed.is_ok() && proved
    })
}
fn canonical_container(path: &syn::Path, name: &str, module: &str) -> bool {
    let parts = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    (parts == [name] || parts == ["std", module, name] || parts == ["core", module, name])
        && path
            .segments
            .iter()
            .take(path.segments.len().saturating_sub(1))
            .all(|segment| matches!(segment.arguments, syn::PathArguments::None))
}
fn returns_seed_capability(output: &syn::ReturnType) -> bool {
    let syn::ReturnType::Type(_, output) = output else {
        return false;
    };
    let syn::Type::Path(result) = &**output else {
        return false;
    };
    if result.qself.is_some() || !canonical_container(&result.path, "Result", "result") {
        return false;
    }
    let Some(segment) = result.path.segments.last() else {
        return false;
    };
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    if args.args.len() != 2 {
        return false;
    }
    let Some(syn::GenericArgument::Type(syn::Type::Path(error))) = args.args.last() else {
        return false;
    };
    let names = error
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    if error.qself.is_some()
        || !(names == ["AuraError"] || names == ["aura_core", "AuraError"])
        || error
            .path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
    {
        return false;
    }
    let Some(syn::GenericArgument::Type(syn::Type::Path(option))) = args.args.first() else {
        return false;
    };
    if option.qself.is_some() || !canonical_container(&option.path, "Option", "option") {
        return false;
    }
    let Some(option) = option.path.segments.last() else {
        return false;
    };
    let syn::PathArguments::AngleBracketed(args) = &option.arguments else {
        return false;
    };
    args.args.len() == 1
        && matches!(args.args.first(),Some(syn::GenericArgument::Type(ty)) if named(ty,OWNER))
}
fn factory(item: &syn::ImplItemFn) -> bool {
    if item.sig.ident != "admit"
        || !declaration(&item.attrs, "proof_issuer", false)
        || !proof_source(&item.attrs)
        || item.block.stmts.len() != 2
    {
        return false;
    }
    if item.sig.inputs.len() != 2 {
        return false;
    }
    if !returns_seed_capability(&item.sig.output) {
        return false;
    }
    let mut inputs = item.sig.inputs.iter();
    if !matches!(inputs.next(),Some(syn::FnArg::Typed(arg)) if matches!(&*arg.pat,syn::Pat::Ident(pat) if pat.ident == "mode") && matches!(&*arg.ty,syn::Type::Path(ty) if ty.qself.is_none() && ty.path.segments.iter().map(|s|s.ident.to_string()).eq(["aura_core","effects","ExecutionMode"].map(str::to_owned))))
    {
        return false;
    }
    if !matches!(inputs.next(),Some(syn::FnArg::Typed(arg)) if matches!(&*arg.pat,syn::Pat::Ident(pat) if pat.ident == "seed"))
    {
        return false;
    }
    let syn::Stmt::Expr(syn::Expr::If(guard), _) = &item.block.stmts[0] else {
        return false;
    };
    let syn::Expr::Binary(condition) = &*guard.cond else {
        return false;
    };
    if !matches!(condition.op, syn::BinOp::And(_))
        || !method(&condition.left, "mode", "is_production")
        || !method(&condition.right, "seed", "is_some")
        || guard.else_branch.is_some()
    {
        return false;
    }
    if !matches!(guard.then_branch.stmts.as_slice(),[syn::Stmt::Expr(syn::Expr::Return(ret),_)] if ret.expr.as_ref().is_some_and(|expr|call(expr,"Err")))
    {
        return false;
    }
    let syn::Stmt::Expr(syn::Expr::Call(result), None) = &item.block.stmts[1] else {
        return false;
    };
    if !path(&result.func, "Ok") || result.args.len() != 1 {
        return false;
    }
    matches!(result.args.first(),Some(syn::Expr::MethodCall(map)) if path(&map.receiver,"seed") && map.method == "map" && map.args.len()==1 && map.args.first().is_some_and(|expr|path(expr,"Self")))
}
#[derive(Default)]
struct SeedSites {
    lines: BTreeSet<usize>,
    invalid: bool,
}
impl<'ast> Visit<'ast> for SeedSites {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let syn::Expr::Path(callee) = &*call.func else {
            syn::visit::visit_expr_call(self, call);
            return;
        };
        let Some(last) = callee.path.segments.last() else {
            return;
        };
        if ["for_simulation_seed", "deterministic", "from_seed"]
            .iter()
            .any(|name| last.ident == *name)
        {
            let valid = call.args.len() == 1
                && matches!(call.args.first(),Some(syn::Expr::Field(field)) if path(&field.base,"self") && matches!(&field.member,syn::Member::Unnamed(index) if index.index==0))
                || last.ident == "deterministic"
                    && call.args.len() == 1
                    && matches!(call.args.first(),Some(syn::Expr::Call(inner)) if matches!(&*inner.func,syn::Expr::Path(path) if path.path.segments.last().is_some_and(|s|s.ident=="from_seed")));
            self.invalid |= !valid;
            self.lines
                .extend(call.span().start().line..=call.span().end().line);
        }
        syn::visit::visit_expr_call(self, call);
    }
}
pub(super) fn is_seed_owner_path(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .is_ok_and(|relative| relative == Path::new("crates/aura-agent/src/runtime/entropy.rs"))
}
fn canonical_imports(file: &syn::File) -> bool {
    let imported_error=file.items.iter().any(|item|matches!(item,syn::Item::Use(item) if matches!(&item.tree,syn::UseTree::Path(path) if path.ident=="aura_core" && matches!(&*path.tree,syn::UseTree::Name(name) if name.ident=="AuraError"))));
    let supported_imports=file.items.iter().all(|item| match item {
        syn::Item::Use(item) => match &item.tree {
            syn::UseTree::Path(path) if path.ident=="aura_core" => matches!(&*path.tree,syn::UseTree::Name(name) if name.ident=="AuraError"),
            syn::UseTree::Path(path) if path.ident=="std" => matches!(&*path.tree,syn::UseTree::Path(sync) if sync.ident=="sync" && matches!(&*sync.tree,syn::UseTree::Name(name) if name.ident=="Arc")),
            _ => false,
        },
        _ => true,
    });
    imported_error && supported_imports
}
pub(super) fn checked_seed_lines(
    root: &Path,
    path: &Path,
    source: &str,
) -> Result<BTreeSet<usize>> {
    if !is_seed_owner_path(root, path) {
        return Ok(BTreeSet::new());
    }
    let file = syn::parse_file(source)?;
    if !canonical_imports(&file)
        || file
            .items
            .iter()
            .any(|item| matches!(item, syn::Item::Type(_)))
    {
        return Ok(BTreeSet::new());
    }
    let owner = file
        .items
        .iter()
        .filter_map(|item| {
            if let syn::Item::Struct(item) = item {
                Some(item)
            } else {
                None
            }
        })
        .find(|item| item.ident == OWNER);
    let Some(owner) = owner else {
        return Ok(BTreeSet::new());
    };
    if matches!(owner.vis, syn::Visibility::Public(_))
        || owner
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("derive"))
    {
        return Ok(BTreeSet::new());
    }
    let syn::Fields::Unnamed(fields) = &owner.fields else {
        return Ok(BTreeSet::new());
    };
    if fields.unnamed.len() != 1 || !matches!(fields.unnamed[0].vis, syn::Visibility::Inherited) {
        return Ok(BTreeSet::new());
    }
    if file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Fn(_) | syn::Item::Mod(_)))
    {
        return Ok(BTreeSet::new());
    }
    let mut methods = Vec::new();
    for item in &file.items {
        if let syn::Item::Impl(item) = item {
            if named(&item.self_ty, OWNER) {
                methods.extend(item.items.iter().filter_map(|item| {
                    if let syn::ImplItem::Fn(item) = item {
                        Some(item)
                    } else {
                        None
                    }
                }));
            }
        }
    }
    if methods.len() != 3 || !methods.iter().any(|item| factory(item)) {
        return Ok(BTreeSet::new());
    }
    let mut sites = SeedSites::default();
    for name in ["crypto_handler", "random_stream"] {
        let Some(item) = methods.iter().find(|item| item.sig.ident == name) else {
            return Ok(BTreeSet::new());
        };
        if !declaration(&item.attrs, "runtime_helper", true)
            || !matches!(item.sig.inputs.iter().next(),Some(syn::FnArg::Receiver(receiver)) if receiver.reference.is_some() && receiver.mutability.is_none())
            || item.sig.inputs.len() != 1
        {
            return Ok(BTreeSet::new());
        }
        sites.visit_block(&item.block);
    }
    if sites.invalid {
        return Ok(BTreeSet::new());
    }
    Ok(sites.lines)
}
#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = include_str!("../../../../crates/aura-agent/src/runtime/entropy.rs");
    const ROOT: &str = "/actual-profile-check/aura";
    const PATH: &str = "/actual-profile-check/aura/crates/aura-agent/src/runtime/entropy.rs";
    #[test]
    fn absolute_entropy_owner_contract_rejects_foreign_suffix_and_relative_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        assert!(root.is_absolute());
        let actual = root.join("crates/aura-agent/src/runtime/entropy.rs");
        assert!(is_seed_owner_path(root, &actual));
        assert!(!checked_seed_lines(root, &actual, SOURCE)
            .unwrap()
            .is_empty());
        for foreign in [
            Path::new("/foreign/aura/crates/aura-agent/src/runtime/entropy.rs"),
            Path::new("crates/aura-agent/src/runtime/entropy.rs"),
        ] {
            assert!(!is_seed_owner_path(root, foreign));
            assert!(checked_seed_lines(root, foreign, SOURCE)
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn checked_entropy_origin_requires_actual_factory_and_typed_receivers() {
        assert!(
            !checked_seed_lines(Path::new(ROOT), Path::new(PATH), SOURCE)
                .unwrap()
                .is_empty()
        );
        for valid in [
            SOURCE.replace(
                "Result<Option<",
                "std::result::Result<core::option::Option<",
            ),
            SOURCE.replace(", AuraError>", ", aura_core::AuraError>"),
        ] {
            assert!(
                !checked_seed_lines(Path::new(ROOT), Path::new(PATH), &valid)
                    .unwrap()
                    .is_empty()
            );
        }
        for bad in [
            SOURCE.replace("Result<Option<", "foreign::Result<Option<"),
            SOURCE.replace("Result<Option<", "Result<foreign::Option<"),
            SOURCE.replace(", AuraError>", ", foreign::AuraError>"),
            SOURCE.replace("use aura_core::AuraError;", "use foreign::AuraError;"),
            SOURCE.to_owned() + "\nuse foreign::Err;\n",
            SOURCE.replace("mode.is_production() && seed.is_some()", "seed.is_some()"),
            SOURCE.replace(
                "mode: aura_core::effects::ExecutionMode",
                "mode: foreign::ExecutionMode",
            ),
            SOURCE.replace("family = \"proof_issuer\"", "family = \"runtime_helper\""),
            SOURCE.replace("kind = \"proof_issuer\"", "kind = \"observed\""),
            SOURCE.replace(
                "#[aura_macros::authoritative_source",
                "#[foreign::authoritative_source",
            ),
            SOURCE.replace(
                "return Err(AuraError::Invalid",
                "return Ok(AuraError::Invalid",
            ),
            SOURCE.replace(
                "receiver_type = NonProductionEntropySeed",
                "receiver_type = String",
            ),
            SOURCE.replace(
                "struct NonProductionEntropySeed([u8; 32])",
                "struct NonProductionEntropySeed(pub [u8; 32])",
            ),
            SOURCE.replace(
                "&self) -> super::subsystems::crypto::CryptoRng",
                "&self, seed: [u8;32]) -> super::subsystems::crypto::CryptoRng",
            ),
            SOURCE.replace("from_seed(self.0)", "from_seed([0;32])"),
        ] {
            assert!(checked_seed_lines(Path::new(ROOT), Path::new(PATH), &bad)
                .unwrap()
                .is_empty());
        }
        for bad in [
            SOURCE.replace(
                "Result<Option<NonProductionEntropySeed>, AuraError>",
                "Result<Option<String>, NonProductionEntropySeed>",
            ),
            SOURCE.replace("fn admit(", "fn decorative_admit("),
            SOURCE.to_owned()
                + "\nfn forge() -> NonProductionEntropySeed { NonProductionEntropySeed([0;32]) }\n",
        ] {
            assert!(checked_seed_lines(Path::new(ROOT), Path::new(PATH), &bad)
                .unwrap()
                .is_empty());
        }
        assert!(checked_seed_lines(
            Path::new(ROOT),
            Path::new("/foreign/crates/aura-agent/src/runtime/entropy.rs"),
            SOURCE
        )
        .unwrap()
        .is_empty());
    }
}
