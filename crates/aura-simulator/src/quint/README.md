# Quint Integration for Aura Simulator

This module provides Quint integration for the Aura simulator. It includes property evaluation, trace conversion, and formal verification support.

## Overview

The Quint integration enables formal verification of protocol implementations through property evaluation and trace analysis. The module supports real-time property validation during simulation and bidirectional conversion between Aura traces and ITF format for external verification tools.

## Property Evaluation Engine

The property evaluator validates Quint specifications against simulation state in real time. It supports multiple property types including safety, liveness, invariant, temporal, security, consensus, and performance properties.

### Core Capabilities

The evaluator provides continuous property evaluation as simulation state changes. It maintains efficient state history for temporal properties and implements high-performance caching with LRU eviction. Properties can be evaluated in batch mode, streaming mode, or on-demand.

The engine supports short-circuit evaluation for early termination and parallel evaluation of independent properties. State indexing enables fast lookups by timestamp or tick for historical queries.

### Usage Example

```rust
use aura_simulator::quint::{PropertyEvaluator, EvaluatorConfig, VerifiableProperty};

let config = EvaluatorConfig {
    max_evaluation_time_ms: 5000,
    max_history_length: 1000,
    enable_caching: true,
    enable_parallel_evaluation: true,
    ..Default::default()
};

let mut evaluator = PropertyEvaluator::with_config(config);
```

The example creates an evaluator with custom performance settings. The configuration specifies timeouts, history limits, and parallel execution options.

### Property Registration

```rust
let property = VerifiableProperty {
    id: "safety_invariant".to_string(),
    name: "no_double_spending".to_string(),
    property_type: PropertyType::Safety,
    expression: "all_transactions.no_duplicates()".to_string(),
    description: "Ensures no double spending occurs".to_string(),
    source_location: "spec.qnt:42".to_string(),
    priority: PropertyPriority::Critical,
    tags: vec!["safety".to_string()],
    continuous_monitoring: true,
};

evaluator.register_property(property)?;
```

This code registers a safety property for continuous monitoring. The evaluator tracks the property across all state transitions.

### Evaluation Modes

```rust
evaluator.set_evaluation_mode(EvaluationMode::Streaming);
let validation_result = evaluator.update_state(&world_state)?;

let result = evaluator.evaluate_all_properties(&world_state)?;
```

Streaming mode evaluates properties continuously as state changes. On-demand mode evaluates properties only when explicitly requested.

## ITF Trace Format Support

The ITF (Informal Trace Format) converter enables bidirectional conversion between Aura simulation traces and ITF format. ITF is a JSON-based format used by formal verification tools like Quint.

### ITF Format Structure

```json
{
  "#meta": {
    "format_version": "1.0",
    "source": "aura-simulator"
  },
  "vars": ["state", "counter"],
  "states": [
    {
      "#meta": {"index": 0},
      "state": "init",
      "counter": {"#bigint": "42"}
    }
  ]
}
```

The format includes metadata, variable declarations, and state sequences. Each state maps variables to typed values using ITF expression types.

### ITF Expression Types

ITF supports primitive types including boolean, string, number, and bigint. Collection types include list, set, tuple, map, and record. Special types handle unserializable data.

Primitive types represent basic values directly. Collection types use tagged JSON objects. The bigint type handles arbitrary precision integers.

### Type Conversion

```rust
use aura_simulator::quint::{ItfTraceConverter};

let mut converter = ItfTraceConverter::new();
let itf_trace = converter.aura_to_itf(&execution_trace)?;
let restored_trace = converter.itf_to_aura(&itf_trace)?;
```

The converter transforms Aura traces to ITF format and back. Type conversion preserves semantic meaning across formats.

### State Mapping

Aura simulation state maps to ITF variables according to fixed rules. The `tick` field maps to an ITF bigint variable. User-defined variables preserve their names and types. Protocol state becomes structured ITF records.

Network state converts to ITF collections. Participant states map to ITF map structures. Message statistics become ITF records with numeric fields.

### JSON Serialization

```rust
let itf_trace = converter.parse_itf_from_json(json_string)?;
let json = converter.serialize_itf_to_json(&itf_trace, true)?;
```

The converter parses ITF from JSON strings and serializes traces back to JSON. Pretty printing is optional for human-readable output.

### Validation Configuration

```rust
use aura_simulator::quint::{ItfValidationConfig};

let validation_config = ItfValidationConfig {
    validate_variable_consistency: true,
    validate_loop_bounds: true,
    validate_expression_types: true,
    max_trace_length: 10000,
    allow_unserializable: false,
};

let converter = ItfTraceConverter::with_config(
    TraceConversionConfig::default(),
    validation_config
);
```

Validation ensures ITF format compliance. The configuration enables consistency checks, type validation, and bounds checking.

