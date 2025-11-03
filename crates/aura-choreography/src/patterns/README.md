# Choreographic Patterns

This directory contains reusable choreographic patterns that can be used across different protocols in Aura.

## Core Choreographic Patterns

The three fundamental patterns that form the building blocks for more complex choreographies:

### 1. All-to-All Broadcast and Gather (`broadcast_and_gather.rs`)

The most fundamental pattern in peer-to-peer protocols where every participant sends a message of the same type to every other participant, and then waits until they have received a message from all other N-1 participants.

**Key Features:**
- Message validation and integrity checking
- Timeout management with configurable timeouts
- Byzantine behavior detection through message verification
- Epoch-based anti-replay protection
- Duplicate message detection
- Comprehensive logging and tracing

**Used extensively in:**
- FROST signing for exchanging nonce commitments and signature shares
- DKD protocols for exchanging key derivation shares
- Any protocol requiring synchronized information exchange

### 2. Decentralized Result Verification (`verify_consistent_result.rs`)

Ensures that all participants have computed the same local result using a secure commit-reveal protocol. Critical for maintaining consensus in decentralized protocols without requiring a central coordinator.

**Key Features:**
- Secure commit-reveal protocol with optional nonces
- Consistency analysis and Byzantine participant detection
- Configurable timeouts for commit and reveal phases
- Early termination optimization when all commitments match
- Comprehensive result comparison with customizable comparators

**Used extensively in:**
- DKD protocols to verify identical derived keys
- FROST signing to verify identical aggregated signatures
- Any protocol requiring decentralized result consistency

### 3. Propose and Acknowledge (`propose_and_acknowledge.rs`)

A simple choreography where a designated initiator role sends a piece of data (the 'proposal') to all other participants, who then receive and implicitly acknowledge it by proceeding to the next step.

**Key Features:**
- Implicit or explicit acknowledgment modes
- Proposal size validation and integrity checking
- Timeout management for acknowledgment collection
- Epoch-based anti-replay protection
- Customizable proposal validation

**Used extensively in:**
- Protocol initialization and configuration distribution
- Epoch announcements and state transitions
- Leader-driven coordination in consensus protocols
- Configuration updates and parameter changes

## Pattern Composition and Benefits

These three fundamental patterns can be composed to build more complex choreographies. For example:

- **FROST Signing:** Uses broadcast_and_gather for nonce commitments, another broadcast_and_gather for signature shares, and verify_consistent_result for final signature verification
- **DKD Protocol:** Uses propose_and_acknowledge for context setup, broadcast_and_gather for key material exchange, and verify_consistent_result for derived key consistency
- **Recovery Protocol:** Uses propose_and_acknowledge for recovery initiation, broadcast_and_gather for guardian shares, and verify_consistent_result for account state consistency

### Code Reduction Benefits

By extracting these patterns, choreography implementations become much shorter and more declarative:

| Protocol | Before Patterns | After Patterns | Reduction |
|----------|----------------|----------------|-----------|
| FROST Signing | ~400 lines | ~150 lines | 62% |
| DKD Protocol | ~350 lines | ~120 lines | 66% |
| Recovery Protocol | ~500 lines | ~180 lines | 64% |

### Security Benefits

- **Uniform Security:** All patterns include consistent epoch-based anti-replay protection
- **Byzantine Tolerance:** Built-in detection of malicious participants across all patterns
- **Timeout Management:** Consistent timeout handling prevents protocol hanging
- **Message Integrity:** All patterns include cryptographic message integrity checking

### Maintainability Benefits

- **Single Source of Truth:** Security properties implemented once and reused everywhere
- **Consistent Testing:** Unified testing approach for all protocols using these patterns
- **Debugging:** Uniform logging and tracing across all choreographic operations
- **Documentation:** Self-documenting choreographies through pattern usage

## ThresholdCollect Pattern

The `ThresholdCollect` pattern provides a generic, reusable implementation for threshold operations that follow the common flow:

1. **Context Agreement** - All participants agree on the operation context/message
2. **Material Exchange** - Participants exchange cryptographic materials (shares/nonces/commitments)
3. **Local Aggregation** - Each participant aggregates the collected materials locally
4. **Result Verification** - Participants verify consistency of the final result

### Key Features

- **Type-safe parameterization** - Parameterized by context, material, and result types
- **Byzantine fault tolerance** - Optional Byzantine behavior detection and handling
- **Epoch-based anti-replay** - Built-in protection against replay attacks
- **Timeout management** - Configurable timeouts for each phase
- **Comprehensive logging** - Detailed tracing for debugging and monitoring
- **Consistency verification** - Automatic verification of result consistency across participants

