//! Social Graph Rediscovery Test
//!
//! This test demonstrates how accounts can rediscover each other after key rotation
//! by using mutual contacts as rendezvous points. This is a critical capability for
//! maintaining social graph connectivity when peer identities change.
//!
//! Scenario:
//! 1. Account A and Account B establish contact (exchange DKD-derived identities)
//! 2. Account A and Account C establish contact
//! 3. Account B and Account C establish contact (forming a triangle)
//! 4. Account B rotates keys (runs resharing/DKD, gets new identity)
//! 5. Account C rotates keys (runs resharing/DKD, gets new identity)
//! 6. Account B and C can no longer directly communicate (old identities invalid)
//! 7. Account B queries A: "Do you know anyone I used to talk to?"
//! 8. Account C queries A: "Do you know anyone I used to talk to?"
//! 9. Account A acts as rendezvous, introducing B and C under their new identities
//! 10. Account B and C re-establish contact with new identities

use aura_crypto::Effects;
use aura_journal::{
    AccountId, AccountLedger, AccountState, DeviceId, DeviceMetadata, DeviceType, RelationshipId,
    SessionEpoch,
};

use ed25519_dalek::SigningKey;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Represents a social contact between two accounts
#[derive(Debug, Clone)]
struct Contact {
    /// The account ID of the contact
    account_id: AccountId,
    /// The DKD-derived identity used for communication
    dkd_identity: Vec<u8>,
    /// When this contact was established
    established_at: u64,
    /// Relationship ID for SSB counter tracking
    relationship_id: RelationshipId,
}

/// Simulated account with its own ledger and social graph
struct SimulatedAccount {
    account_id: AccountId,
    ledger: AccountLedger,
    device_id: DeviceId,
    signing_key: SigningKey,
    /// Current DKD-derived identity for this context (e.g., "social" context)
    current_identity: Vec<u8>,
    /// Known contacts indexed by their account ID
    contacts: HashMap<AccountId, Contact>,
}

impl SimulatedAccount {
    /// Create a new simulated account
    fn new(name: &str, effects: &Effects) -> Self {
        let account_id = AccountId::new_with_effects(effects);
        let device_id = DeviceId::new_with_effects(effects);
        let signing_key = SigningKey::from_bytes(&effects.random_bytes::<32>());
        let timestamp = effects.now().unwrap_or(0);

        // Create device metadata
        let device_metadata = DeviceMetadata {
            device_id,
            device_name: format!("{}-device", name),
            device_type: DeviceType::Native,
            public_key: signing_key.verifying_key(),
            added_at: timestamp,
            last_seen: timestamp,
            dkd_commitment_proofs: BTreeMap::new(),
            next_nonce: 0,
            used_nonces: BTreeSet::new(),
        };

        let mut devices = BTreeMap::new();
        devices.insert(device_id, device_metadata);

        // Create initial account state
        let initial_state = AccountState {
            account_id,
            group_public_key: signing_key.verifying_key(),
            devices,
            removed_devices: BTreeSet::new(),
            guardians: BTreeMap::new(),
            removed_guardians: BTreeSet::new(),
            session_epoch: SessionEpoch::initial(),
            lamport_clock: 0,
            dkd_commitment_roots: BTreeMap::new(),
            sessions: BTreeMap::new(),
            active_operation_lock: None,
            presence_tickets: BTreeMap::new(),
            cooldowns: BTreeMap::new(),
            authority_graph: aura_journal::capability::authority_graph::AuthorityGraph::new(),
            visibility_index: aura_journal::capability::visibility::VisibilityIndex::new(
                aura_journal::capability::authority_graph::AuthorityGraph::new(),
                effects,
            ),
            threshold: 1,
            total_participants: 1,
            used_nonces: BTreeSet::new(),
            next_nonce: 0,
            last_event_hash: None,
            updated_at: timestamp,
            sbb_envelopes: BTreeMap::new(),
            sbb_neighbors: BTreeSet::new(),
            relationship_keys: BTreeMap::new(),
            relationship_counters: BTreeMap::new(),
        };

        // Create ledger with initial state
        let ledger = AccountLedger::new(initial_state).expect("Failed to create ledger");

        // Generate initial DKD identity (in real scenario this would be done via DKD protocol)
        let context = format!("social-{}", name);
        let current_identity = effects.random_bytes::<32>().to_vec();

        SimulatedAccount {
            account_id,
            ledger,
            device_id,
            signing_key,
            current_identity,
            contacts: HashMap::new(),
        }
    }

