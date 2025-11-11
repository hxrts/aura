# Configuration System Reference

This document describes the Aura configuration system for managing runtime settings across all components. The system provides unified configuration traits, multiple format support, hierarchical merging, and validation.

## Architecture Overview

The configuration system uses trait-based abstractions to provide consistent configuration handling across all Aura components. Core traits define standard operations including loading from files, merging configurations, and validating settings. Format implementations support JSON, TOML, and YAML with extensibility for additional formats. Loader implementations handle file discovery, environment variable parsing, and command-line argument processing.

The hierarchical merging strategy combines configuration from multiple sources with well-defined precedence. Default values provide baseline settings suitable for most deployments. File-based configuration overrides defaults with deployment-specific settings. Environment variables override file configuration for runtime customization. Command-line arguments override environment variables for ad-hoc testing and debugging. This hierarchy allows progressive refinement from general defaults to specific runtime settings.

Validation occurs at multiple stages to catch configuration errors early. Type-level validation uses Rust's type system to enforce basic constraints like numeric ranges and enum variants. Runtime validation checks cross-field dependencies and semantic constraints. Custom validators extend validation logic for application-specific requirements. Validation errors provide detailed messages indicating which setting failed and why.

## Core Traits

The AuraConfig trait defines the interface that all configuration types implement. This trait requires Clone, Default, and Send+Sync to enable configuration sharing across threads and components. The trait provides methods for loading from files, merging with other configurations, validating settings, and parsing command-line arguments.

The load_from_file method reads configuration from a file path. The implementation detects file format from the extension and uses the appropriate format parser. Supported formats include .json, .toml, and .yaml files. The method returns a Result indicating success or failure with detailed error information.

The merge_with method combines two configurations following the merging strategy appropriate for each setting type. Numeric settings typically use the value from the other configuration when present. Collection settings may append or replace depending on semantics. The method modifies self in place and returns a Result indicating success or failure.

The validate method checks all configuration constraints and returns a Result. Validation failures include detailed error messages indicating which constraint failed. Multiple validation errors accumulate and report together rather than failing on the first error. This allows operators to fix all configuration issues in one iteration.

The ConfigDefaults trait provides default values for configuration types. Implementations define sensible defaults that work for typical deployments. Default values should prioritize safety over performance, choose conservative resource limits, and disable optional features until explicitly enabled.

The ConfigMerge trait defines merging behavior for configuration types. Implementations specify how to combine values from different sources. Merging strategies include replacing values, appending to collections, taking minimums or maximums, and custom merge logic for complex types.

The ConfigValidation trait defines validation behavior for configuration types. Implementations specify constraints that must hold for valid configurations. Validation rules include range checks for numeric values, enum variant validation for string values, dependency checks between related settings, and custom validation for complex constraints.

## Format Support

The JSON format handler parses configuration from JSON files using serde_json. JSON provides human-readable structure with support for nested objects and arrays. The format works well for complex configurations with multiple levels of nesting. JSON files use .json extension by convention.

The TOML format handler parses configuration from TOML files using the toml crate. TOML provides a minimal syntax focused on readability for configuration files. The format works well for flat configurations without deep nesting. TOML files use .toml extension by convention.

The YAML format handler parses configuration from YAML files using serde_yaml. YAML provides extensive features including anchors, aliases, and multi-document files. The format works well for configurations that benefit from reference reuse. YAML files use .yaml or .yml extension by convention.

Format detection uses file extensions to select the appropriate parser. The loader examines the file path extension and dispatches to the matching format handler. If the extension does not match a known format, the loader returns an error indicating unsupported format. Applications can register custom format handlers for additional file types.

## Configuration Loading

The ConfigLoader trait abstracts configuration loading from various sources. Implementations handle file discovery, parsing, and error recovery. The loader supports multiple sources including files, environment variables, and command-line arguments.

File loading searches for configuration files in standard locations. The search path includes the current directory, a .config subdirectory, and system configuration directories. The first file found with a matching name gets loaded. If no file exists, the loader returns default configuration rather than failing.

Environment variable loading extracts configuration from environment variables with a common prefix. The loader converts environment variable names to configuration field names using standard conventions. Variables use UPPER_CASE_WITH_UNDERSCORES format. The prefix identifies which variables belong to the application.

Command-line argument loading parses arguments into configuration settings. The loader supports both named arguments with --key=value syntax and positional arguments for common settings. Argument parsing handles boolean flags, numeric values, string values, and collections. Parse errors report the specific argument that failed with guidance for valid values.

The ConfigBuilder provides a fluent interface for constructing configurations from multiple sources. The builder starts with default values, applies file configuration if present, applies environment variables, applies command-line arguments, validates the final configuration, and returns the validated result. Each step can fail with detailed errors.

## Hierarchical Merging

Configuration merging combines values from multiple sources following precedence rules. Higher precedence sources override lower precedence sources. The standard precedence order places defaults lowest, then file configuration, then environment variables, then command-line arguments highest.

