# Debug Tools Usage Guide

This guide covers the comprehensive debugging tools available in Aura's simulation framework, including interactive debugging sessions, time travel debugging, and automated analysis capabilities.

## Overview

The Aura debug tools provide:
- **Interactive Debug Sessions**: Step-by-step execution analysis
- **Time Travel Debugging**: Navigate through execution history
- **Property Violation Analysis**: Detailed failure investigation
- **Minimal Reproduction**: Automated minimal test case generation
- **Execution Trace Analysis**: Deep dive into protocol behavior
- **Performance Profiling**: Resource usage and bottleneck detection

## Quick Start

### Basic Debug Session

```bash
# Start an interactive debug session
aura debug session create --scenario scenarios/core_protocols/threshold_key_generation.toml

# List active sessions
aura debug session list

# Resume a session
aura debug session resume <session-id>
```

### Analyzing Property Violations

```bash
# Analyze a property violation
aura debug analyze --violation property_violation.json --detailed

# Generate minimal reproduction
aura debug reproduce --violation property_violation.json --strategy minimal
```

### Time Travel Debugging

```bash
# Navigate through execution timeline
aura debug time-travel --session <session-id> --checkpoint initial
aura debug time-travel --session <session-id> --step-forward 10
aura debug time-travel --session <session-id> --breakpoint property_check
```

## Command Reference

### Session Management

#### Create Debug Session
```bash
aura debug session create [OPTIONS]

Options:
  --scenario <PATH>           Scenario file to debug
  --breakpoints <LIST>        Set initial breakpoints (comma-separated)
  --auto-checkpoints         Enable automatic checkpoint creation
  --trace-level <LEVEL>      Set trace verbosity (error|warn|info|debug|trace)
  --max-steps <N>            Maximum execution steps before auto-pause
  --interactive              Start in interactive mode

Examples:
  # Create basic session
  aura debug session create --scenario scenarios/dkd_basic.toml

  # Create session with breakpoints
  aura debug session create \
    --scenario scenarios/threshold_key_generation.toml \
    --breakpoints "frost_setup,frost_sign,frost_aggregate" \
    --auto-checkpoints

  # Create session with custom limits
  aura debug session create \
    --scenario scenarios/adversarial/byzantine_coordinator.toml \
    --max-steps 1000 \
    --trace-level debug
```

#### List Debug Sessions
```bash
aura debug session list [OPTIONS]

Options:
  --filter <STATUS>          Filter by status (active|paused|completed|failed)
  --sort <FIELD>             Sort by field (created|name|status|duration)
  --format <FORMAT>          Output format (table|json|yaml)

Examples:
  # List all sessions
  aura debug session list

  # List only active sessions
  aura debug session list --filter active

  # List with JSON output
  aura debug session list --format json
```

#### Resume Debug Session
```bash
aura debug session resume [OPTIONS] <SESSION_ID>

Options:
  --continue                 Continue execution immediately
  --step                     Execute single step then pause
  --breakpoint <NAME>        Run until specific breakpoint
  --checkpoint <NAME>        Resume from specific checkpoint

Examples:
  # Resume and continue execution
  aura debug session resume abc123 --continue

  # Resume and step through
  aura debug session resume abc123 --step

  # Resume from checkpoint
  aura debug session resume abc123 --checkpoint pre_signing
```

#### Control Debug Session
```bash
aura debug session control [OPTIONS] <SESSION_ID>

Options:
  --pause                    Pause execution
  --stop                     Stop session
  --reset                    Reset to initial state
  --step-forward <N>         Step forward N execution steps
  --step-backward <N>        Step backward N execution steps

Examples:
  # Pause running session
  aura debug session control abc123 --pause

  # Step forward 5 steps
  aura debug session control abc123 --step-forward 5

  # Reset session
  aura debug session control abc123 --reset
```

### Property Analysis

#### Analyze Violations
```bash
aura debug analyze [OPTIONS]

Options:
  --violation <PATH>         Property violation file to analyze
  --session <SESSION_ID>     Analyze specific debug session
  --property <NAME>          Focus on specific property
  --detailed                 Generate detailed analysis report
  --trace-analysis          Include execution trace analysis
  --causal-analysis         Perform causal chain analysis
  --output <PATH>            Save analysis to file

Examples:
  # Basic violation analysis
  aura debug analyze --violation violation_report.json

  # Detailed analysis with traces
  aura debug analyze \
    --violation violation_report.json \
    --detailed \
    --trace-analysis \
    --output detailed_analysis.json

  # Analyze specific property in session
  aura debug analyze \
    --session abc123 \
    --property threshold_security_maintained \
    --causal-analysis
```

