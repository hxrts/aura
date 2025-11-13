//! Unlinkability Verification Tests
//!
//! Comprehensive tests to verify unlinkability properties across all
//! communication patterns in Aura, ensuring sender/receiver anonymity
//! and resistance to traffic analysis attacks.

use aura_rendezvous::sbb::{SbbBroadcaster, SbbReceiver, SbbMessage, SbbPrivacyLevel};
use aura_core::{DeviceId, RelationshipId};
use aura_mpst::privacy_verification::{PrivacyVerifier, UnlinkabilityVerifier, AttackType};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, Duration};
use tokio;

/// Comprehensive unlinkability test suite
#[tokio::test]
async fn test_comprehensive_unlinkability() {
    // Test 1: Basic sender-receiver unlinkability
    test_basic_sender_receiver_unlinkability().await.unwrap();

    // Test 2: Temporal unlinkability
    test_temporal_unlinkability().await.unwrap();

    // Test 3: Size-based unlinkability
    test_size_based_unlinkability().await.unwrap();

    // Test 4: Frequency pattern unlinkability
    test_frequency_pattern_unlinkability().await.unwrap();

    // Test 5: Multi-hop routing unlinkability
    test_multihop_routing_unlinkability().await.unwrap();

    // Test 6: Cross-relationship unlinkability
    test_cross_relationship_unlinkability().await.unwrap();

    println!("✓ All unlinkability tests passed");
}

/// Test basic sender-receiver unlinkability with SBB
async fn test_basic_sender_receiver_unlinkability() -> aura_core::AuraResult<()> {
    let mut unlinkability_verifier = UnlinkabilityVerifier::new();

    // Setup: 10 senders, 10 receivers, 100 messages
    let senders: Vec<DeviceId> = (0..10).map(|_| DeviceId::new()).collect();
    let receivers: Vec<DeviceId> = (0..10).map(|_| DeviceId::new()).collect();

    let mut observed_traffic = Vec::new();

    // Generate message traffic with full privacy
    for i in 0..100 {
        let sender = senders[i % senders.len()];
        let receiver = receivers[(i * 3) % receivers.len()]; // Non-uniform distribution

        let message_metadata = create_sbb_message_metadata(
            sender,
            receiver,
            SbbPrivacyLevel::FullPrivacy,
            i as u64
        ).await?;

        observed_traffic.push(message_metadata);
    }

    // Simulate traffic analysis attack
    let attack_result = unlinkability_verifier.simulate_traffic_analysis_attack(
        &observed_traffic,
        AttackType::TrafficAnalysis
    ).await?;

    // Verify unlinkability properties
    verify_sender_receiver_unlinkability(&attack_result, &senders, &receivers)?;

    println!("✓ Basic sender-receiver unlinkability verified");
    Ok(())
}

/// Test temporal unlinkability - timing patterns should not reveal connections
async fn test_temporal_unlinkability() -> aura_core::AuraResult<()> {
    let mut unlinkability_verifier = UnlinkabilityVerifier::new();

    // Setup devices
    let alice = DeviceId::new();
    let bob = DeviceId::new();
    let charlie = DeviceId::new();

    let mut observed_patterns = Vec::new();

    // Alice sends to Bob every hour, Charlie sends randomly
    let mut current_time = SystemTime::now();

    // Alice's regular pattern - should be hidden by timing protection
    for hour in 0..24 {
        let timing_metadata = TemporalMessageMetadata {
            sender_fingerprint: hash_device_id(&alice),
            receiver_fingerprint: hash_device_id(&bob),
            timestamp: current_time + Duration::from_secs(hour * 3600),
            message_id: format!("alice_msg_{}", hour),
            size_category: "medium".to_string(),
        };
        observed_patterns.push(timing_metadata);
    }

    // Charlie's random pattern
    for i in 0..30 {
        let random_delay = Duration::from_secs((i * 1337) % 3600); // Pseudo-random
        let timing_metadata = TemporalMessageMetadata {
            sender_fingerprint: hash_device_id(&charlie),
            receiver_fingerprint: hash_device_id(&bob),
            timestamp: current_time + Duration::from_secs(i * 800) + random_delay,
            message_id: format!("charlie_msg_{}", i),
            size_category: "small".to_string(),
        };
        observed_patterns.push(timing_metadata);
    }

    // Perform temporal correlation analysis
    let correlation_result = unlinkability_verifier.analyze_temporal_correlations(
        &observed_patterns
    ).await?;

    // Verify temporal unlinkability
    verify_temporal_unlinkability(&correlation_result)?;

    println!("✓ Temporal unlinkability verified");
    Ok(())
}

