//! Configuration validation utilities and rules

use crate::AuraError;
use std::fmt;

/// Configuration validation result
pub type ValidationResult = Result<(), ValidationError>;

/// Configuration validation errors
#[derive(Debug, Clone)]
pub enum ValidationError {
    /// Value is required but missing
    Required { field: String },
    /// Value is out of acceptable range
    OutOfRange { field: String, min: Option<f64>, max: Option<f64>, actual: f64 },
    /// Value format is invalid
    InvalidFormat { field: String, expected: String, actual: String },
    /// Custom validation failed
    Custom { field: String, message: String },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::Required { field } => {
                write!(f, "Field '{}' is required but missing", field)
            }
            ValidationError::OutOfRange { field, min, max, actual } => {
                let range_desc = match (min, max) {
                    (Some(min), Some(max)) => format!("between {} and {}", min, max),
                    (Some(min), None) => format!("at least {}", min),
                    (None, Some(max)) => format!("at most {}", max),
                    (None, None) => "in valid range".to_string(),
                };
                write!(f, "Field '{}' must be {} (got {})", field, range_desc, actual)
            }
            ValidationError::InvalidFormat { field, expected, actual } => {
                write!(f, "Field '{}' has invalid format. Expected: {}, got: {}", field, expected, actual)
            }
            ValidationError::Custom { field, message } => {
                write!(f, "Field '{}': {}", field, message)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

impl From<ValidationError> for AuraError {
    fn from(err: ValidationError) -> Self {
        AuraError::invalid(err.to_string())
    }
}

/// Configuration validator that accumulates validation rules
pub struct ConfigValidator {
    errors: Vec<ValidationError>,
    field_prefix: String,
}

impl ConfigValidator {
    /// Create a new validator
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            field_prefix: String::new(),
        }
    }
    
    /// Create a validator for a nested field
    pub fn for_field(&self, field_name: &str) -> Self {
        let prefix = if self.field_prefix.is_empty() {
            field_name.to_string()
        } else {
            format!("{}.{}", self.field_prefix, field_name)
        };
        
        Self {
            errors: Vec::new(),
            field_prefix: prefix,
        }
    }
    
    /// Validate that a value is present
    pub fn required<T>(&mut self, field_name: &str, value: &Option<T>) -> &mut Self {
        if value.is_none() {
            self.errors.push(ValidationError::Required {
                field: self.full_field_name(field_name),
            });
        }
        self
    }
    
    /// Validate that a number is within range
    pub fn range<T>(&mut self, field_name: &str, value: T, min: Option<T>, max: Option<T>) -> &mut Self
    where
        T: PartialOrd + Copy + Into<f64>,
    {
        let mut out_of_range = false;
        
        if let Some(min_val) = min {
            if value < min_val {
                out_of_range = true;
            }
        }
        
        if let Some(max_val) = max {
            if value > max_val {
                out_of_range = true;
            }
        }
        
        if out_of_range {
            self.errors.push(ValidationError::OutOfRange {
                field: self.full_field_name(field_name),
                min: min.map(|v| v.into()),
                max: max.map(|v| v.into()),
                actual: value.into(),
            });
        }
        
        self
    }
    
    /// Validate using a custom predicate
    pub fn custom<T, F>(&mut self, field_name: &str, value: &T, predicate: F, message: &str) -> &mut Self
    where
        F: FnOnce(&T) -> bool,
    {
        if !predicate(value) {
            self.errors.push(ValidationError::Custom {
                field: self.full_field_name(field_name),
                message: message.to_string(),
            });
        }
        self
    }
    
    /// Validate string format using regex
    pub fn format(&mut self, field_name: &str, value: &str, pattern: &str) -> &mut Self {
        // Simple format validation without regex dependency
        let is_valid = match pattern {
            "email" => self.is_valid_email(value),
            "url" => self.is_valid_url(value),
            "ipv4" => self.is_valid_ipv4(value),
            "hostname" => self.is_valid_hostname(value),
            _ => {
                // For unknown patterns, just check it's not empty
                !value.is_empty()
            }
        };
        
        if !is_valid {
            self.errors.push(ValidationError::InvalidFormat {
                field: self.full_field_name(field_name),
                expected: pattern.to_string(),
                actual: value.to_string(),
            });
        }
        
        self
    }
    
    /// Validate a collection of items
    pub fn each<T, F>(&mut self, field_name: &str, items: &[T], mut validator: F) -> &mut Self
    where
        F: FnMut(&mut ConfigValidator, usize, &T),
    {
        for (index, item) in items.iter().enumerate() {
            let mut item_validator = self.for_field(&format!("{}[{}]", field_name, index));
            validator(&mut item_validator, index, item);
            self.merge(item_validator);
        }
        self
    }
    