Value merging depends on the setting type. Scalar values like numbers and strings use simple replacement where the higher precedence value completely replaces the lower precedence value. Boolean values follow the same replacement strategy. Enum values validate that the replacement value represents a valid variant.

Collection merging provides two strategies depending on collection semantics. Replacement strategy discards the lower precedence collection and uses the higher precedence collection completely. This strategy suits collections representing alternatives where only one configuration applies. Append strategy combines collections by appending the higher precedence elements to the lower precedence elements. This strategy suits collections representing cumulative settings where all values apply.

Nested object merging recursively merges fields within objects. Each field merges independently according to its type. This allows partially overriding nested configurations without replacing entire objects. For example, overriding a single timeout value within a timing configuration object without replacing all timing values.

Optional value merging uses the Some variant when present regardless of precedence. If the lower precedence value has Some and the higher precedence value has None, the Some variant persists. This allows defaults to provide optional values that remain unless explicitly cleared. Explicit None values in higher precedence sources clear optional values.

## Validation Rules

Range validation checks that numeric values fall within acceptable bounds. Minimum and maximum bounds can be inclusive or exclusive. Validation fails if the value exceeds bounds with an error message indicating the value, bounds, and constraint type. Range validation applies to integers, floating-point numbers, and duration values.

Format validation checks that string values match expected patterns. Regular expressions define acceptable patterns. Common format validations include URLs, email addresses, file paths, and identifiers. Validation fails if the string does not match the pattern with an error message indicating the pattern and example valid values.

Enum validation checks that string values correspond to valid enum variants. The validator maintains a set of valid variant names. Validation fails if the string does not match any variant with an error message listing all valid variants. This prevents typos in configuration values that would cause runtime errors.

Dependency validation checks constraints between related settings. Some settings only make sense when other settings have specific values. For example, a connection timeout only applies when connections are enabled. Dependency validation ensures that related settings maintain consistency. Validation fails if dependencies conflict with an error message explaining the constraint.

Custom validation implements application-specific constraints beyond standard validators. Custom validators receive the complete configuration and can check arbitrary invariants. Common custom validations include checking that referenced resources exist, verifying that collections contain required elements, and ensuring that computed values fall within derived bounds.

## Usage Patterns

Component configuration follows a standard pattern. Define a configuration struct with all settings as fields. Derive or implement AuraConfig, ConfigDefaults, ConfigMerge, and ConfigValidation traits. Use attribute macros to specify validation rules and merge strategies. Load configuration during component initialization using ConfigBuilder.

Application configuration composes component configurations. The application configuration struct contains fields for each component configuration. The application implements AuraConfig by delegating to component implementations. This compositional approach allows components to define their configuration independently while the application coordinates overall settings.

Testing configuration provides deterministic settings for reproducible tests. Test configurations use fixed values rather than defaults that might change. Test configurations disable external dependencies like network connections. Test configurations minimize resource usage to allow running many tests concurrently. The ConfigBuilder supports creating test configurations directly without file loading.

Deployment configuration organizes settings by environment. Development, staging, and production environments use different configuration files with environment-specific settings. The application selects the appropriate file based on an environment variable or command-line argument. Common patterns include connection strings pointing to environment-specific services and resource limits tuned for environment characteristics.

## Extension Points

Custom format handlers extend format support beyond built-in JSON, TOML, and YAML. Implement the ConfigFormat trait specifying how to parse configuration from a byte stream. Register the custom format with the loader associating file extensions with the handler. This allows applications to use specialized configuration formats suited to their needs.

Custom validators extend validation beyond built-in rules. Implement the ValidationRule trait specifying how to check a specific constraint. Register custom validators with the configuration type. Multiple validators can apply to the same configuration with all validators executing during validation.

Custom merge strategies extend merging beyond standard approaches. Implement the ConfigMerge trait specifying how to combine two configuration values. This allows fine-grained control over how configuration from different sources combines. Custom merge strategies enable domain-specific merging semantics that standard strategies cannot express.

Configuration transformations modify loaded configuration before use. Transformations run after loading and merging but before validation. Common transformations include expanding environment variables in string values, resolving relative paths to absolute paths, and substituting placeholders with computed values. Transformations enable dynamic configuration that adapts to the runtime environment.

## Error Handling

Configuration errors use structured error types that distinguish different failure modes. Loading errors indicate file not found, parse failures, or unsupported formats. Merging errors indicate incompatible values or merge strategy failures. Validation errors indicate constraint violations with details about which constraint failed. Each error type provides sufficient context for diagnosis and correction.

Error messages should guide operators toward resolution. Include the setting name, the invalid value, the constraint that failed, and example valid values. For complex constraints, explain the relationship between settings that caused the failure. Avoid technical jargon in error messages when simpler explanations suffice.

Error recovery strategies depend on error type and deployment context. Missing configuration files can fall back to defaults if defaults provide sufficient settings. Invalid values can use fallback values if safe fallbacks exist. Some errors require immediate failure to prevent incorrect behavior. The configuration system provides hooks for applications to implement custom recovery strategies appropriate to their requirements.