### Usage Pattern

To use the `ThresholdCollect` pattern for a new threshold operation:

1. **Define your types:**
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct MyContext { /* ... */ }
   
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct MyMaterial { /* ... */ }
   
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct MyResult { /* ... */ }
   ```

2. **Implement the provider trait:**
   ```rust
   pub struct MyThresholdProvider;
   
   impl ThresholdOperationProvider<MyContext, MyMaterial, MyResult> for MyThresholdProvider {
       fn validate_context(&self, context: &MyContext) -> Result<(), String> {
           // Validate the operation context
       }
       
       fn generate_material(&self, context: &MyContext, participant: ChoreographicRole, effects: &Effects) -> Result<MyMaterial, String> {
           // Generate cryptographic material for this participant
       }
       
       fn validate_material(&self, context: &MyContext, participant: ChoreographicRole, material: &MyMaterial, effects: &Effects) -> Result<(), String> {
           // Validate received material from another participant
       }
       
       fn aggregate_materials(&self, context: &MyContext, materials: &BTreeMap<ChoreographicRole, MyMaterial>, effects: &Effects) -> Result<MyResult, String> {
           // Aggregate all collected materials into final result
       }
       
       fn verify_result(&self, context: &MyContext, result: &MyResult, participants: &[ChoreographicRole], effects: &Effects) -> Result<bool, String> {
           // Verify the final result is valid
       }
       
       fn operation_name(&self) -> &str {
           "MyOperation"
       }
   }
   ```

3. **Execute the choreography:**
   ```rust
   let config = ThresholdCollectConfig {
       threshold: 2,
       phase_timeout_seconds: 30,
       enable_byzantine_detection: true,
       ..Default::default()
   };
   
   let provider = MyThresholdProvider;
   let choreography = ThresholdCollectChoreography::new(
       config,
       context,
       participants,
       provider,
       effects,
   )?;
   
   let result = choreography.execute(handler, endpoint, my_role).await?;
   ```

### Examples

The `threshold_examples.rs` file demonstrates how to implement DKD and FROST protocols using this pattern:

#### DKD Example
- **Context:** `DkdContext` with app_id, derivation_context, and threshold
- **Material:** `DkdMaterial` with key shares and commitments
- **Result:** `DkdResult` with derived key and proof
- **Logic:** Deterministic key derivation with XOR aggregation

#### FROST Example  
- **Context:** `FrostContext` with message, threshold, and key package ID
- **Material:** `FrostMaterial` with FROST commitments and signature shares
- **Result:** `FrostResult` with aggregated signature and verification key
- **Logic:** Threshold signature generation and verification

### Benefits of the Pattern

1. **Code Reuse:** Eliminates duplication across threshold protocols
2. **Consistency:** Ensures all threshold operations follow the same flow
3. **Security:** Built-in Byzantine fault tolerance and anti-replay protection
4. **Maintainability:** Centralized timeout, error handling, and consistency checks
5. **Testing:** Unified testing approach for all threshold operations
6. **Monitoring:** Consistent logging and tracing across protocols

### Protocol Comparison

| Aspect | Before Pattern | After Pattern |
|--------|----------------|---------------|
| Code Lines | ~300-500 per protocol | ~100-150 per protocol |
| Security Checks | Manual, inconsistent | Automatic, uniform |
| Error Handling | Protocol-specific | Centralized, robust |
| Testing | Custom per protocol | Reusable test patterns |
| Monitoring | Inconsistent logging | Uniform tracing |
| Byzantine Tolerance | Optional, manual | Built-in, configurable |

### Configuration Options

The `ThresholdCollectConfig` provides several configuration options:

- `threshold`: Minimum number of participants required for success
- `phase_timeout_seconds`: Timeout for each choreographic phase
- `max_participants`: Maximum allowed participants
- `enable_byzantine_detection`: Enable Byzantine behavior detection
- `epoch`: Epoch for anti-replay protection

### Error Handling

The pattern provides comprehensive error handling:

- **Context validation errors:** Invalid operation context
- **Material validation errors:** Invalid cryptographic material
- **Aggregation errors:** Failures during local aggregation
- **Consistency errors:** Result inconsistency across participants
- **Timeout errors:** Operation timeouts
- **Byzantine errors:** Detected Byzantine behavior

### Future Extensions

Potential extensions to the pattern:

1. **Async material generation:** Support for asynchronous material generation
2. **Partial aggregation:** Support for partial result aggregation
3. **Dynamic thresholds:** Support for dynamic threshold adjustment
4. **Batch operations:** Support for batching multiple operations
5. **Recovery mechanisms:** Automatic recovery from partial failures