//! Aura-specific annotation definitions for choreographies

use syn::{Expr, Lit};
use std::collections::HashMap;

/// Aura-specific annotations that can be applied to choreography elements
#[derive(Debug, Clone, PartialEq)]
pub enum AuraAnnotation {
    /// Guard capability requirement
    GuardCapability(GuardCapability),
    /// Flow cost for communication
    FlowCost(FlowCost),
    /// Journal facts operation
    JournalFacts(JournalFacts),
    /// Journal merge operation
    JournalMerge(JournalMerge),
    /// Leakage budget specification
    LeakageBudget(LeakageBudget),
}

/// Guard capability annotation: `@guard_capability = "capability_name"`
#[derive(Debug, Clone, PartialEq)]
pub struct GuardCapability {
    pub capability: String,
}

/// Flow cost annotation: `@flow_cost = <number>`
#[derive(Debug, Clone, PartialEq)]
pub struct FlowCost {
    pub cost: u32,
}

/// Journal facts annotation: `@journal_facts = "description"`
#[derive(Debug, Clone, PartialEq)]
pub struct JournalFacts {
    pub description: String,
}

/// Journal merge annotation: `@journal_merge = true`
#[derive(Debug, Clone, PartialEq)]
pub struct JournalMerge {
    pub enabled: bool,
}

/// Leakage budget annotation: `@leakage_budget = [external, neighbor, ingroup]`
#[derive(Debug, Clone, PartialEq)]
pub struct LeakageBudget {
    pub external: u32,
    pub neighbor: u32,
    pub ingroup: u32,
}

/// Parse annotations from a list of key-value pairs
pub fn parse_annotations(annotations: HashMap<String, Expr>) -> syn::Result<Vec<AuraAnnotation>> {
    let mut result = Vec::new();
    
    for (key, value) in annotations {
        let annotation = match key.as_str() {
            "guard_capability" => {
                let capability = parse_string_value(&value)?;
                AuraAnnotation::GuardCapability(GuardCapability { capability })
            },
            "flow_cost" => {
                let cost = parse_u32_value(&value)?;
                AuraAnnotation::FlowCost(FlowCost { cost })
            },
            "journal_facts" => {
                let description = parse_string_value(&value)?;
                AuraAnnotation::JournalFacts(JournalFacts { description })
            },
            "journal_merge" => {
                let enabled = parse_bool_value(&value)?;
                AuraAnnotation::JournalMerge(JournalMerge { enabled })
            },
            "leakage_budget" => {
                let (external, neighbor, ingroup) = parse_leakage_budget_value(&value)?;
                AuraAnnotation::LeakageBudget(LeakageBudget {
                    external,
                    neighbor, 
                    ingroup,
                })
            },
            _ => {
                return Err(syn::Error::new_spanned(
                    value,
                    format!("Unknown Aura annotation: {}", key),
                ));
            }
        };
        
        result.push(annotation);
    }
    
    Ok(result)
}

fn parse_string_value(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Lit(expr_lit) => match &expr_lit.lit {
            Lit::Str(lit_str) => Ok(lit_str.value()),
            _ => Err(syn::Error::new_spanned(expr, "Expected string literal")),
        },
        _ => Err(syn::Error::new_spanned(expr, "Expected string literal")),
    }
}

fn parse_u32_value(expr: &Expr) -> syn::Result<u32> {
    match expr {
        Expr::Lit(expr_lit) => match &expr_lit.lit {
            Lit::Int(lit_int) => {
                lit_int.base10_parse::<u32>()
                    .map_err(|_| syn::Error::new_spanned(expr, "Invalid u32 value"))
            },
            _ => Err(syn::Error::new_spanned(expr, "Expected integer literal")),
        },
        _ => Err(syn::Error::new_spanned(expr, "Expected integer literal")),
    }
}

fn parse_bool_value(expr: &Expr) -> syn::Result<bool> {
    match expr {
        Expr::Lit(expr_lit) => match &expr_lit.lit {
            Lit::Bool(lit_bool) => Ok(lit_bool.value),
            _ => Err(syn::Error::new_spanned(expr, "Expected boolean literal")),
        },
        _ => Err(syn::Error::new_spanned(expr, "Expected boolean literal")),
    }
}

fn parse_leakage_budget_value(expr: &Expr) -> syn::Result<(u32, u32, u32)> {
    match expr {
        Expr::Array(expr_array) => {
            if expr_array.elems.len() != 3 {
                return Err(syn::Error::new_spanned(
                    expr,
                    "Leakage budget must have exactly 3 elements: [external, neighbor, ingroup]",
                ));
            }
            
            let external = parse_u32_value(&expr_array.elems[0])?;
            let neighbor = parse_u32_value(&expr_array.elems[1])?;
            let ingroup = parse_u32_value(&expr_array.elems[2])?;
            
            Ok((external, neighbor, ingroup))
        },
        _ => Err(syn::Error::new_spanned(
            expr,
            "Expected array literal: [external, neighbor, ingroup]",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;
    use std::collections::HashMap;

    #[test]
    fn test_guard_capability_annotation() {
        let mut annotations = HashMap::new();
        annotations.insert("guard_capability".to_string(), parse_quote!("test_capability"));
        
        let result = parse_annotations(annotations).unwrap();
        assert_eq!(result.len(), 1);
        
        match &result[0] {
            AuraAnnotation::GuardCapability(cap) => {
                assert_eq!(cap.capability, "test_capability");
            },
            _ => panic!("Expected GuardCapability annotation"),
        }
    }

    #[test]
    fn test_flow_cost_annotation() {
        let mut annotations = HashMap::new();
        annotations.insert("flow_cost".to_string(), parse_quote!(150));
        
        let result = parse_annotations(annotations).unwrap();
        assert_eq!(result.len(), 1);
        
        match &result[0] {
            AuraAnnotation::FlowCost(cost) => {
                assert_eq!(cost.cost, 150);
            },
            _ => panic!("Expected FlowCost annotation"),
        }
    }

    #[test]
    fn test_leakage_budget_annotation() {
        let mut annotations = HashMap::new();
        annotations.insert("leakage_budget".to_string(), parse_quote!([2, 1, 0]));
        
        let result = parse_annotations(annotations).unwrap();
        assert_eq!(result.len(), 1);
        
        match &result[0] {
            AuraAnnotation::LeakageBudget(budget) => {
                assert_eq!(budget.external, 2);
                assert_eq!(budget.neighbor, 1);
                assert_eq!(budget.ingroup, 0);
            },
            _ => panic!("Expected LeakageBudget annotation"),
        }
    }
}