    /// Merge errors from another validator
    pub fn merge(&mut self, other: ConfigValidator) {
        self.errors.extend(other.errors);
    }
    
    /// Get validation result
    pub fn result(self) -> ValidationResult {
        if self.errors.is_empty() {
            Ok(())
        } else {
            // Return the first error (could be enhanced to return all errors)
            Err(self.errors.into_iter().next().unwrap())
        }
    }
    
    /// Get all validation errors
    pub fn all_errors(self) -> Vec<ValidationError> {
        self.errors
    }
    
    /// Get full field name with prefix
    fn full_field_name(&self, field_name: &str) -> String {
        if self.field_prefix.is_empty() {
            field_name.to_string()
        } else {
            format!("{}.{}", self.field_prefix, field_name)
        }
    }
    
    /// Simple email validation
    fn is_valid_email(&self, email: &str) -> bool {
        email.contains('@') && email.contains('.') && !email.starts_with('@') && !email.ends_with('@')
    }
    
    /// Simple URL validation
    fn is_valid_url(&self, url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ftp://")
    }
    
    /// Simple IPv4 validation
    fn is_valid_ipv4(&self, ip: &str) -> bool {
        let parts: Vec<&str> = ip.split('.').collect();
        if parts.len() != 4 {
            return false;
        }
        
        parts.iter().all(|part| {
            part.parse::<u8>().is_ok()
        })
    }
    
    /// Simple hostname validation
    fn is_valid_hostname(&self, hostname: &str) -> bool {
        !hostname.is_empty() &&
        hostname.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') &&
        !hostname.starts_with('-') &&
        !hostname.ends_with('-')
    }
}

impl Default for ConfigValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Validation rule trait for custom validation logic
pub trait ValidationRule<T> {
    fn validate(&self, value: &T) -> ValidationResult;
}

/// Built-in validation rules
pub mod rules {
    use super::*;
    
    /// Rule that validates a value is within a numeric range
    pub struct Range<T> {
        pub min: Option<T>,
        pub max: Option<T>,
        pub field_name: String,
    }
    
    impl<T> Range<T> 
    where
        T: PartialOrd + Copy + Into<f64>,
    {
        pub fn new(field_name: &str) -> Self {
            Self {
                min: None,
                max: None,
                field_name: field_name.to_string(),
            }
        }
        
        pub fn min(mut self, min: T) -> Self {
            self.min = Some(min);
            self
        }
        
        pub fn max(mut self, max: T) -> Self {
            self.max = Some(max);
            self
        }
    }
    
    impl<T> ValidationRule<T> for Range<T>
    where
        T: PartialOrd + Copy + Into<f64>,
    {
        fn validate(&self, value: &T) -> ValidationResult {
            let mut out_of_range = false;
            
            if let Some(min_val) = self.min {
                if *value < min_val {
                    out_of_range = true;
                }
            }
            
            if let Some(max_val) = self.max {
                if *value > max_val {
                    out_of_range = true;
                }
            }
            
            if out_of_range {
                Err(ValidationError::OutOfRange {
                    field: self.field_name.clone(),
                    min: self.min.map(|v| v.into()),
                    max: self.max.map(|v| v.into()),
                    actual: (*value).into(),
                })
            } else {
                Ok(())
            }
        }
    }
    
    /// Rule that validates a string matches a pattern
    pub struct Pattern {
        pub pattern: String,
        pub field_name: String,
    }
    
    impl Pattern {
        pub fn new(field_name: &str, pattern: &str) -> Self {
            Self {
                pattern: pattern.to_string(),
                field_name: field_name.to_string(),
            }
        }
    }
    
    impl ValidationRule<String> for Pattern {
        fn validate(&self, value: &String) -> ValidationResult {
            let validator = ConfigValidator::new();
            let is_valid = match self.pattern.as_str() {
                "email" => validator.is_valid_email(value),
                "url" => validator.is_valid_url(value),
                "ipv4" => validator.is_valid_ipv4(value),
                "hostname" => validator.is_valid_hostname(value),
                _ => !value.is_empty(),
            };
            
            if is_valid {
                Ok(())
            } else {
                Err(ValidationError::InvalidFormat {
                    field: self.field_name.clone(),
                    expected: self.pattern.clone(),
                    actual: value.clone(),
                })
            }
        }
    }
}