#### Root Cause Analysis
```bash
aura debug analyze root-cause [OPTIONS]

Options:
  --violation <PATH>         Violation to analyze
  --depth <N>                Analysis depth (default: 10)
  --include-external         Include external factors
  --hypothesis-testing       Enable hypothesis testing
  --confidence-threshold     Minimum confidence level (0.0-1.0)

Examples:
  # Deep root cause analysis
  aura debug analyze root-cause \
    --violation violation.json \
    --depth 15 \
    --include-external \
    --hypothesis-testing

  # High-confidence analysis
  aura debug analyze root-cause \
    --violation violation.json \
    --confidence-threshold 0.8
```

### Time Travel Debugging

#### Navigate Timeline
```bash
aura debug time-travel [OPTIONS] <SESSION_ID>

Options:
  --checkpoint <NAME>        Jump to named checkpoint
  --step-forward <N>         Move forward N steps
  --step-backward <N>        Move backward N steps
  --timestamp <TIME>         Jump to specific timestamp
  --breakpoint <NAME>        Continue to breakpoint
  --interactive              Enter interactive navigation mode

Examples:
  # Jump to specific checkpoint
  aura debug time-travel abc123 --checkpoint frost_setup_complete

  # Move through execution
  aura debug time-travel abc123 --step-forward 10
  aura debug time-travel abc123 --step-backward 5

  # Interactive navigation
  aura debug time-travel abc123 --interactive
```

#### Checkpoint Management
```bash
aura debug checkpoint [OPTIONS] <SESSION_ID>

Options:
  --create <NAME>            Create checkpoint at current position
  --list                     List all checkpoints
  --delete <NAME>            Delete checkpoint
  --info <NAME>              Show checkpoint information

Examples:
  # Create checkpoint
  aura debug checkpoint abc123 --create pre_aggregation

  # List checkpoints
  aura debug checkpoint abc123 --list

  # Show checkpoint details
  aura debug checkpoint abc123 --info frost_setup_complete
```

### Minimal Reproduction

#### Generate Minimal Cases
```bash
aura debug reproduce [OPTIONS]

Options:
  --violation <PATH>         Violation to reproduce
  --strategy <STRATEGY>      Reduction strategy (minimal|binary|targeted)
  --max-iterations <N>       Maximum reduction iterations
  --preserve-properties      Preserve specific properties during reduction
  --output-scenario <PATH>   Save minimal scenario to file
  --validate                 Validate reproduction triggers same violation

Examples:
  # Generate minimal reproduction
  aura debug reproduce \
    --violation complex_violation.json \
    --strategy minimal \
    --output-scenario minimal_repro.toml

  # Binary search reduction
  aura debug reproduce \
    --violation violation.json \
    --strategy binary \
    --max-iterations 20 \
    --validate

  # Preserve specific properties
  aura debug reproduce \
    --violation violation.json \
    --preserve-properties "byzantine_tolerance,network_reliability" \
    --output-scenario targeted_repro.toml
```

### Execution Inspection

#### Inspect State
```bash
aura debug inspect [OPTIONS] <SESSION_ID>

Options:
  --state                    Show current execution state
  --participants             Show participant states
  --network                  Show network state
  --properties               Show property evaluation status
  --resources                Show resource usage
  --format <FORMAT>          Output format (json|yaml|table)

Examples:
  # Inspect current state
  aura debug inspect abc123 --state

  # Check participant status
  aura debug inspect abc123 --participants --format table

  # Full system inspection
  aura debug inspect abc123 --state --network --properties --resources
```

#### Trace Analysis
```bash
aura debug trace [OPTIONS] <SESSION_ID>

Options:
  --export <PATH>            Export trace to file
  --format <FORMAT>          Export format (json|csv|quint)
  --filter <FILTER>          Filter trace events
  --compress                 Compress exported trace
  --from-checkpoint <NAME>   Start from checkpoint
  --to-checkpoint <NAME>     End at checkpoint

Examples:
  # Export full trace
  aura debug trace abc123 --export full_trace.json

  # Export filtered trace
  aura debug trace abc123 \
    --export filtered_trace.json \
    --filter "event_type=protocol_message"

  # Export trace segment
  aura debug trace abc123 \
    --export segment_trace.json \
    --from-checkpoint pre_signing \
    --to-checkpoint post_aggregation
```

### Report Generation

#### Generate Debug Reports
```bash
aura debug report [OPTIONS]

Options:
  --session <SESSION_ID>     Generate report for session
  --violation <PATH>         Generate report for violation
  --template <TEMPLATE>      Report template (summary|detailed|executive)
  --output <PATH>            Output file path
  --format <FORMAT>          Report format (html|pdf|markdown|json)
  --include-traces           Include execution traces
  --include-analysis         Include automated analysis
  --compress                 Compress large reports

Examples:
  # Generate summary report
  aura debug report \
    --session abc123 \
    --template summary \
    --output session_summary.html

  # Detailed violation report
  aura debug report \
    --violation violation.json \
    --template detailed \
    --format pdf \
    --include-traces \
    --include-analysis \
    --output detailed_violation_report.pdf

  # Executive summary
  aura debug report \
    --session abc123 \
    --template executive \
    --format markdown \
    --output executive_summary.md
```

