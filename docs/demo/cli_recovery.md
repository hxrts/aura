# Aura CLI Recovery Demo

This demonstrates Aura's social identity and secure messaging capabilities from **Bob's perspective** as the demo user through the **CLI interface**. Alice and Charlie are pre-configured to support Bob's user flow.

## Demo Preparation (Pre-configured)

### Alice Creates Account
**Demo Setup**: Alice's account is pre-configured for the demo environment.

**Behind the Scenes**:
```bash
# Technical equivalent 
aura init --name "Alice" --threshold 2 --devices 3
```
- Creates new `AuthorityId` for Alice's identity
- Generates FROST threshold key shares (2-of-3 setup)
- Initializes commitment tree with Alice's devices
- Creates initial journal for Alice's authority
- Generates DKD capabilities for various contexts (messaging, storage, etc.)

### Charlie Creates Account  
**Demo Setup**: Charlie's account is pre-configured for the demo environment.

**Behind the Scenes**:
- Independent authority creation with separate `AuthorityId`
- Own threshold setup and commitment tree
- No relationship to Alice's account yet

### Alice and Charlie Establish Friendship
**Demo Setup**: Alice and Charlie's friendship is pre-established for the demo.

**Behind the Scenes**:
```bash
# Technical equivalent
aura invite create --type friendship --recipient charlie
aura invite accept --invite-token <token>
```
- Creates RelationalContext for Alice-Charlie friendship
- Establishes shared `ContextId` and derives context keys using DKD
- Both authorities can now communicate in this relationship context
- Mutual friendship relationship established

**Result**: Alice and Charlie can message each other and are ready to interact with Bob during the demo.

---

## Bob's User Flow (The Actual Demo)

### Alice Sends Invite to Bob  
**Bob's Experience**: Bob receives an invite code from Alice (the demo starts here).

**Behind the Scenes**:
- Alice creates device invitation using `aura-invitation/device_invitation.rs`
- Generates onboarding token with Alice's authority commitment
- Token is content-addressed for security and unlinkability
- No personal information embedded - just cryptographic invitation

### Bob Downloads and Installs Desktop Binary
**Bob's Experience**: Bob downloads and installs the Aura CLI tool on his computer.

**Behind the Scenes**:
- Bob gets fresh installation with no existing identity
- Aura binary contains full P2P infrastructure (no servers)  
- Ready to bootstrap into network via invitation

### Bob Provides Invite Code on Startup
**Bob's Experience**: Bob runs `aura init` and is prompted "Enter invite code to join". Bob enters Alice's invitation code.

**Behind the Scenes**:
```bash
# Technical equivalent
aura bootstrap --invite-code <alice-invite>
```
- Creates new `AuthorityId` for Bob's identity
- Generates Bob's threshold keys (2-of-3 devices recommended)
- Uses Alice's invitation to establish initial network connection
- Creates Alice-Bob RelationalContext for their friendship
- Derives context keys for secure communication

### Guardian Setup Prompt
**Bob's Experience**: The CLI displays: "Alice invited you! Would you like to add Alice as a guardian to help recover your account if you lose access? (y/n)" Bob types `y`.

**Behind the Scenes**:
- Uses `aura-invitation/guardian_invitation.rs` choreography
- Bob initiates guardian relationship request
- Creates guardian-specific RelationalContext between Alice and Bob
- This is separate from their friendship context for security isolation

### Alice Accepts Guardian Request
**Bob's Experience**: CLI shows "Waiting for Alice to accept guardian request..."
**Alice's Experience**: Alice runs `aura guardian list-requests` and sees Bob's request, then runs `aura guardian accept <request-id>`.

**Behind the Scenes**:
```bash
# Alice runs (or GUI equivalent)
aura recovery guardian-accept --request-id <bob-request>
```
- Alice approves guardian relationship in guardian RelationalContext
- GuardianBinding fact is committed to both authorities' journals
- Bob's account now has Alice configured as trusted guardian
- Guardian context keys derived for future recovery operations

### Bob Gets Guardian Confirmation
**Bob's Experience**: CLI displays: "Great! Alice has agreed to be your guardian. She can help you recover your account if needed."

**Behind the Scenes**:
- Bob's journal receives guardian confirmation fact
- Bob's authority now has guardian recovery capability
- Alice has corresponding guardian duties stored in her journal
- Recovery threshold established (1-of-1 in this case)