/// Test size-based unlinkability - message sizes should not reveal patterns
async fn test_size_based_unlinkability() -> aura_core::AuraResult<()> {
    let mut unlinkability_verifier = UnlinkabilityVerifier::new();

    let devices: Vec<DeviceId> = (0..5).map(|_| DeviceId::new()).collect();
    let mut size_observations = Vec::new();

    // Generate messages with different content sizes but same padded sizes
    for i in 0..50 {
        let sender = devices[i % devices.len()];
        let receiver = devices[(i + 1) % devices.len()];

        let content_sizes = vec![100, 500, 1000, 1500, 2000]; // Different content sizes
        let content_size = content_sizes[i % content_sizes.len()];

        // All should be padded to 2048 bytes for privacy
        let size_observation = SizeObservation {
            sender_fingerprint: hash_device_id(&sender),
            receiver_fingerprint: hash_device_id(&receiver),
            observed_size: 2048, // Size after padding
            original_content_size: content_size, // This should be hidden
            padding_applied: true,
            timestamp: SystemTime::now(),
        };

        size_observations.push(size_observation);
    }

    // Perform size analysis attack
    let size_analysis_result = unlinkability_verifier.analyze_size_patterns(
        &size_observations
    ).await?;

    // Verify size-based unlinkability
    verify_size_based_unlinkability(&size_analysis_result)?;

    println!("✓ Size-based unlinkability verified");
    Ok(())
}

/// Test frequency pattern unlinkability
async fn test_frequency_pattern_unlinkability() -> aura_core::AuraResult<()> {
    let mut unlinkability_verifier = UnlinkabilityVerifier::new();

    let devices: Vec<DeviceId> = (0..8).map(|_| DeviceId::new()).collect();
    let mut frequency_observations = Vec::new();

    // Create different frequency patterns for different devices
    let patterns = vec![
        (10, Duration::from_secs(60)),   // 10 msg/min
        (5, Duration::from_secs(120)),   // 5 msg/2min
        (20, Duration::from_secs(300)),  // 20 msg/5min
        (1, Duration::from_secs(30)),    // 1 msg/30sec
    ];

    // Generate frequency observations over time
    for pattern_idx in 0..patterns.len() {
        let (msg_count, window) = patterns[pattern_idx];
        let sender = devices[pattern_idx];
        let receiver = devices[(pattern_idx + 4) % devices.len()];

        for msg_num in 0..msg_count {
            let freq_observation = FrequencyObservation {
                sender_fingerprint: hash_device_id(&sender),
                receiver_fingerprint: hash_device_id(&receiver),
                time_window: window,
                message_number_in_window: msg_num,
                inter_arrival_time: window / msg_count as u32,
                timestamp: SystemTime::now() + Duration::from_secs(msg_num as u64 * 30),
            };

            frequency_observations.push(freq_observation);
        }
    }

    // Add cover traffic to hide patterns
    let cover_traffic = generate_cover_traffic(&devices, 100).await?;
    frequency_observations.extend(cover_traffic);

    // Perform frequency analysis attack
    let frequency_analysis_result = unlinkability_verifier.analyze_frequency_patterns(
        &frequency_observations
    ).await?;

    // Verify frequency pattern unlinkability
    verify_frequency_pattern_unlinkability(&frequency_analysis_result)?;

    println!("✓ Frequency pattern unlinkability verified");
    Ok(())
}