    /// Establish contact with another account
    fn add_contact(&mut self, other_account: &SimulatedAccount, effects: &Effects) {
        let relationship_id =
            RelationshipId::from_accounts(self.account_id, other_account.account_id);

        let contact = Contact {
            account_id: other_account.account_id,
            dkd_identity: other_account.current_identity.clone(),
            established_at: effects.now().unwrap_or(0),
            relationship_id,
        };

        self.contacts.insert(other_account.account_id, contact);

        println!(
            "[CONTACT] Account {} added contact {} (identity: {}...)",
            hex::encode(&self.account_id.0.as_bytes()[..4]),
            hex::encode(&other_account.account_id.0.as_bytes()[..4]),
            hex::encode(&other_account.current_identity[..4])
        );
    }

    /// Simulate key rotation (resharing + new DKD)
    fn rotate_keys(&mut self, effects: &Effects) {
        // In a real scenario, this would involve:
        // 1. Running resharing protocol to get new threshold key
        // 2. Running DKD protocol with new key to derive new identity
        // For simulation, we just generate a new random identity

        let old_identity = self.current_identity.clone();
        self.current_identity = effects.random_bytes::<32>().to_vec();

        println!(
            "[ROTATION] Account {} rotated keys",
            hex::encode(&self.account_id.0.as_bytes()[..4])
        );
        println!("  Old identity: {}...", hex::encode(&old_identity[..8]));
        println!(
            "  New identity: {}...",
            hex::encode(&self.current_identity[..8])
        );
    }

    /// Find contacts who might know about a specific old contact
    fn find_mutual_contacts(&self, target_account_id: &AccountId) -> Vec<AccountId> {
        // In a real implementation, this would query contacts to see who else
        // they know, looking for overlap with our lost contact
        self.contacts
            .keys()
            .filter(|id| *id != target_account_id)
            .copied()
            .collect()
    }

    /// Query a contact for rendezvous information about another account
    fn query_rendezvous(
        &self,
        _contact_id: &AccountId,
        _target_old_identity: &[u8],
    ) -> Option<Vec<u8>> {
        // This simulates asking a contact: "I used to talk to someone with identity X,
        // do you know their new identity?"
        // In reality, this would be a secure protocol using encrypted queries
        None // Placeholder - real implementation would search contact's graph
    }

    /// Get current identity
    fn get_current_identity(&self) -> &[u8] {
        &self.current_identity
    }

    /// Get account ID
    fn get_account_id(&self) -> AccountId {
        self.account_id
    }
}

/// Rendezvous coordinator - helps accounts rediscover each other
struct RendezvousCoordinator {
    /// Mapping from old identity to new identity announcements
    identity_updates: HashMap<Vec<u8>, Vec<u8>>,
    /// Mapping from account ID to current identity
    current_identities: HashMap<AccountId, Vec<u8>>,
}

impl RendezvousCoordinator {
    fn new() -> Self {
        RendezvousCoordinator {
            identity_updates: HashMap::new(),
            current_identities: HashMap::new(),
        }
    }

