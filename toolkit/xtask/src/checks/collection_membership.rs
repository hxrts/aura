//! Narrow source-proven collection membership; unknown origins remain checked.
use anyhow::Result;
use std::collections::BTreeSet;
use syn::{parse::Parse, visit::Visit};

fn imported_device_id(tree: &syn::UseTree, prefix: &str) -> bool {
    match tree {
        syn::UseTree::Path(path) => {
            imported_device_id(&path.tree, &format!("{prefix}{}::", path.ident))
        }
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .any(|tree| imported_device_id(tree, prefix)),
        syn::UseTree::Name(name) => prefix == "aura_core::" && name.ident == "DeviceId",
        _ => false,
    }
}
fn device_collection(ty: &syn::Type, imported: bool) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some()
        || !((path.path.segments.len() == 1 && path.path.segments[0].ident == "Vec")
            || path
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                == ["std", "vec", "Vec"])
    {
        return false;
    }
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    if args.args.len() != 1 {
        return false;
    }
    let Some(syn::GenericArgument::Type(syn::Type::Path(element))) = args.args.first() else {
        return false;
    };
    element.qself.is_none()
        && ((imported && element.path.is_ident("DeviceId"))
            || element
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                == ["aura_core", "DeviceId"])
}
#[derive(Default)]
struct Shadow {
    names: BTreeSet<String>,
}
impl<'ast> Visit<'ast> for Shadow {
    fn visit_stmt_macro(&mut self, _: &'ast syn::StmtMacro) {
        self.names.insert("*".into());
    }
    fn visit_item_macro(&mut self, _: &'ast syn::ItemMacro) {
        self.names.insert("*".into());
    }
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.names.insert(pattern.ident.to_string());
    }
    fn visit_local(&mut self, local: &'ast syn::Local) {
        struct Names<'a>(&'a mut BTreeSet<String>);
        impl<'ast> Visit<'ast> for Names<'_> {
            fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
                self.0.insert(pat.ident.to_string());
            }
        }
        Names(&mut self.names).visit_pat(&local.pat);
        syn::visit::visit_local(self, local);
    }
    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        for input in &closure.inputs {
            if let syn::Pat::Ident(name) = input {
                self.names.insert(name.ident.to_string());
            }
        }
        syn::visit::visit_expr_closure(self, closure);
    }
}
struct Matches {
    _value: syn::Expr,
    _comma: syn::Token![,],
    pattern: syn::Pat,
    guard: Option<syn::Expr>,
}
impl Parse for Matches {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let value = input.parse()?;
        let comma = input.parse()?;
        let pattern = syn::Pat::parse_multi_with_leading_vert(input)?;
        let guard = if input.peek(syn::Token![if]) {
            input.parse::<syn::Token![if]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("unsupported matches tokens"));
        }
        Ok(Self {
            _value: value,
            _comma: comma,
            pattern,
            guard,
        })
    }
}
struct Membership {
    fields: BTreeSet<(String, String, String)>,
    bound: BTreeSet<String>,
    spans: Vec<proc_macro2::Span>,
    matches_allowed: bool,
}
impl Membership {
    fn bindings(&self, pattern: &syn::Pat) -> BTreeSet<String> {
        let syn::Pat::Struct(pattern) = pattern else {
            return BTreeSet::new();
        };
        if pattern.path.segments.len() != 2 {
            return BTreeSet::new();
        }
        let mut segments = pattern.path.segments.iter();
        let owner = segments.next().unwrap().ident.to_string();
        let variant = segments.next().unwrap().ident.to_string();
        pattern
            .fields
            .iter()
            .filter_map(|field| {
                let syn::Member::Named(name) = &field.member else {
                    return None;
                };
                if !self
                    .fields
                    .contains(&(owner.clone(), variant.clone(), name.to_string()))
                {
                    return None;
                }
                let syn::Pat::Ident(binding) = field.pat.as_ref() else {
                    return None;
                };
                Some(binding.ident.to_string())
            })
            .collect()
    }
    fn scoped(&mut self, pattern: &syn::Pat, guard: Option<&syn::Expr>, body: Option<&syn::Expr>) {
        let mut names = self.bindings(pattern);
        let mut shadow = Shadow::default();
        if let Some(body) = body {
            shadow.visit_expr(body);
        }
        if let Some(guard) = guard {
            shadow.visit_expr(guard);
        }
        names.retain(|name| !shadow.names.contains("*") && !shadow.names.contains(name));
        let previous = self.bound.clone();
        if shadow.names.contains("*") {
            self.bound.clear();
        }
        let mut declared = Shadow::default();
        declared.visit_pat(pattern);
        self.bound.retain(|name| !declared.names.contains(name));
        self.bound.extend(names);
        if let Some(guard) = guard {
            self.visit_expr(guard);
        }
        if let Some(body) = body {
            self.visit_expr(body);
        }
        self.bound = previous;
    }
}
#[derive(Default)]
struct LocalTypeNames(BTreeSet<String>);
impl<'ast> Visit<'ast> for LocalTypeNames {
    fn visit_stmt_macro(&mut self, _: &'ast syn::StmtMacro) {
        self.0.insert("*".into());
    }
    fn visit_item_macro(&mut self, _: &'ast syn::ItemMacro) {
        self.0.insert("*".into());
    }
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.0.insert(item.ident.to_string());
    }
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.0.insert(item.ident.to_string());
    }
    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        self.0.insert(item.ident.to_string());
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        fn collect(tree: &syn::UseTree, names: &mut BTreeSet<String>) {
            match tree {
                syn::UseTree::Path(path) => collect(&path.tree, names),
                syn::UseTree::Group(group) => {
                    for tree in &group.items {
                        collect(tree, names);
                    }
                }
                syn::UseTree::Name(name) => {
                    names.insert(name.ident.to_string());
                }
                syn::UseTree::Rename(rename) => {
                    names.insert(rename.rename.to_string());
                }
                syn::UseTree::Glob(_) => {
                    names.insert("*".into());
                }
            }
        }
        collect(&item.tree, &mut self.0);
    }
}
impl Membership {
    fn visit_unshadowed_block(&mut self, block: &syn::Block) {
        let mut names = LocalTypeNames::default();
        names.visit_block(block);
        let saved = self.fields.clone();
        if names.0.contains("*") {
            self.fields.clear();
        } else {
            self.fields.retain(|(owner, _, _)| !names.0.contains(owner));
        }
        self.visit_block(block);
        self.fields = saved;
    }
}
impl<'ast> Visit<'ast> for Membership {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.visit_unshadowed_block(&item.block);
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.visit_unshadowed_block(&item.block);
    }
    fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {} // nested name resolution is unresolved
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.scoped(
            &arm.pat,
            arm.guard.as_ref().map(|(_, expr)| expr.as_ref()),
            Some(&arm.body),
        );
    }
    fn visit_expr_macro(&mut self, expr: &'ast syn::ExprMacro) {
        if self.matches_allowed && expr.mac.path.is_ident("matches") {
            if let Ok(parsed) = syn::parse2::<Matches>(expr.mac.tokens.clone()) {
                self.scoped(&parsed.pattern, parsed.guard.as_ref(), None);
            }
        }
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "contains" && call.args.len() == 1 {
            if let syn::Expr::Path(receiver) = call.receiver.as_ref() {
                if receiver.qself.is_none()
                    && receiver.path.segments.len() == 1
                    && self
                        .bound
                        .contains(&receiver.path.segments[0].ident.to_string())
                {
                    self.spans.push(call.method.span());
                }
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}
/// Mask only the contains method identifier for source-proven Vec<DeviceId>
/// enum-field membership. Receiver and argument expressions remain enforced.
/// Other calls on the same line remain visible to the security scanner.
pub(super) fn mask_typed_membership(source: &str) -> Result<Vec<String>> {
    let file = syn::parse_file(source)?;
    let imported = file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Use(item) if item.attrs.is_empty() && imported_device_id(&item.tree, "")));
    // A same-file type declaration shadows canonical/prelude spelling.
    let shadowed = file.items.iter().any(|item| match item {
        syn::Item::Use(item) => {
            fn noncanonical_device(tree: &syn::UseTree, prefix: &str) -> bool {
                match tree {
                    syn::UseTree::Path(path) => {
                        noncanonical_device(&path.tree, &format!("{prefix}{}::", path.ident))
                    }
                    syn::UseTree::Group(group) => group
                        .items
                        .iter()
                        .any(|tree| noncanonical_device(tree, prefix)),
                    syn::UseTree::Name(name) => name.ident == "DeviceId" && prefix != "aura_core::",
                    syn::UseTree::Rename(rename) => rename.rename == "DeviceId",
                    _ => false,
                }
            }

            fn ambiguous(tree: &syn::UseTree) -> bool {
                match tree {
                    syn::UseTree::Glob(_) => true,
                    syn::UseTree::Rename(rename) => {
                        rename.rename == "DeviceId"
                            || rename.rename == "Vec"
                            || rename.rename == "std"
                            || rename.rename == "aura_core"
                    }
                    syn::UseTree::Path(path) => ambiguous(&path.tree),
                    syn::UseTree::Group(group) => group.items.iter().any(ambiguous),
                    syn::UseTree::Name(name) => {
                        name.ident == "Vec" || name.ident == "std" || name.ident == "aura_core"
                    }
                }
            }
            !item.attrs.is_empty() || noncanonical_device(&item.tree, "") || ambiguous(&item.tree)
        }
        syn::Item::Mod(item) => ["std", "aura_core", "Vec", "DeviceId"]
            .iter()
            .any(|name| item.ident == *name),
        syn::Item::ExternCrate(item) => {
            let name = item
                .rename
                .as_ref()
                .map(|(_, name)| name)
                .unwrap_or(&item.ident);
            name == "std" || name == "aura_core"
        }
        syn::Item::Type(item) => item.ident == "DeviceId" || item.ident == "Vec",
        syn::Item::Struct(item) => item.ident == "DeviceId" || item.ident == "Vec",
        syn::Item::Enum(item) => item.ident == "DeviceId" || item.ident == "Vec",
        _ => false,
    });
    #[derive(Default)]
    struct MacroShadow(bool);
    impl<'ast> Visit<'ast> for MacroShadow {
        fn visit_item_macro(&mut self, _: &'ast syn::ItemMacro) {
            // Item macros can introduce an otherwise invisible matches binding.
            self.0 = true;
        }
        fn visit_stmt_macro(&mut self, _: &'ast syn::StmtMacro) {
            // Statement expansion can likewise introduce a local macro binding.
            self.0 = true;
        }
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            fn shadows(tree: &syn::UseTree) -> bool {
                match tree {
                    syn::UseTree::Glob(_) => true,
                    syn::UseTree::Name(name) => name.ident == "matches",
                    syn::UseTree::Rename(rename) => rename.rename == "matches",
                    syn::UseTree::Path(path) => shadows(&path.tree),
                    syn::UseTree::Group(group) => group.items.iter().any(shadows),
                }
            }
            self.0 |= shadows(&item.tree);
        }
    }
    let mut macro_shadow = MacroShadow::default();
    macro_shadow.visit_file(&file);
    let mut owner = Membership {
        fields: BTreeSet::new(),
        bound: BTreeSet::new(),
        spans: Vec::new(),
        matches_allowed: !macro_shadow.0,
    };

    if !shadowed {
        for item in &file.items {
            if let syn::Item::Enum(item) = item {
                if item.attrs.iter().any(|attribute| {
                    !attribute.path().is_ident("doc") && !attribute.path().is_ident("derive")
                }) {
                    continue;
                }
                if item.generics.params.iter().any(|parameter| matches!(parameter, syn::GenericParam::Type(parameter) if parameter.ident == "DeviceId" || parameter.ident == "Vec")) { continue; }
                for variant in &item.variants {
                    if variant
                        .attrs
                        .iter()
                        .any(|attribute| !attribute.path().is_ident("doc"))
                    {
                        continue;
                    }
                    for field in &variant.fields {
                        if field
                            .attrs
                            .iter()
                            .any(|attribute| !attribute.path().is_ident("doc"))
                        {
                            continue;
                        }
                        let duplicate = variant
                            .fields
                            .iter()
                            .filter(|other| other.ident == field.ident)
                            .count()
                            != 1;
                        if !duplicate && device_collection(&field.ty, imported) {
                            if let Some(name) = &field.ident {
                                owner.fields.insert((
                                    item.ident.to_string(),
                                    variant.ident.to_string(),
                                    name.to_string(),
                                ));
                            }
                        }
                    }
                }
            }
        }
        owner.visit_file(&file);
    }
    let mut lines: Vec<Vec<u8>> = source
        .lines()
        .map(|line| line.as_bytes().to_vec())
        .collect();
    for span in owner.spans {
        let start = span.start();
        let end = span.end();
        for number in start.line..=end.line {
            if let Some(line) = lines.get_mut(number - 1) {
                let from = if number == start.line {
                    start.column
                } else {
                    0
                };
                let to = if number == end.line {
                    end.column.min(line.len())
                } else {
                    line.len()
                };
                if let Some(bytes) = line.get_mut(from..to) {
                    bytes.fill(b' ');
                }
            }
        }
    }
    Ok(lines
        .into_iter()
        .map(|line| {
            String::from_utf8(line).expect("mask preserves UTF-8 outside complete AST expressions")
        })
        .collect())
}
