# Advanced Choreography Guide

This guide covers advanced choreographic programming patterns for building complex distributed protocols. It focuses on protocol composition, error handling, resilience patterns, and system layering techniques that build on foundational concepts.

For DSL syntax, annotations, and basic choreographic programming, see [Multi-party Session Types and Choreography](108_mpst_and_choreography.md). For foundational concepts, see [Coordination Systems Guide](803_coordination_guide.md).

## Protocol Composition and Layering

Complex applications require composing multiple protocols and layering different coordination mechanisms. Composition patterns enable building sophisticated distributed systems.

### Sequential Protocol Composition

Chain protocols to create multi-phase workflows:

```rust
pub struct SequentialProtocolRunner<E: Effects> {
    effects: E,
    device_id: aura_core::DeviceId,
}

impl<E: Effects> SequentialProtocolRunner<E> {
    pub async fn execute_authentication_flow(
        &self,
        target_device: aura_core::DeviceId,
    ) -> Result<AuthenticationResult, ProtocolError> {
        // Phase 1: Identity exchange
        let identity_result = self.execute_identity_exchange(target_device).await?;
        
        // Phase 2: Capability negotiation
        let capability_result = self.execute_capability_negotiation(
            target_device,
            &identity_result
        ).await?;
        
        // Phase 3: Session establishment
        let session_result = self.execute_session_establishment(
            target_device,
            &capability_result
        ).await?;
        
        Ok(AuthenticationResult {
            identity: identity_result,
            capabilities: capability_result,
            session: session_result,
        })
    }
}
```

Sequential composition executes protocols in dependency order. Each phase uses results from previous phases. Failed phases abort the entire workflow. State accumulates across phase boundaries.

### Parallel Protocol Execution

Execute multiple protocols concurrently with synchronization points:

```rust
pub struct ParallelProtocolCoordinator<E: Effects> {
    effects: E,
    coordination_config: CoordinationConfig,
}

impl<E: Effects> ParallelProtocolCoordinator<E> {
    pub async fn execute_distributed_computation(
        &self,
        computation_request: ComputationRequest,
        worker_devices: Vec<aura_core::DeviceId>,
    ) -> Result<ComputationResult, ProtocolError> {
        // Launch parallel worker protocols
        let worker_futures = worker_devices.into_iter().map(|device| {
            self.execute_worker_protocol(device, computation_request.clone())
        });
        
        // Wait for all workers with timeout
        let worker_results = tokio::time::timeout(
            self.coordination_config.worker_timeout,
            futures::future::try_join_all(worker_futures)
        ).await??;
        
        // Aggregate results from all workers
        let aggregated_result = self.aggregate_worker_results(worker_results).await?;
        
        // Broadcast final result to all workers
        let broadcast_futures = worker_devices.into_iter().map(|device| {
            self.broadcast_final_result(device, &aggregated_result)
        });
        
        futures::future::try_join_all(broadcast_futures).await?;
        
        Ok(aggregated_result)
    }
}
```

Parallel execution maximizes resource utilization. Synchronization points coordinate distributed phases. Timeout handling manages unresponsive participants. Result aggregation combines parallel outputs.

### Protocol Adaptation Layers

Adapt between different protocol interfaces:

```rust
pub struct ProtocolAdapter<E: Effects> {
    effects: E,
    adaptation_config: AdaptationConfig,
}

impl<E: Effects> ProtocolAdapter<E> {
    pub async fn adapt_legacy_to_modern(
        &self,
        legacy_request: LegacyProtocolRequest,
    ) -> Result<ModernProtocolResult, AdaptationError> {
        // Convert legacy request format
        let modern_request = self.convert_request_format(legacy_request)?;
        
        // Execute modern protocol
        let modern_result = self.execute_modern_protocol(modern_request).await?;
        
        // Convert result back to legacy format if needed
        let adapted_result = self.convert_result_format(modern_result)?;
        
        Ok(adapted_result)
    }
    
    async fn execute_capability_bridging(
        &self,
        source_capabilities: CapabilitySet,
        target_protocol: ProtocolType,
    ) -> Result<BridgedCapabilities, AdaptationError> {
        // Map capabilities between different systems
        let mapped_capabilities = self.map_capability_semantics(
            source_capabilities,
            target_protocol
        )?;
        
        // Verify capability compatibility
        self.verify_capability_compatibility(&mapped_capabilities, target_protocol).await?;
        
        Ok(BridgedCapabilities {
            original: source_capabilities,
            mapped: mapped_capabilities,
            protocol: target_protocol,
        })
    }
}
```

