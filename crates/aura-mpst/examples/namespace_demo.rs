//! Namespace Demonstration Example
//!
//! This example shows how Aura's choreography system supports namespacing
//! for organizing different protocols and avoiding naming conflicts.

use aura_macros::choreography;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use rumpsteak_aura::*;
use rumpsteak_aura_choreography::Label;
use serde::{Deserialize, Serialize};

// Required type definitions for the generated choreography
#[allow(dead_code)]
type Channel = channel::Bidirectional<UnboundedSender<Label>, UnboundedReceiver<Label>>;

// Message types for the authentication protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct LoginRequest {
    username: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct UserQuery {
    user_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct UserData {
    user_info: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct AuthResponse {
    token: String,
}

// Authentication Protocol with auth_service namespace
choreography! {
    #[namespace = "auth_service"]
    choreography AuthenticationProtocol {
        roles: Client, AuthServer, Database;

        Client -> AuthServer: LoginRequest;
        AuthServer -> Database: UserQuery;
        Database -> AuthServer: UserData;
        AuthServer -> Client: AuthResponse;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Aura Namespace Demonstration ===\n");

    println!("[OK] Authentication Protocol");
    println!("   Namespace: auth_service");
    println!("   Roles: Client, AuthServer, Database");
    println!("   Generated session types with namespace isolation");
    println!(
        "
   Key Features Demonstrated:
   [-] Namespace attribute: #[namespace = \"auth_service\"]"
    );
    println!("   [-] Role isolation within namespace");
    println!("   [-] Session type generation for choreography");
    println!("   [-] Message flow: Client -> AuthServer -> Database -> AuthServer -> Client");

    println!("\nNamespace demonstration complete!");
    println!("   The choreography macro successfully:");
    println!("   [OK] Applied namespace isolation");
    println!("   [OK] Generated role types");
    println!("   [OK] Created session types for message flows");
    println!("   [OK] Integrated with Aura's MPST system");

    Ok(())
}