### Bob Receives Charlie's Invite
**Bob's Experience**: Bob gets a second invite code from Charlie and runs `aura invite accept <charlie-invite-code>`.

**Behind the Scenes**:
```bash
# Technical equivalent
aura invite accept --invite-token <charlie-invite>
```
- Creates separate Alice-Charlie RelationalContext
- No cross-linking between Alice-Bob and Bob-Charlie relationships
- Context isolation preserves privacy - Alice doesn't learn about Bob-Charlie connection
- Three separate relationships now exist: Alice↔Bob, Alice↔Charlie, Bob↔Charlie

### Charlie Gets Acceptance Notification
**Bob's Experience**: CLI shows "You're now connected with Charlie!"
**Charlie's Experience**: Charlie runs `aura contacts list` and sees Bob is now connected.

**Behind the Scenes**:
- Charlie's journal receives relationship acceptance fact
- Charlie can now derive context keys for Bob communication
- Secure channel can be established for messaging

### Group Chat Creation
**Bob's Experience**: Bob runs `aura chat list` and sees a new group "Demo Group" that Charlie created and invited him to.
**Charlie's Experience**: Charlie runs `aura chat create --name "Demo Group" --members bob` then `aura chat send --group <group-id> --message "Hey Bob, welcome to Aura!"`.

**Behind the Scenes**:
```bash
# Technical equivalent
aura chat create --members bob,charlie
aura chat send --chat <chat-id> --message "Hey Bob, welcome to Aura!"
```
- Creates new `ContextId` for the group chat context  
- Uses DKD to derive group chat encryption keys
- Messages stored as facts in both participants' journals
- Real-time sync via `aura-sync` anti-entropy protocol
- Flow budgets applied to prevent spam

### Bob Responds in Chat
**Bob's Experience**: Bob runs `aura chat history --group <group-id>` to see Charlie's message, then responds with `aura chat send --group <group-id> --message "Hey Charlie! Thanks for adding me to Aura!"`.

**Behind the Scenes**:
- Bob derives same group context keys as Charlie
- Messages encrypted/decrypted using shared context keys
- CRDT-based message ordering ensures consistency
- Messages replicated across all group members' devices

**Result**: Bob is fully onboarded with guardian protection and active friendships.

---

## Catastrophic Failure (Demo Simulation)

### Bob Loses Everything  
**Bob's Experience**: The demo simulates Bob losing his device by having him close Aura and all data is wiped.

**Behind the Scenes**:
```bash
# Simulate catastrophic failure
rm -rf ~/.aura_data
rm aura-binary  
```
- All local keys, configuration, and message history destroyed
- Bob's threshold key shares lost
- No way to derive context keys for any relationships
- Bob effectively locked out of all contexts

**Critical Point**: Bob's identity (`AuthorityId`) and relationships still exist in Alice and Charlie's journals, but Bob has no way to access them.

---

## Guardian Recovery (Bob Gets Back In)

### Bob Re-downloads and Re-installs Aura
**Bob's Experience**: Bob downloads and installs the Aura CLI again on a fresh device.

**Behind the Scenes**:
- Fresh installation with no existing identity
- No access to previous `AuthorityId` or relationships
- Aura detects no existing account

### Bob Initiates Recovery
**Bob's Experience**: Bob runs `aura recovery start` and the CLI prompts him to begin account recovery.

**Behind the Scenes**:
```bash
# Technical equivalent
aura recovery initiate --lost-device-scenario
```
- Creates temporary recovery context and identity for this recovery session
- Generates temporary `ContextId` for recovery coordination
- Creates recovery request with partial identity information
- **Important**: This is temporary identity, not Bob's original authority

### Recovery Context Information
**Bob's Experience**: The CLI displays a recovery code: "Give this recovery code to your guardian: `recovery-temp-abc123-def456`"

**Behind the Scenes**:
- Recovery code contains temporary context information for this session
- Includes cryptographic material for guardian to verify recovery attempt
- Code is designed to be communicated through out-of-band channels (phone call, email)
- Guardian can use this to initiate recovery on their end

### Bob Contacts Alice with Recovery Code
**Bob's Experience**: Bob calls Alice: "I lost my Aura device. Can you help me recover? Use this code: `recovery-temp-abc123-def456`"

**Behind the Scenes**:
- Out-of-band communication (not through Aura network)
- Alice needs to input this into her Aura guardian interface
- Code allows Alice to verify this is legitimate recovery request from Bob