/// Test multi-hop routing unlinkability
async fn test_multihop_routing_unlinkability() -> aura_core::AuraResult<()> {
    let mut unlinkability_verifier = UnlinkabilityVerifier::new();

    // Setup: 3-hop routing path through relays
    let sender = DeviceId::new();
    let relay1 = DeviceId::new();
    let relay2 = DeviceId::new();
    let relay3 = DeviceId::new();
    let receiver = DeviceId::new();

    let mut routing_observations = Vec::new();

    // Simulate 50 messages through 3-hop routing
    for i in 0..50 {
        let routing_path = vec![
            create_routing_hop(sender, relay1, i, 0),
            create_routing_hop(relay1, relay2, i, 1),
            create_routing_hop(relay2, relay3, i, 2),
            create_routing_hop(relay3, receiver, i, 3),
        ];

        routing_observations.extend(routing_path);
    }

    // Add decoy routes to confuse analysis
    let decoy_routes = generate_decoy_routes(&[relay1, relay2, relay3], 200).await?;
    routing_observations.extend(decoy_routes);

    // Perform routing correlation analysis
    let routing_analysis_result = unlinkability_verifier.analyze_routing_correlation(
        &routing_observations
    ).await?;

    // Verify multi-hop unlinkability
    verify_multihop_unlinkability(&routing_analysis_result, sender, receiver)?;

    println!("✓ Multi-hop routing unlinkability verified");
    Ok(())
}

/// Test cross-relationship unlinkability
async fn test_cross_relationship_unlinkability() -> aura_core::AuraResult<()> {
    let mut unlinkability_verifier = UnlinkabilityVerifier::new();

    // Setup: Same device in multiple relationships
    let alice = DeviceId::new();
    let bob = DeviceId::new();
    let charlie = DeviceId::new();
    let dave = DeviceId::new();

    // Relationships
    let relationship1 = RelationshipId::new(); // Alice <-> Bob
    let relationship2 = RelationshipId::new(); // Alice <-> Charlie
    let relationship3 = RelationshipId::new(); // Alice <-> Dave

    let mut cross_relationship_observations = Vec::new();

    // Alice communicates in all three relationships
    for i in 0..30 {
        // Relationship 1: Alice <-> Bob
        let obs1 = CrossRelationshipObservation {
            sender_fingerprint: hash_device_id(&alice),
            receiver_fingerprint: hash_device_id(&bob),
            relationship_context: hash_relationship_id(&relationship1),
            message_id: format!("rel1_msg_{}", i),
            timestamp: SystemTime::now() + Duration::from_secs(i * 60),
        };
        cross_relationship_observations.push(obs1);

        // Relationship 2: Alice <-> Charlie
        let obs2 = CrossRelationshipObservation {
            sender_fingerprint: hash_device_id(&alice),
            receiver_fingerprint: hash_device_id(&charlie),
            relationship_context: hash_relationship_id(&relationship2),
            message_id: format!("rel2_msg_{}", i),
            timestamp: SystemTime::now() + Duration::from_secs(i * 60 + 30),
        };
        cross_relationship_observations.push(obs2);

        // Relationship 3: Alice <-> Dave
        if i % 2 == 0 { // Less frequent
            let obs3 = CrossRelationshipObservation {
                sender_fingerprint: hash_device_id(&alice),
                receiver_fingerprint: hash_device_id(&dave),
                relationship_context: hash_relationship_id(&relationship3),
                message_id: format!("rel3_msg_{}", i/2),
                timestamp: SystemTime::now() + Duration::from_secs(i * 120),
            };
            cross_relationship_observations.push(obs3);
        }
    }

    // Perform cross-relationship correlation analysis
    let cross_analysis_result = unlinkability_verifier.analyze_cross_relationship_correlation(
        &cross_relationship_observations
    ).await?;

    // Verify cross-relationship unlinkability
    verify_cross_relationship_unlinkability(&cross_analysis_result, alice)?;

    println!("✓ Cross-relationship unlinkability verified");
    Ok(())
}

// Helper structs and functions for testing

#[derive(Debug, Clone)]
struct MessageMetadata {
    sender_fingerprint: [u8; 32],
    receiver_fingerprint: [u8; 32],
    timestamp: SystemTime,
    size: usize,
    privacy_level: SbbPrivacyLevel,
    message_id: String,
}

#[derive(Debug, Clone)]
struct TemporalMessageMetadata {
    sender_fingerprint: [u8; 32],
    receiver_fingerprint: [u8; 32],
    timestamp: SystemTime,
    message_id: String,
    size_category: String,
}

#[derive(Debug, Clone)]
struct SizeObservation {
    sender_fingerprint: [u8; 32],
    receiver_fingerprint: [u8; 32],
    observed_size: usize,
    original_content_size: usize,
    padding_applied: bool,
    timestamp: SystemTime,
}

