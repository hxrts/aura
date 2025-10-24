// Invitation method trait interface
//
// This module provides a trait-based interface for different invitation delivery methods.
// Currently supports QR codes, but can be extended for other methods like:
// - Deep links via messaging apps
// - NFC tags
// - Email invitations
// - SMS with verification codes

use crate::Result;
use serde::{Deserialize, Serialize};

/// Trait for different invitation delivery methods
pub trait InvitationMethod: Send + Sync {
    /// Generate the invitation representation (e.g., QR code SVG, formatted message)
    fn generate(&self, invitation_data: &InvitationData) -> Result<InvitationOutput>;
    
    /// Get the method type identifier
    fn method_type(&self) -> InvitationMethodType;
    
    /// Check if this method is available in the current environment
    fn is_available(&self) -> bool;
}

/// Data contained in an invitation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationData {
    /// Deep link URI for the invitation
    pub deep_link: String,
    /// Verification code for manual entry
    pub verification_code: String,
    /// Optional message from inviter
    pub message: Option<String>,
    /// Invitation metadata for display
    pub metadata: InvitationMetadata,
}

/// Metadata about the invitation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationMetadata {
    /// Inviter's display name (if available)
    pub inviter_name: Option<String>,
    /// Role being offered
    pub role: String,
    /// Expiration time as human-readable string
    pub expires_in: String,
}

/// Output format for different invitation methods
#[derive(Debug, Clone)]
pub enum InvitationOutput {
    /// SVG data for visual display (QR codes, etc.)
    Svg(String),
    /// Plain text for copying (SMS, email, etc.)
    Text(String),
    /// HTML for rich formatting
    Html(String),
    /// Binary data with mime type (for future use)
    Binary { data: Vec<u8>, mime_type: String },
}

/// Supported invitation method types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvitationMethodType {
    QrCode,
    DeepLink,
    Sms,
    Email,
    Nfc,
}

/// QR Code invitation method implementation
#[cfg(feature = "qrcode")]
pub struct QrCodeInvitation {
    /// Minimum dimensions for the QR code
    pub min_size: u32,
    /// Dark color (hex format)
    pub dark_color: String,
    /// Light color (hex format)
    pub light_color: String,
}

#[cfg(feature = "qrcode")]
impl Default for QrCodeInvitation {
    fn default() -> Self {
        Self {
            min_size: 256,
            dark_color: "#000000".to_string(),
            light_color: "#FFFFFF".to_string(),
        }
    }
}

#[cfg(feature = "qrcode")]
impl InvitationMethod for QrCodeInvitation {
    fn generate(&self, invitation_data: &InvitationData) -> Result<InvitationOutput> {
        use qrcode::{render::svg, QrCode};
        
        let svg_string = QrCode::new(&invitation_data.deep_link)
            .map_err(|e| crate::AgentError::device_not_found(format!("QR generation failed: {}", e)))?
            .render::<svg::Color>()
            .min_dimensions(self.min_size, self.min_size)
            .dark_color(svg::Color(&self.dark_color))
            .light_color(svg::Color(&self.light_color))
            .build();
        
        Ok(InvitationOutput::Svg(svg_string))
    }
    
    fn method_type(&self) -> InvitationMethodType {
        InvitationMethodType::QrCode
    }
    
    fn is_available(&self) -> bool {
        true
    }
}

/// Deep link invitation method (for messaging apps)
pub struct DeepLinkInvitation {
    /// Template for the message
    pub message_template: String,
}

impl Default for DeepLinkInvitation {
    fn default() -> Self {
        Self {
            message_template: "You've been invited to become a guardian!\n\nClick here: {link}\n\nOr enter code: {code}".to_string(),
        }
    }
}

impl InvitationMethod for DeepLinkInvitation {
    fn generate(&self, invitation_data: &InvitationData) -> Result<InvitationOutput> {
        let message = self.message_template
            .replace("{link}", &invitation_data.deep_link)
            .replace("{code}", &invitation_data.verification_code);
        
        let full_message = if let Some(personal_msg) = &invitation_data.message {
            format!("{}\n\nPersonal message: {}", message, personal_msg)
        } else {
            message
        };
        
        Ok(InvitationOutput::Text(full_message))
    }
    
    fn method_type(&self) -> InvitationMethodType {
        InvitationMethodType::DeepLink
    }
    
    fn is_available(&self) -> bool {
        true
    }
}

/// Factory for creating invitation methods
pub struct InvitationMethodFactory;

impl InvitationMethodFactory {
    /// Create a QR code invitation method
    #[cfg(feature = "qrcode")]
    pub fn qr_code() -> Box<dyn InvitationMethod> {
        Box::new(QrCodeInvitation::default())
    }
    
    /// Create a deep link invitation method
    pub fn deep_link() -> Box<dyn InvitationMethod> {
        Box::new(DeepLinkInvitation::default())
    }
    
    /// Get all available invitation methods
    pub fn available_methods() -> Vec<InvitationMethodType> {
        let mut methods = vec![InvitationMethodType::DeepLink];
        
        #[cfg(feature = "qrcode")]
        methods.push(InvitationMethodType::QrCode);
        
        methods
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_deep_link_invitation() {
        let method = DeepLinkInvitation::default();
        let data = InvitationData {
            deep_link: "aura://guardian/invite?token=123".to_string(),
            verification_code: "123456".to_string(),
            message: Some("Please help me recover my account".to_string()),
            metadata: InvitationMetadata {
                inviter_name: Some("Alice".to_string()),
                role: "Recovery".to_string(),
                expires_in: "24 hours".to_string(),
            },
        };
        
        let output = method.generate(&data).unwrap();
        match output {
            InvitationOutput::Text(text) => {
                assert!(text.contains("aura://guardian/invite?token=123"));
                assert!(text.contains("123456"));
                assert!(text.contains("Please help me recover my account"));
            }
            _ => panic!("Expected text output"),
        }
    }
    
    #[cfg(feature = "qrcode")]
    #[test]
    fn test_qr_code_invitation() {
        let method = QrCodeInvitation::default();
        let data = InvitationData {
            deep_link: "aura://guardian/invite?token=123".to_string(),
            verification_code: "123456".to_string(),
            message: None,
            metadata: InvitationMetadata {
                inviter_name: None,
                role: "Recovery".to_string(),
                expires_in: "24 hours".to_string(),
            },
        };
        
        let output = method.generate(&data).unwrap();
        match output {
            InvitationOutput::Svg(svg) => {
                assert!(svg.contains("<svg"));
                assert!(svg.contains("</svg>"));
            }
            _ => panic!("Expected SVG output"),
        }
    }
}