## Interactive Debugging Workflows

### Workflow 1: Protocol Development Debugging

```bash
# 1. Create debug session for new protocol
aura debug session create \
  --scenario scenarios/new_protocol.toml \
  --breakpoints "setup,execute,verify" \
  --auto-checkpoints \
  --interactive

# 2. Step through protocol phases
# (In interactive mode, use commands like 'step', 'continue', 'inspect')

# 3. Analyze any failures
aura debug analyze --session <session-id> --detailed

# 4. Generate report
aura debug report --session <session-id> --template detailed
```

### Workflow 2: Property Violation Investigation

```bash
# 1. Start with violation file
aura debug analyze --violation property_violation.json --detailed

# 2. Create reproduction scenario
aura debug reproduce \
  --violation property_violation.json \
  --strategy minimal \
  --output-scenario minimal_repro.toml

# 3. Debug minimal reproduction
aura debug session create \
  --scenario minimal_repro.toml \
  --trace-level debug

# 4. Time travel to understand root cause
aura debug time-travel <session-id> --interactive

# 5. Generate comprehensive report
aura debug report \
  --violation property_violation.json \
  --template detailed \
  --include-analysis \
  --format html
```

### Workflow 3: Performance Analysis

```bash
# 1. Create session with resource monitoring
aura debug session create \
  --scenario scenarios/performance_test.toml \
  --trace-level info \
  --auto-checkpoints

# 2. Monitor resource usage during execution
aura debug inspect <session-id> --resources

# 3. Analyze performance bottlenecks
aura debug analyze --session <session-id> --trace-analysis

# 4. Export detailed traces for external analysis
aura debug trace <session-id> \
  --export performance_trace.json \
  --compress
```

## Advanced Features

### Custom Breakpoints

```bash
# Set conditional breakpoints
aura debug session create \
  --scenario scenarios/complex_scenario.toml \
  --breakpoints "participant_count > 5,byzantine_detected"

# Runtime breakpoint management
aura debug session control <session-id> --add-breakpoint "custom_condition"
aura debug session control <session-id> --remove-breakpoint "old_condition"
```

### Trace Filtering and Analysis

```bash
# Filter traces by participant
aura debug trace <session-id> \
  --filter "participant_id=alice" \
  --export alice_trace.json

# Filter by time range
aura debug trace <session-id> \
  --filter "timestamp > 1000 AND timestamp < 5000" \
  --export time_segment.json

# Filter by event type
aura debug trace <session-id> \
  --filter "event_type IN [protocol_message, state_change]" \
  --export filtered_events.json
```

### Integration with External Tools

```bash
# Export for Quint analysis
aura debug trace <session-id> \
  --export quint_trace.itf \
  --format quint

# Export for visualization
aura debug trace <session-id> \
  --export visualization_data.json \
  --format json \
  --include-metadata

# Integration with CI/CD
aura debug report \
  --session <session-id> \
  --format json \
  --output ci_debug_report.json
```

## Troubleshooting

### Common Issues

#### Session Creation Fails
```bash
# Check scenario validity
aura scenarios validate --scenario your_scenario.toml

# Check resource availability
aura debug session list --filter active
```

#### Performance Issues
```bash
# Use sampling for large traces
aura debug session create \
  --scenario large_scenario.toml \
  --trace-sampling 0.1

# Limit execution steps
aura debug session create \
  --scenario complex_scenario.toml \
  --max-steps 1000
```

#### Memory Usage
```bash
# Enable trace compression
aura debug trace <session-id> \
  --export compressed_trace.json \
  --compress

# Use minimal trace format
aura debug session create \
  --scenario scenario.toml \
  --trace-level warn
```

### Best Practices

1. **Use Checkpoints Strategically**: Create checkpoints at protocol boundaries
2. **Filter Traces Early**: Apply filters during export to reduce file sizes
3. **Incremental Analysis**: Start with summary analysis before detailed investigation
4. **Preserve Evidence**: Always save violation files and minimal reproductions
5. **Document Findings**: Use report generation for team communication

## Integration with Development Workflow

### Pre-commit Debugging
```bash
# Quick validation before commit
just debug-scenario scenarios/critical_path.toml

# Automated debugging in CI
just debug-all-scenarios --fail-fast
```

### Continuous Integration
```bash
# CI debug pipeline
aura debug session create --scenario $SCENARIO --non-interactive
aura debug analyze --session $SESSION_ID --output ci_analysis.json
aura debug report --session $SESSION_ID --template summary --format json
```

For more information, see:
- [Quint Integration Guide](quint_integration_guide.md)
- [Scenario Composition Guide](scenario_composition_guide.md)
- [CI/CD Integration Guide](cicd_integration_guide.md)