Adaptation layers bridge different protocol generations. Format conversion handles message compatibility. Capability mapping preserves security properties across protocol boundaries.

## Error Handling and Resilience

Production choreographic protocols require comprehensive error handling and resilience mechanisms. These patterns ensure graceful degradation under adverse conditions.

### Timeout and Retry Patterns

Implement robust timeout handling with exponential backoff:

```rust
pub struct ResilientProtocolExecutor<E: Effects> {
    effects: E,
    retry_config: RetryConfig,
}

impl<E: Effects> ResilientProtocolExecutor<E> {
    pub async fn execute_with_resilience<T>(
        &self,
        protocol_fn: impl Fn() -> Pin<Box<dyn Future<Output = Result<T, ProtocolError>> + Send>>,
        operation_name: &str,
    ) -> Result<T, ProtocolError> {
        let mut attempt = 0;
        let mut last_error = None;
        
        while attempt < self.retry_config.max_attempts {
            match tokio::time::timeout(
                self.retry_config.operation_timeout,
                protocol_fn()
            ).await {
                Ok(Ok(result)) => {
                    if attempt > 0 {
                        self.effects.log_info(&format!(
                            "Protocol {} succeeded on attempt {}",
                            operation_name, attempt + 1
                        )).await;
                    }
                    return Ok(result);
                }
                Ok(Err(e)) => {
                    last_error = Some(e.clone());
                    
                    if !e.is_retryable() || attempt >= self.retry_config.max_attempts - 1 {
                        break;
                    }
                    
                    self.effects.log_warn(&format!(
                        "Protocol {} attempt {} failed: {}, retrying...",
                        operation_name, attempt + 1, e
                    )).await;
                }
                Err(_) => {
                    let timeout_error = ProtocolError::Timeout {
                        operation: operation_name.to_string(),
                        timeout: self.retry_config.operation_timeout,
                    };
                    last_error = Some(timeout_error);
                    
                    if attempt >= self.retry_config.max_attempts - 1 {
                        break;
                    }
                }
            }
            
            // Exponential backoff with jitter
            let delay = self.retry_config.base_delay * 2_u32.pow(attempt);
            let jittered_delay = self.add_jitter(delay);
            tokio::time::sleep(jittered_delay).await;
            
            attempt += 1;
        }
        
        Err(last_error.unwrap_or_else(|| ProtocolError::UnknownFailure))
    }
    
    fn add_jitter(&self, delay: Duration) -> Duration {
        let jitter_range = delay / 4; // 25% jitter
        let jitter_ms = fastrand::u64(0..jitter_range.as_millis() as u64);
        delay + Duration::from_millis(jitter_ms)
    }
}
```

Resilient execution handles transient failures automatically. Exponential backoff prevents overwhelming failed services. Jitter reduces thundering herd effects. Retry limits prevent infinite loops.

### Compensation and Rollback

Implement compensation logic for failed multi-phase protocols:

```rust
pub struct CompensatingProtocolManager<E: Effects> {
    effects: E,
    compensation_log: CompensationLog,
}

impl<E: Effects> CompensatingProtocolManager<E> {
    pub async fn execute_compensating_transaction(
        &self,
        transaction: CompensatingTransaction,
    ) -> Result<TransactionResult, TransactionError> {
        let mut completed_operations = Vec::new();
        
        // Execute forward operations
        for operation in &transaction.operations {
            match self.execute_operation(operation).await {
                Ok(result) => {
                    completed_operations.push((operation.clone(), result));
                    self.compensation_log.record_operation(operation).await?;
                }
                Err(e) => {
                    // Compensation required
                    self.effects.log_error(&format!(
                        "Operation {} failed: {}, starting compensation",
                        operation.operation_id, e
                    )).await;
                    
                    self.execute_compensation(&completed_operations).await?;
                    
                    return Err(TransactionError::OperationFailed {
                        failed_operation: operation.clone(),
                        cause: e,
                        compensated: true,
                    });
                }
            }
        }
        
        // All operations succeeded
        self.compensation_log.mark_transaction_complete(&transaction.transaction_id).await?;
        
        Ok(TransactionResult {
            transaction_id: transaction.transaction_id,
            completed_operations,
            status: TransactionStatus::Committed,
        })
    }
    
    async fn execute_compensation(
        &self,
        completed_operations: &[(Operation, OperationResult)],
    ) -> Result<(), CompensationError> {
        // Execute compensations in reverse order
        for (operation, _result) in completed_operations.iter().rev() {
            match self.execute_compensation_for_operation(operation).await {
                Ok(_) => {
                    self.compensation_log.record_compensation(operation).await?;
                }
                Err(e) => {
                    self.effects.log_error(&format!(
                        "Compensation failed for operation {}: {}",
                        operation.operation_id, e
                    )).await;
                    
                    // Log compensation failure but continue with other compensations
                    self.compensation_log.record_compensation_failure(operation, e).await?;
                }
            }
        }
        
        Ok(())
    }
}
```

Compensation protocols ensure consistency despite partial failures. Operations execute in forward order. Compensations execute in reverse order. Compensation logging provides audit trails.

### Fault Isolation and Circuit Breakers

Implement circuit breaker patterns for fault isolation:

```rust
pub struct CircuitBreakerProtocolWrapper<E: Effects> {
    effects: E,
    circuit_state: Arc<Mutex<CircuitState>>,
    config: CircuitBreakerConfig,
}

#[derive(Clone)]
pub enum CircuitState {
    Closed { failure_count: usize },
    Open { opened_at: Instant },
    HalfOpen { test_requests: usize },
}

impl<E: Effects> CircuitBreakerProtocolWrapper<E> {
    pub async fn execute_with_circuit_breaker<T>(
        &self,
        protocol_fn: impl Fn() -> Pin<Box<dyn Future<Output = Result<T, ProtocolError>> + Send>>,
        service_name: &str,
    ) -> Result<T, ProtocolError> {
        // Check circuit state
        let should_execute = {
            let mut state = self.circuit_state.lock().unwrap();
            match &*state {
                CircuitState::Closed { failure_count } => {
                    *failure_count < self.config.failure_threshold
                }
                CircuitState::Open { opened_at } => {
                    let elapsed = opened_at.elapsed();
                    if elapsed >= self.config.recovery_timeout {
                        *state = CircuitState::HalfOpen { test_requests: 0 };
                        true
                    } else {
                        false
                    }
                }
                CircuitState::HalfOpen { test_requests } => {
                    *test_requests < self.config.test_request_threshold
                }
            }
        };
        
        if !should_execute {
            return Err(ProtocolError::CircuitBreakerOpen {
                service: service_name.to_string(),
            });
        }
        
        // Execute protocol
        match protocol_fn().await {
            Ok(result) => {
                self.record_success().await;
                Ok(result)
            }
            Err(e) => {
                self.record_failure(&e).await;
                Err(e)
            }
        }
    }
    
    async fn record_success(&self) {
        let mut state = self.circuit_state.lock().unwrap();
        *state = CircuitState::Closed { failure_count: 0 };
        
        self.effects.log_debug("Circuit breaker reset to closed state").await;
    }
    
    async fn record_failure(&self, error: &ProtocolError) {
        let mut state = self.circuit_state.lock().unwrap();
        
        match &*state {
            CircuitState::Closed { failure_count } => {
                let new_count = failure_count + 1;
                if new_count >= self.config.failure_threshold {
                    *state = CircuitState::Open { opened_at: Instant::now() };
                    
                    self.effects.log_warn(&format!(
                        "Circuit breaker opened due to {} failures",
                        new_count
                    )).await;
                } else {
                    *state = CircuitState::Closed { failure_count: new_count };
                }
            }
            CircuitState::HalfOpen { test_requests } => {
                *state = CircuitState::Open { opened_at: Instant::now() };
                
                self.effects.log_warn("Circuit breaker reopened during half-open test").await;
            }
            CircuitState::Open { .. } => {
                // Already open, no state change needed
            }
        }
    }
}
```