### Error Handling

```rust
match converter.aura_to_itf(&execution_trace) {
    Ok(itf_trace) => println!("Conversion successful"),
    Err(ItfError::InvalidFormat(msg)) => eprintln!("Invalid format: {}", msg),
    Err(ItfError::TypeConversion(msg)) => eprintln!("Type error: {}", msg),
    Err(ItfError::Validation(msg)) => eprintln!("Validation error: {}", msg),
    Err(e) => eprintln!("Other error: {}", e),
}
```

The converter provides detailed error reporting for format violations, type mismatches, and validation failures. Each error type indicates a specific problem category.

## Configuration

### Evaluator Configuration

```rust
pub struct EvaluatorConfig {
    pub max_evaluation_time_ms: u64,
    pub max_history_length: usize,
    pub enable_caching: bool,
    pub cache_eviction_threshold: usize,
    pub enable_parallel_evaluation: bool,
    pub max_parallel_threads: usize,
    pub enable_evaluation_tracing: bool,
    pub stream_batch_size: usize,
    pub enable_short_circuit: bool,
}
```

Configuration controls evaluator behavior and performance. Timeouts prevent runaway evaluation. History limits bound memory usage. Caching and parallelization improve performance.

### Conversion Configuration

```rust
TraceConversionConfig {
    max_trace_length: 10000,
    sampling_rate: 1.0,
    compress_repeated_states: true,
    include_metadata: true,
    preserve_types: true,
}
```

Conversion settings control trace size and fidelity. Sampling reduces trace length. Compression removes duplicate states. Type preservation maintains semantic accuracy.

## State History Management

The evaluator maintains chronologically ordered state snapshots for temporal property evaluation. Snapshots include timestamps, tick numbers, and variable values. Hash-based indices enable efficient lookup by timestamp or tick.

History has configurable maximum length with automatic eviction of oldest entries. Each snapshot stores minimal state representation for memory efficiency. State hashes enable integrity checking and deduplication.

## Performance Optimizations

### Caching Strategy

The evaluator uses LRU eviction for result caching. Cache keys derive from state content hashes for precision. Access patterns determine eviction decisions.

### Parallel Evaluation

Independent properties evaluate concurrently when safe. A thread pool manages evaluation threads with configurable size. Work distribution balances load across available threads.

### Memory Management

Bounded history prevents unbounded memory growth. Hash-based indices provide fast state lookup without full scans. Snapshot compression minimizes memory footprint per state.

## Integration with Quint

### Property Verification

ITF traces enable property verification in Quint. Convert Aura simulation traces to ITF format. Load ITF traces in Quint verification environment. Verify temporal properties and invariants against execution traces.

### Trace Analysis

ITF format enables advanced trace analysis through specialized tools. Automated verification checks properties against specifications. Step-by-step debugging examines protocol execution. Regression testing compares traces across versions.

## Best Practices

### Variable Naming

Use valid identifiers for ITF variable names. Avoid reserved keywords and special characters. Choose descriptive names that map to protocol concepts.

### Type Selection

Select appropriate ITF types for data representation. Prefer primitive types when possible for better tool support. Use structured types for complex data that requires organization.

### Metadata Usage

Include meaningful metadata for trace provenance. Add timestamps for temporal analysis. Provide source information to aid debugging.

### Performance Tuning

Configure sampling rate to control trace size. Enable state compression for repetitive traces. Monitor memory usage during long simulations. Adjust cache size based on evaluation patterns.

## Error Handling

### Evaluation Errors

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

Evaluation errors indicate various failure modes. Formula errors suggest invalid property syntax. Timeout errors mean evaluation exceeded time limits. State extraction errors indicate missing or malformed state data.

### Common Issues

Variable inconsistency occurs when states have different variable sets. Type conversion errors arise from unsupported type combinations. Loop index errors indicate invalid ITF loop references. Large integers must use bigint to avoid overflow.

### Debugging

Enable detailed validation for comprehensive error reporting. Use pretty-printed JSON for manual trace inspection. Check trace completeness and variable consistency. Verify metadata integrity before processing.

## Statistics and Monitoring

```rust
pub struct EvaluationStatistics {
    pub total_evaluations: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub total_evaluation_time_ms: u64,
    pub consistent_holds: u64,
    pub consistent_failures: u64,
    pub temporal_evaluations: u64,
}
```

Statistics track evaluator performance and behavior. Cache hit rate indicates caching effectiveness. Evaluation time shows performance characteristics. Failure counts identify problematic properties.

## Testing

```bash
cargo test quint::evaluator
cargo test quint::evaluator --release -- --nocapture
cargo test quint::trace_converter
```

The first command runs all evaluator tests with standard settings. The second runs with optimizations and full output. The third tests trace conversion functionality.