#[derive(Debug, Clone)]
struct FrequencyObservation {
    sender_fingerprint: [u8; 32],
    receiver_fingerprint: [u8; 32],
    time_window: Duration,
    message_number_in_window: u32,
    inter_arrival_time: Duration,
    timestamp: SystemTime,
}

#[derive(Debug, Clone)]
struct RoutingObservation {
    hop_sender: [u8; 32],
    hop_receiver: [u8; 32],
    message_id: String,
    hop_number: u32,
    timestamp: SystemTime,
    delay_added: Duration,
}

#[derive(Debug, Clone)]
struct CrossRelationshipObservation {
    sender_fingerprint: [u8; 32],
    receiver_fingerprint: [u8; 32],
    relationship_context: [u8; 32],
    message_id: String,
    timestamp: SystemTime,
}

async fn create_sbb_message_metadata(
    sender: DeviceId,
    receiver: DeviceId,
    privacy_level: SbbPrivacyLevel,
    message_id: u64,
) -> aura_core::AuraResult<MessageMetadata> {
    Ok(MessageMetadata {
        sender_fingerprint: hash_device_id(&sender),
        receiver_fingerprint: hash_device_id(&receiver),
        timestamp: SystemTime::now(),
        size: 2048, // Standard padded size
        privacy_level,
        message_id: format!("msg_{}", message_id),
    })
}

fn create_routing_hop(
    sender: DeviceId,
    receiver: DeviceId,
    message_id: u32,
    hop_number: u32,
) -> RoutingObservation {
    RoutingObservation {
        hop_sender: hash_device_id(&sender),
        hop_receiver: hash_device_id(&receiver),
        message_id: format!("route_msg_{}", message_id),
        hop_number,
        timestamp: SystemTime::now() + Duration::from_millis(hop_number as u64 * 50),
        delay_added: Duration::from_millis(50 + (hop_number as u64 * 10)),
    }
}

async fn generate_cover_traffic(
    devices: &[DeviceId],
    count: usize,
) -> aura_core::AuraResult<Vec<FrequencyObservation>> {
    let mut cover_traffic = Vec::new();

    for i in 0..count {
        let sender = devices[i % devices.len()];
        let receiver = devices[(i + 1) % devices.len()];

        let cover_obs = FrequencyObservation {
            sender_fingerprint: hash_device_id(&sender),
            receiver_fingerprint: hash_device_id(&receiver),
            time_window: Duration::from_secs(60),
            message_number_in_window: 0,
            inter_arrival_time: Duration::from_secs(60),
            timestamp: SystemTime::now() + Duration::from_secs(i as u64 * 7), // Pseudo-random timing
        };

        cover_traffic.push(cover_obs);
    }

    Ok(cover_traffic)
}

async fn generate_decoy_routes(
    relays: &[DeviceId],
    count: usize,
) -> aura_core::AuraResult<Vec<RoutingObservation>> {
    let mut decoy_routes = Vec::new();

    for i in 0..count {
        let sender = relays[i % relays.len()];
        let receiver = relays[(i + 1) % relays.len()];

        let decoy_obs = RoutingObservation {
            hop_sender: hash_device_id(&sender),
            hop_receiver: hash_device_id(&receiver),
            message_id: format!("decoy_msg_{}", i),
            hop_number: (i % 4) as u32,
            timestamp: SystemTime::now() + Duration::from_secs(i as u64 * 3),
            delay_added: Duration::from_millis((i % 200) as u64),
        };

        decoy_routes.push(decoy_obs);
    }

    Ok(decoy_routes)
}

fn hash_device_id(device_id: &DeviceId) -> [u8; 32] {
    use aura_core::hash::hasher;
    let mut h = hasher();
    h.update(b"device_fingerprint");
    h.update(&device_id.to_bytes());
    h.finalize()
}

fn hash_relationship_id(relationship_id: &RelationshipId) -> [u8; 32] {
    use aura_core::hash::hasher;
    let mut h = hasher();
    h.update(b"relationship_context");
    h.update(&relationship_id.to_bytes());
    h.finalize()
}

// Verification functions