Circuit breakers prevent cascading failures across distributed systems. Failure thresholds trigger circuit opening. Recovery timeouts enable gradual service restoration. Half-open states test service health carefully.

## System Layering Techniques

Advanced choreographic applications layer multiple coordination mechanisms. Layering enables separation of concerns and modular system design.

### Capability Layer Integration

Layer capability checking across protocol boundaries:

```rust
pub struct LayeredCapabilityManager<E: Effects> {
    effects: E,
    capability_layers: Vec<CapabilityLayer>,
}

impl<E: Effects> LayeredCapabilityManager<E> {
    pub async fn evaluate_layered_capabilities(
        &self,
        request: LayeredCapabilityRequest,
    ) -> Result<LayeredCapabilityGrant, CapabilityError> {
        let mut capability_context = request.initial_context;
        let mut layer_grants = Vec::new();
        
        for (layer_index, layer) in self.capability_layers.iter().enumerate() {
            let layer_request = CapabilityRequest {
                requesting_device: request.requesting_device,
                operation: request.operation.clone(),
                context: capability_context.clone(),
                layer_index,
            };
            
            match layer.evaluate_capabilities(&layer_request).await {
                Ok(grant) => {
                    capability_context = grant.refined_context.clone();
                    layer_grants.push(grant);
                }
                Err(e) => {
                    return Err(CapabilityError::LayerDenied {
                        layer_index,
                        layer_name: layer.name().to_string(),
                        denial_reason: e.to_string(),
                    });
                }
            }
        }
        
        Ok(LayeredCapabilityGrant {
            final_context: capability_context,
            layer_grants,
            overall_authorization: AuthorizationLevel::Granted,
        })
    }
}
```

Capability layers refine authorization progressively. Each layer applies additional constraints. Layer failures deny the entire request. Context refinement accumulates across layers.

### Privacy Budget Layering

Layer privacy budget enforcement across protocol hierarchies:

```rust
pub struct LayeredPrivacyManager<E: Effects> {
    effects: E,
    privacy_layers: BTreeMap<PrivacyLayer, PrivacyBudgetManager>,
}

impl<E: Effects> LayeredPrivacyManager<E> {
    pub async fn charge_layered_privacy_cost(
        &self,
        operation: &PrivacyOperation,
        context: &PrivacyContext,
    ) -> Result<LayeredPrivacyReceipt, PrivacyError> {
        let mut layer_receipts = Vec::new();
        let base_cost = operation.calculate_base_cost();
        
        for (layer, budget_manager) in &self.privacy_layers {
            let layer_cost = layer.calculate_layer_cost(&base_cost, context);
            
            match budget_manager.charge_budget(context, layer_cost).await {
                Ok(receipt) => {
                    layer_receipts.push(LayerReceipt {
                        layer: layer.clone(),
                        cost_charged: layer_cost,
                        receipt,
                    });
                }
                Err(e) => {
                    // Rollback previous charges
                    self.rollback_privacy_charges(&layer_receipts).await?;
                    
                    return Err(PrivacyError::BudgetExhausted {
                        layer: layer.clone(),
                        required_cost: layer_cost,
                        cause: e,
                    });
                }
            }
        }
        
        Ok(LayeredPrivacyReceipt {
            operation_id: operation.operation_id,
            total_cost: base_cost,
            layer_receipts,
            timestamp: self.effects.current_timestamp().await,
        })
    }
}
```

Privacy budget layering enables fine-grained leakage control. Different layers track different privacy concerns. Budget exhaustion at any layer denies operations. Rollback mechanisms ensure consistency.

Advanced choreographic programming enables building sophisticated distributed systems with strong security and privacy guarantees. These patterns provide the foundation for complex real-world applications.

Continue with [Testing Guide](805_testing_guide.md) and [Simulation Guide](806_simulation_guide.md) for comprehensive protocol testing approaches using Aura's simulation infrastructure.