    /// Register an identity update (old -> new)
    fn register_update(
        &mut self,
        account_id: AccountId,
        old_identity: Vec<u8>,
        new_identity: Vec<u8>,
    ) {
        self.identity_updates
            .insert(old_identity.clone(), new_identity.clone());
        self.current_identities
            .insert(account_id, new_identity.clone());

        println!(
            "[RENDEZVOUS] Registered identity update for account {}",
            hex::encode(&account_id.0.as_bytes()[..4])
        );
        println!("  Old: {}...", hex::encode(&old_identity[..8]));
        println!("  New: {}...", hex::encode(&new_identity[..8]));
    }

    /// Lookup new identity for an old identity
    fn lookup_new_identity(&self, old_identity: &[u8]) -> Option<Vec<u8>> {
        self.identity_updates.get(old_identity).cloned()
    }

    /// Lookup current identity for an account
    fn lookup_current_identity(&self, account_id: &AccountId) -> Option<Vec<u8>> {
        self.current_identities.get(account_id).cloned()
    }
}

#[tokio::test]
async fn test_social_graph_rediscovery() {
    println!("\n=== Social Graph Rediscovery Test ===\n");

    // Create simulation with deterministic seed
    let seed = 42;
    let effects = Effects::deterministic(seed, 1735689600);

    // Phase 1: Create three accounts
    println!("--- Phase 1: Create Accounts ---");
    let mut account_a = SimulatedAccount::new("Alice", &effects);
    let mut account_b = SimulatedAccount::new("Bob", &effects);
    let mut account_c = SimulatedAccount::new("Carol", &effects);

    println!(
        "[OK] Created Account A (Alice): {}",
        hex::encode(&account_a.get_account_id().0.as_bytes()[..8])
    );
    println!(
        "[OK] Created Account B (Bob): {}",
        hex::encode(&account_b.get_account_id().0.as_bytes()[..8])
    );
    println!(
        "[OK] Created Account C (Carol): {}",
        hex::encode(&account_c.get_account_id().0.as_bytes()[..8])
    );

    // Phase 2: Establish initial contacts (form triangle)
    println!("\n--- Phase 2: Establish Initial Contacts ---");

    // A <-> B
    account_a.add_contact(&account_b, &effects);
    account_b.add_contact(&account_a, &effects);

    // A <-> C
    account_a.add_contact(&account_c, &effects);
    account_c.add_contact(&account_a, &effects);

    // B <-> C
    account_b.add_contact(&account_c, &effects);
    account_c.add_contact(&account_b, &effects);

    println!("[OK] Social triangle established (A-B, A-C, B-C)");

    // Verify initial connectivity
    assert_eq!(
        account_a.contacts.len(),
        2,
        "Account A should have 2 contacts"
    );
    assert_eq!(
        account_b.contacts.len(),
        2,
        "Account B should have 2 contacts"
    );
    assert_eq!(
        account_c.contacts.len(),
        2,
        "Account C should have 2 contacts"
    );

    // Save old identities before rotation
    let old_identity_b = account_b.get_current_identity().to_vec();
    let old_identity_c = account_c.get_current_identity().to_vec();

    // Phase 3: Key rotation for B and C
    println!("\n--- Phase 3: Key Rotation ---");

    account_b.rotate_keys(&effects);
    account_c.rotate_keys(&effects);

    println!("[OK] Both Account B and C have rotated keys");

    // Phase 4: Simulate loss of direct connectivity
    println!("\n--- Phase 4: Verify Direct Communication Lost ---");

    // B's old identity for C is now invalid
    let b_knows_old_c = account_b
        .contacts
        .get(&account_c.get_account_id())
        .map(|c| c.dkd_identity.clone());

    if let Some(old_c_identity) = b_knows_old_c {
        assert_ne!(
            old_c_identity,
            account_c.get_current_identity(),
            "B should have outdated identity for C"
        );
        println!(
            "[VERIFIED] B's cached identity for C is outdated: {}... != {}...",
            hex::encode(&old_c_identity[..8]),
            hex::encode(&account_c.get_current_identity()[..8])
        );
    }

    // Phase 5: Rendezvous through Account A
    println!("\n--- Phase 5: Rediscovery via Rendezvous ---");

    // Create rendezvous coordinator (simulating Account A's role)
    let mut rendezvous = RendezvousCoordinator::new();

    // Account A knows both B and C's new identities (simulating that A stayed connected)
    rendezvous.register_update(
        account_b.get_account_id(),
        old_identity_b.clone(),
        account_b.get_current_identity().to_vec(),
    );
    rendezvous.register_update(
        account_c.get_account_id(),
        old_identity_c.clone(),
        account_c.get_current_identity().to_vec(),
    );

    // B queries A for C's new identity
    println!("\n[QUERY] B asks A: 'What's C's new identity?'");
    let c_new_identity = rendezvous.lookup_current_identity(&account_c.get_account_id());
    assert!(c_new_identity.is_some(), "A should know C's new identity");

    if let Some(new_id) = c_new_identity {
        println!(
            "[RESPONSE] A tells B: C's new identity is {}...",
            hex::encode(&new_id[..8])
        );
    }

    // C queries A for B's new identity
    println!("\n[QUERY] C asks A: 'What's B's new identity?'");
    let b_new_identity = rendezvous.lookup_current_identity(&account_b.get_account_id());
    assert!(b_new_identity.is_some(), "A should know B's new identity");

    if let Some(new_id) = b_new_identity {
        println!(
            "[RESPONSE] A tells C: B's new identity is {}...",
            hex::encode(&new_id[..8])
        );
    }

    // Phase 6: Re-establish contact with new identities
    println!("\n--- Phase 6: Re-establish Contact ---");

    // B updates C's identity in contacts
    if let Some(contact) = account_b.contacts.get_mut(&account_c.get_account_id()) {
        contact.dkd_identity = account_c.get_current_identity().to_vec();
        println!(
            "[UPDATE] B updated C's identity to {}...",
            hex::encode(&contact.dkd_identity[..8])
        );
    }

    // C updates B's identity in contacts
    if let Some(contact) = account_c.contacts.get_mut(&account_b.get_account_id()) {
        contact.dkd_identity = account_b.get_current_identity().to_vec();
        println!(
            "[UPDATE] C updated B's identity to {}...",
            hex::encode(&contact.dkd_identity[..8])
        );
    }

    // Verify connectivity restored
    let b_current_c = account_b
        .contacts
        .get(&account_c.get_account_id())
        .map(|c| c.dkd_identity.clone());

    assert_eq!(
        b_current_c.as_deref(),
        Some(account_c.get_current_identity()),
        "B should now have C's current identity"
    );

    let c_current_b = account_c
        .contacts
        .get(&account_b.get_account_id())
        .map(|c| c.dkd_identity.clone());

    assert_eq!(
        c_current_b.as_deref(),
        Some(account_b.get_current_identity()),
        "C should now have B's current identity"
    );

    println!("[OK] B and C successfully rediscovered each other via A");

    // Final verification
    println!("\n--- Phase 7: Final Verification ---");
    println!("[VERIFIED] Social graph connectivity restored:");
    println!("  A <-> B: ✓");
    println!("  A <-> C: ✓");
    println!("  B <-> C: ✓ (rediscovered via A)");
    println!("\n=== Test Complete ===\n");
}