### Alice Initiates Guardian Recovery  
**Bob's Experience**: CLI shows "Waiting for Alice to help with recovery..."
**Alice's Experience**: Alice runs `aura guardian recover --recovery-code recovery-temp-abc123-def456`.

**Behind the Scenes**:
```bash
# Alice runs (or GUI equivalent)
aura recovery guardian-assist --recovery-code recovery-temp-abc123-def456
```
- Alice's Aura verifies the recovery request against Bob's guardian relationship
- Uses `aura-recovery/recovery_protocol.rs` choreography
- Alice's guardian key shares are prepared for reconstruction
- Recovery approval process begins

### Guardian Key Reconstruction
**Bob's Experience**: CLI shows "Alice is helping recover your account..."
**Alice's Experience**: CLI prompts Alice "Approve recovery for Bob? (y/n)" and Alice types `y`.

**Behind the Scenes**:
- Alice provides her guardian key share to recovery process
- If threshold met (1-of-1 in this case), Bob's original threshold keys are reconstructed
- Bob regains access to his original `AuthorityId`
- All original context keys can now be re-derived using DKD
- Bob's device gets his reconstructed account state

### Account Regeneration
**Bob's Experience**: CLI displays "Recovery successful! Your account has been restored."

**Behind the Scenes**:
- Bob's temporary recovery identity is discarded
- Bob's original authority is restored with all relationships intact
- Context keys for Alice friendship, Charlie friendship, and group chat can be re-derived
- Bob can access all his previous RelationalContexts

### Contact List Restoration
**Bob's Experience**: Bob runs `aura contacts list` and sees Alice and Charlie restored exactly as before.

**Behind the Scenes**:
- Authority state reconstruction includes all relationship facts
- Alice-Bob and Bob-Charlie RelationalContexts are accessible again
- Guardian relationship with Alice is maintained
- All friendship contexts and permissions restored

### Rendezvous Reconnection
**Bob's Experience**: CLI shows "Reconnecting to friends..." and then "Connected to Alice, connecting to Charlie..."

**Behind the Scenes**:
```bash
# Technical equivalent happening automatically
aura rendezvous connect --peer alice
aura rendezvous connect --peer charlie --via alice  # Multi-hop through Alice
```
- Uses `aura-rendezvous` to re-establish secure channels
- Bob can connect to Alice directly (guardian relationship provides bootstrap)
- Bob connects to Charlie through Alice (multi-hop peer discovery)
- Each connection uses appropriate context keys for that relationship

### Message Sync Recovery
**Bob's Experience**: CLI shows "Syncing chat history..." and when Bob runs `aura chat history --group <group-id>` all previous messages with Charlie appear exactly as they were before.

**Behind the Scenes**:
- Uses `aura-sync` anti-entropy protocol to sync journal state
- Charlie's journal contains full group chat history
- CRDT-based sync ensures Bob gets all missed messages in correct order
- Group chat context keys allow Bob to decrypt all previous messages
- Message delivery receipts updated to show Bob is back online

**Final Result**: Bob has completely recovered his identity, relationships, and full chat history. From the user perspective, it's as if he never lost his device.

---

## Technical Architecture Highlights

### Privacy Preservation Throughout Recovery
- **Identity isolation**: Recovery happens in temporary context that doesn't leak Bob's real identity
- **Guardian isolation**: Alice only sees recovery request, not Bob's other relationships  
- **Context partitioning**: Alice-Bob, Bob-Charlie, and group chat contexts remain separate
- **No global identity**: Even during recovery, no single point reveals Bob's complete social graph

### Cryptographic Security During Recovery
- **Threshold reconstruction**: Original security properties maintained through guardian key shares
- **Forward secrecy**: Recovery doesn't compromise past or future communications
- **Context key derivation**: All encryption keys properly re-derived from reconstructed authority
- **Authentication**: Guardian verification ensures only legitimate recovery requests succeed

### Distributed Consistency During Recovery
- **CRDT convergence**: All participants eventually have consistent state after sync
- **Causal ordering**: Message history maintains proper ordering across all devices
- **Conflict resolution**: Any temporary inconsistencies resolve automatically
- **Availability**: System remains functional for Alice and Charlie while Bob recovers

This demo showcases Aura's unique value proposition: **true peer-to-peer social recovery** without compromising privacy, security, or requiring trust in centralized services.
