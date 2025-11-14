//! Example demonstrating lifecycle management in AuraEffectSystem
//!
//! This example shows how to:
//! - Initialize an effect system with lifecycle management
//! - Register custom lifecycle-aware components
//! - Monitor system health
//! - Perform graceful shutdown

use aura_core::{AuraResult, DeviceId};
use aura_protocol::effects::{
    AuraEffectSystemBuilder, EffectSystemState,
    lifecycle::{LifecycleAware, HealthStatus},
};
use aura_protocol::ExecutionMode;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, error};

/// Example lifecycle-aware component
struct DatabaseConnection {
    name: String,
    connected: Arc<AtomicBool>,
}

impl DatabaseConnection {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl LifecycleAware for DatabaseConnection {
    async fn on_initialize(&self) -> AuraResult<()> {
        info!("Initializing database connection: {}", self.name);
        
        // Simulate connection setup
        sleep(Duration::from_millis(100)).await;
        self.connected.store(true, Ordering::Relaxed);
        
        info!("Database connection {} established", self.name);
        Ok(())
    }

    async fn on_shutdown(&self) -> AuraResult<()> {
        info!("Shutting down database connection: {}", self.name);
        
        // Simulate connection cleanup
        sleep(Duration::from_millis(50)).await;
        self.connected.store(false, Ordering::Relaxed);
        
        info!("Database connection {} closed", self.name);
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        if self.is_connected() {
            HealthStatus::healthy()
                .with_metadata(serde_json::json!({
                    "connection": self.name,
                    "status": "connected"
                }))
        } else {
            HealthStatus::unhealthy("Database connection lost")
                .with_metadata(serde_json::json!({
                    "connection": self.name,
                    "status": "disconnected"
                }))
        }
    }
}

/// Example service that depends on the database
struct DataService {
    name: String,
    db: Arc<DatabaseConnection>,
    running: Arc<AtomicBool>,
}

impl DataService {
    fn new(name: impl Into<String>, db: Arc<DatabaseConnection>) -> Self {
        Self {
            name: name.into(),
            db,
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl LifecycleAware for DataService {
    async fn on_initialize(&self) -> AuraResult<()> {
        info!("Initializing data service: {}", self.name);
        
        // Check if database is connected
        if !self.db.is_connected() {
            return Err(aura_core::AuraError::invalid(
                "Cannot initialize data service: database not connected"
            ));
        }
        
        // Simulate service startup
        sleep(Duration::from_millis(50)).await;
        self.running.store(true, Ordering::Relaxed);
        
        info!("Data service {} started", self.name);
        Ok(())
    }

    async fn on_shutdown(&self) -> AuraResult<()> {
        info!("Shutting down data service: {}", self.name);
        
        // Simulate service cleanup
        sleep(Duration::from_millis(25)).await;
        self.running.store(false, Ordering::Relaxed);
        
        info!("Data service {} stopped", self.name);
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let is_running = self.running.load(Ordering::Relaxed);
        let db_connected = self.db.is_connected();
        
        if is_running && db_connected {
            HealthStatus::healthy()
                .with_metadata(serde_json::json!({
                    "service": self.name,
                    "status": "running",
                    "database": "connected"
                }))
        } else {
            let message = if !is_running {
                "Service not running"
            } else {
                "Database dependency unavailable"
            };
            
            HealthStatus::unhealthy(message)
                .with_metadata(serde_json::json!({
                    "service": self.name,
                    "running": is_running,
                    "database_connected": db_connected
                }))
        }
    }
}

#[tokio::main]
async fn main() -> AuraResult<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_timestamp(true)
        .init();

    println!("=== Aura Effect System Lifecycle Management Example ===\n");

    // Create the effect system
    let device_id = DeviceId::new();
    let effect_system = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_execution_mode(ExecutionMode::Testing)
        .build()
        .await?;

    println!("Effect system created with device ID: {:?}\n", device_id);

    // Check initial state
    println!("Initial lifecycle state: {:?}", effect_system.lifecycle_state());
    assert_eq!(effect_system.lifecycle_state(), EffectSystemState::Uninitialized);

    // Create and register components
    let db = Arc::new(DatabaseConnection::new("main_db"));
    let data_service = Arc::new(DataService::new("user_data_service", db.clone()));

    println!("\nRegistering lifecycle-aware components...");
    effect_system.register_lifecycle_component(
        "database",
        Box::new(db.clone()) as Box<dyn LifecycleAware>
    ).await;

    effect_system.register_lifecycle_component(
        "data_service",
        Box::new(data_service.clone()) as Box<dyn LifecycleAware>
    ).await;

    // Initialize the system
    println!("\nInitializing effect system...");
    match effect_system.initialize_lifecycle().await {
        Ok(()) => {
            println!("✓ Effect system initialized successfully");
            println!("  Current state: {:?}", effect_system.lifecycle_state());
            println!("  System ready: {}", effect_system.is_ready());
        }
        Err(e) => {
            error!("Failed to initialize effect system: {}", e);
            return Err(e);
        }
    }

    // Perform health check
    println!("\nPerforming system health check...");
    let health_report = effect_system.health_check().await;
    
    println!("System Health Report:");
    println!("  Overall health: {}", if health_report.is_healthy { "✓ Healthy" } else { "✗ Unhealthy" });
    println!("  Uptime: {:?}", health_report.uptime);
    println!("  State: {:?}", health_report.state);
    println!("  Component health:");
    
    for (component_name, health_status) in &health_report.component_health {
        let status_icon = if health_status.is_healthy { "✓" } else { "✗" };
        println!("    {} {}: {}", 
            status_icon,
            component_name,
            health_status.message.as_deref().unwrap_or("Healthy")
        );
        
        if let Some(metadata) = &health_status.metadata {
            println!("      Metadata: {}", serde_json::to_string_pretty(metadata)?);
        }
    }

    // Simulate some work
    println!("\nSimulating system operations for 2 seconds...");
    sleep(Duration::from_secs(2)).await;

    // Check uptime
    println!("System uptime: {:?}", effect_system.uptime());

    // Shutdown the system
    println!("\nInitiating graceful shutdown...");
    match effect_system.shutdown_lifecycle().await {
        Ok(()) => {
            println!("✓ Effect system shut down successfully");
            println!("  Final state: {:?}", effect_system.lifecycle_state());
        }
        Err(e) => {
            error!("Error during shutdown: {}", e);
            // Continue anyway - best effort shutdown
        }
    }

    // Verify components are shut down
    println!("\nVerifying component states:");
    println!("  Database connected: {}", db.is_connected());
    println!("  Data service running: {}", data_service.running.load(Ordering::Relaxed));

    // Try to use the system after shutdown (should fail)
    println!("\nAttempting to use system after shutdown...");
    match effect_system.ensure_ready() {
        Ok(()) => println!("  Unexpected: System still ready!"),
        Err(e) => println!("  Expected error: {}", e),
    }

    println!("\n=== Example completed successfully ===");
    Ok(())
}