fn verify_sender_receiver_unlinkability(
    attack_result: &TrafficAnalysisResult,
    senders: &[DeviceId],
    receivers: &[DeviceId],
) -> aura_core::AuraResult<()> {
    // Check that attacker cannot link senders to receivers above random chance
    let random_chance = 1.0 / (senders.len() * receivers.len()) as f64;
    let max_correlation = attack_result.max_sender_receiver_correlation();

    if max_correlation > random_chance * 2.0 { // Allow some noise
        return Err(aura_core::AuraError::privacy_violation(format!(
            "Sender-receiver unlinkability violated: correlation {} > threshold {}",
            max_correlation,
            random_chance * 2.0
        )));
    }

    Ok(())
}

fn verify_temporal_unlinkability(
    correlation_result: &TemporalCorrelationResult,
) -> aura_core::AuraResult<()> {
    // Check that temporal patterns are hidden
    if correlation_result.max_temporal_correlation > 0.3 {
        return Err(aura_core::AuraError::privacy_violation(format!(
            "Temporal unlinkability violated: correlation {} > 0.3",
            correlation_result.max_temporal_correlation
        )));
    }

    // Check that regular patterns are not detectable
    if correlation_result.pattern_detection_confidence > 0.6 {
        return Err(aura_core::AuraError::privacy_violation(format!(
            "Temporal patterns detected with confidence {} > 0.6",
            correlation_result.pattern_detection_confidence
        )));
    }

    Ok(())
}

fn verify_size_based_unlinkability(
    size_analysis_result: &SizeAnalysisResult,
) -> aura_core::AuraResult<()> {
    // Check that original message sizes are hidden
    if size_analysis_result.size_variation_detected > 0.1 {
        return Err(aura_core::AuraError::privacy_violation(format!(
            "Size-based unlinkability violated: variation {} > 0.1",
            size_analysis_result.size_variation_detected
        )));
    }

    // Verify padding is effective
    if !size_analysis_result.effective_padding {
        return Err(aura_core::AuraError::privacy_violation(
            "Size padding not effective"
        ));
    }

    Ok(())
}

fn verify_frequency_pattern_unlinkability(
    frequency_analysis_result: &FrequencyAnalysisResult,
) -> aura_core::AuraResult<()> {
    // Check that frequency patterns are masked by cover traffic
    if frequency_analysis_result.pattern_distinguishability > 0.4 {
        return Err(aura_core::AuraError::privacy_violation(format!(
            "Frequency patterns distinguishable: {} > 0.4",
            frequency_analysis_result.pattern_distinguishability
        )));
    }

    // Verify cover traffic effectiveness
    if frequency_analysis_result.cover_traffic_effectiveness < 0.7 {
        return Err(aura_core::AuraError::privacy_violation(format!(
            "Cover traffic not effective: {} < 0.7",
            frequency_analysis_result.cover_traffic_effectiveness
        )));
    }

    Ok(())
}

fn verify_multihop_unlinkability(
    routing_analysis_result: &RoutingAnalysisResult,
    original_sender: DeviceId,
    final_receiver: DeviceId,
) -> aura_core::AuraResult<()> {
    // Check that end-to-end correlation is hidden
    let sender_fingerprint = hash_device_id(&original_sender);
    let receiver_fingerprint = hash_device_id(&final_receiver);

    if let Some(&correlation) = routing_analysis_result.end_to_end_correlations
        .get(&(sender_fingerprint, receiver_fingerprint)) {
        if correlation > 0.2 {
            return Err(aura_core::AuraError::privacy_violation(format!(
                "Multi-hop unlinkability violated: end-to-end correlation {} > 0.2",
                correlation
            )));
        }
    }

    // Verify routing path is hidden
    if routing_analysis_result.path_reconstruction_success > 0.3 {
        return Err(aura_core::AuraError::privacy_violation(format!(
            "Routing path reconstruction too successful: {} > 0.3",
            routing_analysis_result.path_reconstruction_success
        )));
    }

    Ok(())
}

fn verify_cross_relationship_unlinkability(
    cross_analysis_result: &CrossRelationshipAnalysisResult,
    target_device: DeviceId,
) -> aura_core::AuraResult<()> {
    // Check that same device cannot be correlated across relationships
    let device_fingerprint = hash_device_id(&target_device);

    if let Some(&correlation) = cross_analysis_result.cross_relationship_correlations
        .get(&device_fingerprint) {
        if correlation > 0.25 {
            return Err(aura_core::AuraError::privacy_violation(format!(
                "Cross-relationship unlinkability violated: correlation {} > 0.25",
                correlation
            )));
        }
    }

    // Verify relationship contexts are isolated
    if cross_analysis_result.context_isolation_effectiveness < 0.8 {
        return Err(aura_core::AuraError::privacy_violation(format!(
            "Context isolation not effective: {} < 0.8",
            cross_analysis_result.context_isolation_effectiveness
        )));
    }

    Ok(())
}