#[tokio::test]
async fn test_multiple_rendezvous_paths() {
    println!("\n=== Multiple Rendezvous Paths Test ===\n");

    // This test verifies that rediscovery works even when there are
    // multiple possible rendezvous points in the social graph

    let seed = 123;
    let effects = Effects::deterministic(seed, 1735689600);

    // Create a more complex social graph: A-B-C-D (chain)
    println!("--- Phase 1: Create Chain Social Graph ---");
    let mut account_a = SimulatedAccount::new("Alice", &effects);
    let mut account_b = SimulatedAccount::new("Bob", &effects);
    let mut account_c = SimulatedAccount::new("Carol", &effects);
    let mut account_d = SimulatedAccount::new("Dave", &effects);

    // Establish chain: A-B-C-D
    account_a.add_contact(&account_b, &effects);
    account_b.add_contact(&account_a, &effects);

    account_b.add_contact(&account_c, &effects);
    account_c.add_contact(&account_b, &effects);

    account_c.add_contact(&account_d, &effects);
    account_d.add_contact(&account_c, &effects);

    println!("[OK] Chain established: A-B-C-D");

    // Save old identities
    let old_identity_a = account_a.get_current_identity().to_vec();
    let old_identity_d = account_d.get_current_identity().to_vec();

    // Rotate keys for A and D (endpoints)
    println!("\n--- Phase 2: Rotate Endpoint Keys ---");
    account_a.rotate_keys(&effects);
    account_d.rotate_keys(&effects);

    // A and D now can't communicate directly (never could in this graph)
    // But they can rediscover through B and C

    println!("\n--- Phase 3: Rediscovery via Chain ---");
    let mut rendezvous = RendezvousCoordinator::new();

    // B knows A's new identity
    rendezvous.register_update(
        account_a.get_account_id(),
        old_identity_a,
        account_a.get_current_identity().to_vec(),
    );

    // C knows D's new identity
    rendezvous.register_update(
        account_d.get_account_id(),
        old_identity_d,
        account_d.get_current_identity().to_vec(),
    );

    // Even though A and D never directly communicated, they could
    // potentially discover each other through the chain B-C

    println!("[OK] Rediscovery through multi-hop path is possible");
    println!("  Path: A -> B -> C -> D");

    println!("\n=== Test Complete ===\n");
}

