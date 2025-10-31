# Quint Property Evaluation Engine

This module provides a production-ready, high-performance property evaluation engine for real-time validation of Quint specifications against simulation state.

## Features

### Core Capabilities

- **Real-time Property Evaluation**: Continuous evaluation of properties as simulation state changes
- **Temporal Logic Support**: Full support for LTL/CTL temporal properties with state history tracking
- **Multiple Property Types**: Support for safety, liveness, invariant, temporal, security, consensus, and performance properties
- **High-Performance Caching**: LRU-based result caching with configurable eviction policies
- **State History Management**: Efficient storage and querying of historical states for temporal properties

### Evaluation Modes

1. **Batch Mode**: Evaluate all properties once against current state
2. **Streaming Mode**: Continuously evaluate properties as state changes
3. **On-Demand Mode**: Evaluate specific properties when requested

### Optimization Features

- **Short-circuit Evaluation**: Early termination for obvious results
- **Parallel Evaluation**: Concurrent evaluation of independent properties
- **Result Caching**: Avoid re-evaluation of identical states
- **Efficient State Indexing**: Fast lookup of historical states by timestamp or tick

## Architecture

### Key Components

1. **PropertyEvaluator**: Main evaluation engine
2. **StateHistory**: Manages temporal state snapshots
3. **EvaluationCache**: High-performance result caching
4. **WorldStateAdapter**: Adapter for simulation state integration

### Property Types Supported

- **Invariant Properties**: Always hold in reachable states
- **Safety Properties**: Something bad never happens
- **Liveness Properties**: Something good eventually happens
- **Temporal Properties**: Complex temporal logic (LTL/CTL)
- **Security Properties**: Cryptographic and access control
- **Consensus Properties**: Agreement and consistency
- **Performance Properties**: Timing and resource constraints

## Usage

```rust
use aura_simulator::quint::{PropertyEvaluator, EvaluatorConfig, VerifiableProperty, PropertyType};

// Create evaluator with custom configuration
let config = EvaluatorConfig {
    max_evaluation_time_ms: 5000,
    max_history_length: 1000,
    enable_caching: true,
    enable_parallel_evaluation: true,
    ..Default::default()
};

let mut evaluator = PropertyEvaluator::with_config(config);

// Register properties for evaluation
let property = VerifiableProperty {
    id: "safety_invariant".to_string(),
    name: "no_double_spending".to_string(),
    property_type: PropertyType::Safety,
    expression: "all_transactions.no_duplicates()".to_string(),
    description: "Ensures no double spending occurs".to_string(),
    source_location: "spec.qnt:42".to_string(),
    priority: PropertyPriority::Critical,
    tags: vec!["safety".to_string(), "consensus".to_string()],
    continuous_monitoring: true,
};

evaluator.register_property(property)?;

// Update state and evaluate (streaming mode)
evaluator.set_evaluation_mode(EvaluationMode::Streaming);
let validation_result = evaluator.update_state(&world_state)?;

// Or evaluate on-demand
let result = evaluator.evaluate_all_properties(&world_state)?;
```

## Configuration

```rust
pub struct EvaluatorConfig {
    /// Maximum evaluation time per property (milliseconds)
    pub max_evaluation_time_ms: u64,
    /// Maximum state history to maintain
    pub max_history_length: usize,
    /// Enable result caching for performance
    pub enable_caching: bool,
    /// Cache eviction threshold
    pub cache_eviction_threshold: usize,
    /// Enable parallel evaluation
    pub enable_parallel_evaluation: bool,
    /// Maximum parallel threads
    pub max_parallel_threads: usize,
    /// Enable detailed evaluation tracing
    pub enable_evaluation_tracing: bool,
    /// Batch size for streaming evaluation
    pub stream_batch_size: usize,
    /// Enable optimized short-circuit evaluation
    pub enable_short_circuit: bool,
}
```

## State History Management

The evaluator maintains an efficient history of simulation states to support temporal property evaluation:

```rust
pub struct StateHistory {
    /// Chronologically ordered state snapshots
    snapshots: VecDeque<StateSnapshot>,
    /// Index by timestamp for efficient lookup
    timestamp_index: HashMap<u64, usize>,
    /// Index by tick for range queries
    tick_index: HashMap<u64, usize>,
}
```

### State Snapshot Structure

```rust
pub struct StateSnapshot {
    /// Timestamp when this snapshot was taken
    pub timestamp: u64,
    /// Simulation tick for this snapshot
    pub tick: u64,
    /// State variables at this point in time
    pub variables: HashMap<String, QuintValue>,
    /// Metadata about the state
    pub metadata: HashMap<String, QuintValue>,
    /// Hash of the state for integrity checking
    pub state_hash: u64,
}
```

## Performance Optimizations

### Caching Strategy

- **LRU Eviction**: Least recently used cache entries are evicted first
- **State Hash Keys**: Cache keys based on state content hashes for precision
- **Access Tracking**: Efficient tracking of cache access patterns

### Parallel Evaluation

- **Independent Properties**: Properties are evaluated concurrently when safe
- **Thread Pool Management**: Configurable number of evaluation threads
- **Load Balancing**: Work distribution across available threads

### Memory Management

- **Bounded History**: Configurable maximum history length with automatic eviction
- **Efficient Indexing**: Hash-based indices for fast state lookup
- **Snapshot Compression**: Minimal state representation for memory efficiency

## Error Handling

The evaluator provides comprehensive error handling for various failure modes:

```rust
pub enum EvaluationError {
    EvaluationFailed(String),
    InvalidTemporalFormula(String),
    InsufficientHistory(String),
    EvaluationTimeout { property: String, timeout_ms: u64 },
    InvalidPropertyType(String),
    CacheCorruption(String),
    StateExtraction(String),
}
```

## Integration with Quint Infrastructure

The evaluator integrates seamlessly with the existing Quint infrastructure:

- **QuintBridge**: Load properties from .qnt specification files
- **PropertyExtractor**: Convert Quint properties to evaluatable form
- **TraceConverter**: Convert evaluation results to Quint trace format
- **ChaosGenerator**: Generate test scenarios targeting specific properties

## Statistics and Monitoring

The evaluator provides detailed statistics for performance monitoring:

```rust
pub struct EvaluationStatistics {
    pub total_evaluations: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub total_evaluation_time_ms: u64,
    pub consistent_holds: u64,
    pub consistent_failures: u64,
    pub temporal_evaluations: u64,
    pub invariant_evaluations: u64,
    pub safety_evaluations: u64,
}
```

## Future Enhancements

### Planned Features

1. **True Quint Integration**: Direct integration with Quint evaluator backend
2. **Advanced Temporal Logic**: Support for more complex temporal operators
3. **Distributed Evaluation**: Multi-node property evaluation for large simulations
4. **Property Dependencies**: Evaluation optimization based on property relationships
5. **Machine Learning Integration**: Predictive property violation detection

### Performance Improvements

1. **SIMD Optimization**: Vectorized property evaluation where applicable
2. **GPU Acceleration**: Parallel evaluation on GPU for massive property sets
3. **Incremental Evaluation**: Only re-evaluate properties affected by state changes
4. **Adaptive Caching**: Dynamic cache sizing based on evaluation patterns

## Contributing

When extending the evaluator:

1. **Maintain Type Safety**: All property expressions should be statically validated
2. **Preserve Performance**: New features must not degrade evaluation performance
3. **Add Comprehensive Tests**: Include both unit and integration tests
4. **Document Performance Characteristics**: Specify time/space complexity
5. **Follow Error Handling Patterns**: Use consistent error types and messages

## Testing

The evaluator includes comprehensive test coverage:

```bash
# Run evaluator tests
cargo test quint::evaluator

# Run with performance profiling
cargo test quint::evaluator --release -- --nocapture

# Run specific test categories
cargo test quint::evaluator::test_property_evaluation
cargo test quint::evaluator::test_state_history
cargo test quint::evaluator::test_evaluation_cache
```

## License

This code is part of the Aura project and follows the same licensing terms.