# ITF Trace Format Support Guide

This guide documents the complete ITF (Informal Trace Format) integration in the Aura simulator, including bidirectional conversion between Aura simulation traces and ITF format for Quint formal verification.

## Overview

The ITF (Informal Trace Format) is a JSON-based format for representing state machine executions and counterexamples. It provides a bridge between simulation execution traces and formal verification tools like Quint.

### Key Features

- **Full ITF Specification Compliance**: Complete support for all ITF expression types
- **Bidirectional Conversion**: Seamless conversion between Aura and ITF formats
- **Comprehensive Validation**: Extensive error checking and format compliance
- **Type Safety**: Robust type conversion with detailed error reporting
- **Production Ready**: Optimized for real-world Quint-generated ITF traces

## ITF Format Structure

### Basic ITF Trace

```json
{
  "#meta": {
    "format_version": "1.0",
    "source": "aura-simulator",
    "created_at": "2023-01-01T00:00:00Z"
  },
  "params": ["threshold", "participants"],
  "vars": ["state", "counter", "participants"],
  "states": [
    {
      "#meta": {
        "index": 0,
        "timestamp": 1672531200000
      },
      "state": "init",
      "counter": {"#bigint": "42"},
      "participants": {
        "#set": ["alice", "bob", "charlie"]
      }
    }
  ],
  "loop": 0
}
```

### ITF Expression Types

The ITF format supports the following expression types:

#### Primitive Types
- `boolean`: `true` or `false`
- `string`: `"hello world"`
- `number`: `42` (for small integers)
- `bigint`: `{"#bigint": "999999999999999999999"}`

#### Collection Types
- `list`: `[1, 2, 3]`
- `set`: `{"#set": ["a", "b", "c"]}`
- `tuple`: `{"#tup": ["first", 42, true]}`
- `map`: `{"#map": [["key1", "value1"], ["key2", "value2"]]}`
- `record`: `{"field1": "value1", "field2": 42}`

#### Special Types
- `unserializable`: `{"#unserializable": "Complex data structure"}`

## Aura ↔ ITF Mapping

### Aura Simulation State → ITF State

| Aura Field | ITF Variable | Type | Description |
|------------|--------------|------|-------------|
| `tick` | `tick` | bigint | Simulation tick counter |
| `time` | `time` | bigint | Simulation timestamp |
| `variables.*` | `*` | mixed | User-defined variables |
| `protocol_state.current_phase` | `protocol_phase` | string | Current protocol phase |
| `protocol_state.active_sessions` | `active_sessions` | list of records | Active protocol sessions |
| `participant_states` | `participants` | map | Participant state snapshots |
| `network_state.partitions` | `network_partitions` | list of lists | Network partition groups |
| `network_state.message_stats` | `message_stats` | record | Message delivery statistics |

### Type Conversion Rules

#### Numbers
- Small integers (-2³¹ to 2³¹-1) → JSON number
- Large integers → ITF bigint with `#bigint` tag
- Floating point → String representation

#### Collections
- Aura `Vec<T>` → ITF list `[...]`
- Aura `HashSet<T>` → ITF set `{"#set": [...]}`
- Aura `HashMap<String, T>` → ITF record `{"key": value}`
- Aura `HashMap<T, U>` → ITF map `{"#map": [[key, value]]}`

#### Complex Structures
- Session information → ITF record with structured fields
- Network topology → ITF list of participant groups
- Protocol variables → ITF record with type preservation

## Usage Examples

### Basic Conversion

```rust
use crate::quint::trace_converter::{ItfTraceConverter, ItfTrace};

let mut converter = ItfTraceConverter::new();

// Convert Aura trace to ITF
let itf_trace = converter.aura_to_itf(&execution_trace)?;

// Convert ITF back to Aura
let restored_trace = converter.itf_to_aura(&itf_trace)?;
```

### JSON Serialization

```rust
// Parse ITF from JSON string
let itf_trace = converter.parse_itf_from_json(json_string)?;

// Serialize to pretty JSON
let json = converter.serialize_itf_to_json(&itf_trace, true)?;
```

### Advanced Validation