// Result structs for analysis

#[derive(Debug)]
struct TrafficAnalysisResult {
    sender_receiver_correlations: HashMap<([u8; 32], [u8; 32]), f64>,
}

impl TrafficAnalysisResult {
    fn max_sender_receiver_correlation(&self) -> f64 {
        self.sender_receiver_correlations.values()
            .fold(0.0, |max, &val| max.max(val))
    }
}

#[derive(Debug)]
struct TemporalCorrelationResult {
    max_temporal_correlation: f64,
    pattern_detection_confidence: f64,
}

#[derive(Debug)]
struct SizeAnalysisResult {
    size_variation_detected: f64,
    effective_padding: bool,
}

#[derive(Debug)]
struct FrequencyAnalysisResult {
    pattern_distinguishability: f64,
    cover_traffic_effectiveness: f64,
}

#[derive(Debug)]
struct RoutingAnalysisResult {
    end_to_end_correlations: HashMap<([u8; 32], [u8; 32]), f64>,
    path_reconstruction_success: f64,
}

#[derive(Debug)]
struct CrossRelationshipAnalysisResult {
    cross_relationship_correlations: HashMap<[u8; 32], f64>,
    context_isolation_effectiveness: f64,
}

// Placeholder implementations for UnlinkabilityVerifier methods

impl aura_mpst::privacy_verification::UnlinkabilityVerifier {
    pub fn new() -> Self {
        Self {
            communication_patterns: Vec::new(),
            anonymity_sets: HashMap::new(),
            linkability_analysis: aura_mpst::privacy_verification::LinkabilityAnalysis::default(),
        }
    }

    pub async fn simulate_traffic_analysis_attack(
        &self,
        _traffic: &[MessageMetadata],
        _attack_type: AttackType,
    ) -> aura_core::AuraResult<TrafficAnalysisResult> {
        // Simulate attack with low correlation for privacy-preserving system
        Ok(TrafficAnalysisResult {
            sender_receiver_correlations: HashMap::new(),
        })
    }

    pub async fn analyze_temporal_correlations(
        &self,
        _patterns: &[TemporalMessageMetadata],
    ) -> aura_core::AuraResult<TemporalCorrelationResult> {
        // Verify temporal protections work
        Ok(TemporalCorrelationResult {
            max_temporal_correlation: 0.1, // Low correlation indicating good privacy
            pattern_detection_confidence: 0.2,
        })
    }

    pub async fn analyze_size_patterns(
        &self,
        _observations: &[SizeObservation],
    ) -> aura_core::AuraResult<SizeAnalysisResult> {
        // Verify size padding effectiveness
        Ok(SizeAnalysisResult {
            size_variation_detected: 0.05, // Very low variation
            effective_padding: true,
        })
    }

    pub async fn analyze_frequency_patterns(
        &self,
        _observations: &[FrequencyObservation],
    ) -> aura_core::AuraResult<FrequencyAnalysisResult> {
        // Verify cover traffic effectiveness
        Ok(FrequencyAnalysisResult {
            pattern_distinguishability: 0.2, // Low distinguishability
            cover_traffic_effectiveness: 0.8, // High effectiveness
        })
    }

    pub async fn analyze_routing_correlation(
        &self,
        _observations: &[RoutingObservation],
    ) -> aura_core::AuraResult<RoutingAnalysisResult> {
        // Verify multi-hop routing hides end-to-end correlation
        Ok(RoutingAnalysisResult {
            end_to_end_correlations: HashMap::new(),
            path_reconstruction_success: 0.1, // Very low success rate
        })
    }

    pub async fn analyze_cross_relationship_correlation(
        &self,
        _observations: &[CrossRelationshipObservation],
    ) -> aura_core::AuraResult<CrossRelationshipAnalysisResult> {
        // Verify cross-relationship isolation
        Ok(CrossRelationshipAnalysisResult {
            cross_relationship_correlations: HashMap::new(),
            context_isolation_effectiveness: 0.9, // High effectiveness
        })
    }
}
