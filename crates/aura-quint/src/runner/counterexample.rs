use crate::evaluator::QuintEvaluator;
use crate::{AuraResult, PropertySpec};
use aura_core::AuraError;
use serde_json::Value;

/// Counterexample generation engine using bounded depth-first search.
#[derive(Debug)]
pub(crate) struct CounterexampleGenerator {
    /// Maximum search depth for bounded DFS exploration.
    max_depth: usize,
    /// Random seed for deterministic shrinking during counterexample minimization.
    random_seed: Option<u64>,
}

impl CounterexampleGenerator {
    pub(crate) fn new(max_depth: usize, random_seed: Option<u64>) -> Self {
        Self {
            max_depth,
            random_seed,
        }
    }

    #[cfg(test)]
    pub(crate) fn max_depth(&self) -> usize {
        self.max_depth
    }

    #[cfg(test)]
    pub(crate) fn random_seed(&self) -> Option<u64> {
        self.random_seed
    }

    /// Generate a counterexample using bounded depth-first search.
    pub(crate) async fn generate_counterexample(
        &self,
        property_spec: &PropertySpec,
        evaluator: &QuintEvaluator,
    ) -> AuraResult<Option<Value>> {
        tracing::debug!(
            "Generating counterexample for property: {} (max_depth: {}, seed: {:?})",
            property_spec.spec_file,
            self.max_depth,
            self.random_seed
        );

        let json_ir = evaluator.parse_file(&property_spec.spec_file).await?;
        let enhanced_ir = self.prepare_bounded_search_config(&json_ir)?;
        let result = evaluator.simulate_via_evaluator(&enhanced_ir).await?;

        let parsed_result: Value = serde_json::from_str(&result).map_err(|err| {
            AuraError::invalid(format!("Failed to parse simulation result: {err}"))
        })?;

        if let Some(counterexample) = parsed_result.get("counterexample") {
            let shrunk = self
                .shrink_counterexample(counterexample, property_spec, evaluator)
                .await?;
            tracing::debug!(
                "Counterexample found and shrunk: {} steps -> {} steps",
                counterexample
                    .as_array()
                    .map(|steps| steps.len())
                    .unwrap_or(0),
                shrunk.as_array().map(|steps| steps.len()).unwrap_or(0)
            );
            Ok(Some(shrunk))
        } else {
            tracing::debug!(
                "No counterexample found within depth bound {}",
                self.max_depth
            );
            Ok(None)
        }
    }

    fn prepare_bounded_search_config(&self, json_ir: &str) -> AuraResult<String> {
        let mut ir_value: Value = serde_json::from_str(json_ir)
            .map_err(|err| AuraError::invalid(format!("Failed to parse JSON IR: {err}")))?;

        let search_config = serde_json::json!({
            "maxDepth": self.max_depth,
            "randomSeed": self.random_seed,
            "searchStrategy": "bounded_dfs",
            "enableShrinking": true,
            "maxShrinkIterations": 100
        });

        if let Some(ir_obj) = ir_value.as_object_mut() {
            if let Some(existing_config) = ir_obj.get_mut("simulationConfig") {
                if let Some(config_obj) = existing_config.as_object_mut() {
                    config_obj.insert("searchConfig".to_string(), search_config);
                }
            } else {
                ir_obj.insert(
                    "simulationConfig".to_string(),
                    serde_json::json!({ "searchConfig": search_config }),
                );
            }
        }

        serde_json::to_string(&ir_value).map_err(|err| {
            AuraError::invalid(format!("Failed to serialize bounded search config: {err}"))
        })
    }

    async fn shrink_counterexample(
        &self,
        counterexample: &Value,
        property_spec: &PropertySpec,
        evaluator: &QuintEvaluator,
    ) -> AuraResult<Value> {
        if let Some(trace) = counterexample.as_array() {
            if trace.len() <= 1 || trace.len() <= self.max_depth / 10 {
                return Ok(counterexample.clone());
            }

            let mut rng_state = self.random_seed.unwrap_or(0);
            let mut shrunk_trace = trace.clone();
            let mut step_size = shrunk_trace.len() / 2;
            let mut shrink_attempts = 0usize;

            while step_size >= 1 && shrunk_trace.len() > 1 && shrink_attempts < 100 {
                shrink_attempts = shrink_attempts.saturating_add(1);
                rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                let start_idx = (rng_state as usize) % shrunk_trace.len().max(1);
                let end_idx = (start_idx + step_size).min(shrunk_trace.len());
                let candidate = shrunk_trace
                    .iter()
                    .enumerate()
                    .filter(|(idx, _)| *idx < start_idx || *idx >= end_idx)
                    .map(|(_, step)| step.clone())
                    .collect::<Vec<_>>();

                if !candidate.is_empty() {
                    let candidate_value = Value::Array(candidate.clone());
                    match self
                        .verify_trace_violates_property(&candidate_value, property_spec, evaluator)
                        .await
                    {
                        Ok(true) => shrunk_trace = candidate,
                        Ok(false) => {}
                        Err(err) => {
                            tracing::warn!(
                                "Shrink attempt {}: verification failed, skipping candidate: {}",
                                shrink_attempts,
                                err
                            );
                        }
                    }
                }

                step_size /= 2;
            }

            tracing::debug!(
                "Shrunk counterexample from {} to {} steps",
                trace.len(),
                shrunk_trace.len()
            );
            Ok(Value::Array(shrunk_trace))
        } else {
            Ok(counterexample.clone())
        }
    }

    async fn verify_trace_violates_property(
        &self,
        trace: &Value,
        property_spec: &PropertySpec,
        evaluator: &QuintEvaluator,
    ) -> AuraResult<bool> {
        let json_ir = evaluator.parse_file(&property_spec.spec_file).await?;
        let mut ir_value: Value = serde_json::from_str(&json_ir)
            .map_err(|err| AuraError::invalid(format!("Failed to parse JSON IR: {err}")))?;

        if let Some(ir_obj) = ir_value.as_object_mut() {
            ir_obj.insert(
                "replayConfig".to_string(),
                serde_json::json!({
                    "mode": "replay",
                    "trace": trace,
                    "checkProperty": property_spec.name,
                }),
            );
        }

        let enhanced_ir = serde_json::to_string(&ir_value).map_err(|err| {
            AuraError::invalid(format!("Failed to serialize replay config: {err}"))
        })?;
        let result = evaluator.simulate_via_evaluator(&enhanced_ir).await?;
        let parsed_result: Value = serde_json::from_str(&result)
            .map_err(|err| AuraError::invalid(format!("Failed to parse replay result: {err}")))?;

        Ok(parsed_result
            .get("propertyViolated")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| parsed_result.get("counterexample").is_some()))
    }
}