```rust
use crate::quint::trace_converter::{ItfValidationConfig};

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

### Error Handling

```rust
match converter.aura_to_itf(&execution_trace) {
    Ok(itf_trace) => {
        println!("Conversion successful");
    },
    Err(ItfError::InvalidFormat(msg)) => {
        eprintln!("Invalid format: {}", msg);
    },
    Err(ItfError::TypeConversion(msg)) => {
        eprintln!("Type conversion error: {}", msg);
    },
    Err(ItfError::Validation(msg)) => {
        eprintln!("Validation error: {}", msg);
    },
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

## Compliance and Validation

### Format Compliance

The ITF converter ensures full compliance with the ITF specification:

- Required fields (`vars`, `states`) validation
- Optional fields (`meta`, `params`, `loop`) support
- Variable name uniqueness checking
- State variable consistency validation
- Loop index bounds checking
- Expression type validation

### Error Detection

The converter detects and reports various error conditions:

- **Malformed JSON**: Syntax errors, invalid structure
- **Missing Fields**: Required ITF fields not present
- **Type Mismatches**: Invalid expression types
- **Inconsistent Variables**: Variable sets differ between states
- **Invalid Identifiers**: Non-compliant variable names
- **Loop Errors**: Invalid loop indices

### Validation Configuration

```rust
ItfValidationConfig {
    validate_variable_consistency: bool,  // Check variable consistency across states
    validate_loop_bounds: bool,           // Validate loop index bounds
    validate_expression_types: bool,      // Type-check ITF expressions
    max_trace_length: usize,             // Maximum allowed trace length
    allow_unserializable: bool,          // Allow unserializable expressions
}
```

## Performance Considerations

### Large Traces

For large simulation traces:

- **Sampling**: Configure sampling rate to reduce trace size
- **Compression**: Enable repeated state compression
- **Memory**: Monitor memory usage during conversion
- **Streaming**: Consider streaming conversion for very large traces

### Optimization Settings

```rust
TraceConversionConfig {
    max_trace_length: 10000,           // Limit trace size
    sampling_rate: 1.0,                // No sampling by default
    compress_repeated_states: true,     // Enable compression
    include_metadata: true,             // Include ITF metadata
    preserve_types: true,               // Maintain type information
}
```

## Integration with Quint

### Property Verification

ITF traces can be used with Quint for property verification:

1. Convert Aura simulation trace to ITF format
2. Load ITF trace in Quint verification environment
3. Verify temporal properties and invariants
4. Analyze property violations and counterexamples

### Trace Analysis

ITF format enables advanced trace analysis:

- **Visualization**: ITF traces can be visualized in specialized tools
- **Property Checking**: Automated verification against formal specifications
- **Debugging**: Step-by-step analysis of protocol execution
- **Regression Testing**: Compare traces across protocol versions

## Best Practices

### Variable Naming

- Use valid identifiers for variable names
- Avoid reserved keywords and special characters
- Use descriptive names that map to protocol concepts

### Type Selection

- Use appropriate ITF types for data representation
- Prefer primitive types when possible for better tool support
- Use structured types (records, maps) for complex data

### Metadata Usage

- Include meaningful metadata for trace provenance
- Use timestamps for temporal analysis
- Add source information for debugging

### Error Handling

- Always validate ITF traces before processing
- Handle conversion errors gracefully
- Provide detailed error messages for debugging

## Troubleshooting

### Common Issues

1. **Variable Inconsistency**: Ensure all states have the same variable set
2. **Type Conversion Errors**: Check for unsupported type combinations
3. **Loop Index Errors**: Verify loop indices are within valid range
4. **JSON Syntax Errors**: Validate JSON format before parsing
5. **Large Integer Overflow**: Use bigint for numbers exceeding 32-bit range

### Debugging Tips

- Enable detailed validation for comprehensive error reporting
- Use pretty-printed JSON for manual inspection
- Check trace completeness and consistency
- Verify metadata integrity

### Performance Issues

- Reduce trace length through sampling
- Enable state compression for repetitive traces
- Monitor memory usage during conversion
- Consider batch processing for multiple traces

## Future Enhancements

- **Streaming Conversion**: Support for very large traces
- **Incremental Updates**: Efficient trace modification
- **Custom Serializers**: Domain-specific ITF extensions
- **Performance Optimization**: Faster conversion algorithms
- **Enhanced Validation**: More sophisticated compliance checking