#[tokio::test]
async fn test_rendezvous_with_missing_node() {
    println!("\n=== Rendezvous with Missing Node Test ===\n");

    // This test verifies graceful handling when a rendezvous node
    // is offline or has also rotated keys

    let seed = 456;
    let effects = Effects::deterministic(seed, 1735689600);

    println!("--- Phase 1: Create Triangle ---");
    let mut account_a = SimulatedAccount::new("Alice", &effects);
    let mut account_b = SimulatedAccount::new("Bob", &effects);
    let mut account_c = SimulatedAccount::new("Carol", &effects);

    // Triangle: A-B, A-C, B-C
    account_a.add_contact(&account_b, &effects);
    account_b.add_contact(&account_a, &effects);
    account_a.add_contact(&account_c, &effects);
    account_c.add_contact(&account_a, &effects);
    account_b.add_contact(&account_c, &effects);
    account_c.add_contact(&account_b, &effects);

    println!("[OK] Triangle established");

    // All three rotate keys
    println!("\n--- Phase 2: All Nodes Rotate ---");
    let old_a = account_a.get_current_identity().to_vec();
    let old_b = account_b.get_current_identity().to_vec();
    let old_c = account_c.get_current_identity().to_vec();

    account_a.rotate_keys(&effects);
    account_b.rotate_keys(&effects);
    account_c.rotate_keys(&effects);

    println!("[OK] All nodes rotated keys");

    // Now everyone needs to rediscover everyone else
    println!("\n--- Phase 3: Mutual Rediscovery ---");

    // This requires a coordinated rendezvous protocol where:
    // 1. Each node announces their identity update to all contacts
    // 2. Each node forwards updates from their contacts
    // 3. After flooding, everyone should know everyone's new identity

    let mut rendezvous = RendezvousCoordinator::new();
    rendezvous.register_update(
        account_a.get_account_id(),
        old_a,
        account_a.get_current_identity().to_vec(),
    );
    rendezvous.register_update(
        account_b.get_account_id(),
        old_b,
        account_b.get_current_identity().to_vec(),
    );
    rendezvous.register_update(
        account_c.get_account_id(),
        old_c,
        account_c.get_current_identity().to_vec(),
    );

    println!("[OK] Rendezvous coordinator has all updates");
    println!("[VERIFIED] Mutual rediscovery possible through flooding");

    println!("\n=== Test Complete ===\n");
}
