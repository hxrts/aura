// Deterministic Key Derivation (DKD) Protocol Specification
// Simplified version that follows working Quint syntax patterns
module deterministic_key_derivation {
  // ==================== STATE ====================
  
  // Phase tracking
  var phase: str  // "init", "commit", "reveal", "derive", "done", "failed"
  var currentRound: int
  
  // Participant states (simplified to 3 fixed participants)
  var aliceCommitted: bool
  var aliceSalt: int
  var aliceCommitment: int
  var aliceDerivedKey: int
  var aliceReceivedBobCommit: int
  var aliceReceivedCharCommit: int
  var aliceReceivedBobSalt: int
  var aliceReceivedCharSalt: int
  
  var bobCommitted: bool
  var bobSalt: int
  var bobCommitment: int
  var bobDerivedKey: int
  var bobReceivedAliceCommit: int
  var bobReceivedCharCommit: int
  var bobReceivedAliceSalt: int
  var bobReceivedCharSalt: int
  
  var charlieCommitted: bool
  var charlieSalt: int
  var charlieCommitment: int
  var charlieDerivedKey: int
  var charlieReceivedAliceCommit: int
  var charlieReceivedBobCommit: int
  var charlieReceivedAliceSalt: int
  var charlieReceivedBobSalt: int
  
  var charlieByzantine: bool
  
  // Protocol parameters
  var appId: str
  var context: str
  
  // ==================== CONSTANTS ====================
  
  pure val TIMEOUT_ROUNDS = 50
  pure val ROOT_KEY = 100  // Fixed root key for simplicity
  
  // ==================== INITIALIZATION ====================
  
  action init = all {
    phase' = "init",
    currentRound' = 0,
    aliceCommitted' = false,
    aliceSalt' = 0,
    aliceCommitment' = 0,
    aliceDerivedKey' = 0,
    aliceReceivedBobCommit' = 0,
    aliceReceivedCharCommit' = 0,
    aliceReceivedBobSalt' = 0,
    aliceReceivedCharSalt' = 0,
    bobCommitted' = false,
    bobSalt' = 0,
    bobCommitment' = 0,
    bobDerivedKey' = 0,
    bobReceivedAliceCommit' = 0,
    bobReceivedCharCommit' = 0,
    bobReceivedAliceSalt' = 0,
    bobReceivedCharSalt' = 0,
    charlieCommitted' = false,
    charlieSalt' = 0,
    charlieCommitment' = 0,
    charlieDerivedKey' = 0,
    charlieReceivedAliceCommit' = 0,
    charlieReceivedBobCommit' = 0,
    charlieReceivedAliceSalt' = 0,
    charlieReceivedBobSalt' = 0,
    charlieByzantine' = false,
    appId' = "",
    context' = ""
  }
  
  // ==================== SETUP ====================
  
  action setupProtocol: bool = all {
    phase == "init",
    phase' = "commit",
    appId' = "test_app",
    context' = "test_context",
    // Keep all other state unchanged
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine
  }
  
  // ==================== COMMITMENT PHASE ====================
  
  action aliceGenerateCommitment: bool = {
    val salt = generateDKDSalt("alice", currentRound)
    val shareBytes = ROOT_KEY  // Alice's share of root key
    val point = computeDKDPoint(shareBytes, context)
    val commitment = computeDKDCommitment(point)
    all {
      phase == "commit",
      not(aliceCommitted),
      aliceSalt' = salt,
      aliceCommitment' = commitment,
      aliceCommitted' = true,
      // Keep all other state unchanged
      phase' = phase,
      currentRound' = currentRound,
      aliceDerivedKey' = aliceDerivedKey,
      aliceReceivedBobCommit' = aliceReceivedBobCommit,
      aliceReceivedCharCommit' = aliceReceivedCharCommit,
      aliceReceivedBobSalt' = aliceReceivedBobSalt,
      aliceReceivedCharSalt' = aliceReceivedCharSalt,
      bobCommitted' = bobCommitted,
      bobSalt' = bobSalt,
      bobCommitment' = bobCommitment,
      bobDerivedKey' = bobDerivedKey,
      bobReceivedAliceCommit' = bobReceivedAliceCommit,
      bobReceivedCharCommit' = bobReceivedCharCommit,
      bobReceivedAliceSalt' = bobReceivedAliceSalt,
      bobReceivedCharSalt' = bobReceivedCharSalt,
      charlieCommitted' = charlieCommitted,
      charlieSalt' = charlieSalt,
      charlieCommitment' = charlieCommitment,
      charlieDerivedKey' = charlieDerivedKey,
      charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
      charlieReceivedBobCommit' = charlieReceivedBobCommit,
      charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
      charlieReceivedBobSalt' = charlieReceivedBobSalt,
      charlieByzantine' = charlieByzantine,
      appId' = appId,
      context' = context
    }
  }
  
  action bobGenerateCommitment: bool = {
    val salt = generateDKDSalt("bob", currentRound)
    val shareBytes = ROOT_KEY  // Bob's share of root key
    val point = computeDKDPoint(shareBytes, context)
    val commitment = computeDKDCommitment(point)
    all {
      phase == "commit",
      not(bobCommitted),
      bobSalt' = salt,
      bobCommitment' = commitment,
      bobCommitted' = true,
      // Keep all other state unchanged
      phase' = phase,
      currentRound' = currentRound,
      aliceCommitted' = aliceCommitted,
      aliceSalt' = aliceSalt,
      aliceCommitment' = aliceCommitment,
      aliceDerivedKey' = aliceDerivedKey,
      aliceReceivedBobCommit' = aliceReceivedBobCommit,
      aliceReceivedCharCommit' = aliceReceivedCharCommit,
      aliceReceivedBobSalt' = aliceReceivedBobSalt,
      aliceReceivedCharSalt' = aliceReceivedCharSalt,
      bobDerivedKey' = bobDerivedKey,
      bobReceivedAliceCommit' = bobReceivedAliceCommit,
      bobReceivedCharCommit' = bobReceivedCharCommit,
      bobReceivedAliceSalt' = bobReceivedAliceSalt,
      bobReceivedCharSalt' = bobReceivedCharSalt,
      charlieCommitted' = charlieCommitted,
      charlieSalt' = charlieSalt,
      charlieCommitment' = charlieCommitment,
      charlieDerivedKey' = charlieDerivedKey,
      charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
      charlieReceivedBobCommit' = charlieReceivedBobCommit,
      charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
      charlieReceivedBobSalt' = charlieReceivedBobSalt,
      charlieByzantine' = charlieByzantine,
      appId' = appId,
      context' = context
    }
  }
  
  action charlieGenerateCommitment: bool = {
    val salt = generateDKDSalt("charlie", currentRound)
    val shareBytes = ROOT_KEY  // Charlie's share of root key
    val point = computeDKDPoint(shareBytes, context)
    val commitment = computeDKDCommitment(point)
    all {
      phase == "commit",
      not(charlieCommitted),
      charlieSalt' = salt,
      charlieCommitment' = commitment,
      charlieCommitted' = true,
      // Keep all other state unchanged
      phase' = phase,
      currentRound' = currentRound,
      aliceCommitted' = aliceCommitted,
      aliceSalt' = aliceSalt,
      aliceCommitment' = aliceCommitment,
      aliceDerivedKey' = aliceDerivedKey,
      aliceReceivedBobCommit' = aliceReceivedBobCommit,
      aliceReceivedCharCommit' = aliceReceivedCharCommit,
      aliceReceivedBobSalt' = aliceReceivedBobSalt,
      aliceReceivedCharSalt' = aliceReceivedCharSalt,
      bobCommitted' = bobCommitted,
      bobSalt' = bobSalt,
      bobCommitment' = bobCommitment,
      bobDerivedKey' = bobDerivedKey,
      bobReceivedAliceCommit' = bobReceivedAliceCommit,
      bobReceivedCharCommit' = bobReceivedCharCommit,
      bobReceivedAliceSalt' = bobReceivedAliceSalt,
      bobReceivedCharSalt' = bobReceivedCharSalt,
      charlieDerivedKey' = charlieDerivedKey,
      charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
      charlieReceivedBobCommit' = charlieReceivedBobCommit,
      charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
      charlieReceivedBobSalt' = charlieReceivedBobSalt,
      charlieByzantine' = charlieByzantine,
      appId' = appId,
      context' = context
    }
  }
  
  action sendAliceCommitToBob: bool = all {
    phase == "commit",
    aliceCommitted,
    bobReceivedAliceCommit' = aliceCommitment,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendAliceCommitToCharlie: bool = all {
    phase == "commit",
    aliceCommitted,
    charlieReceivedAliceCommit' = aliceCommitment,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendBobCommitToAlice: bool = all {
    phase == "commit",
    bobCommitted,
    aliceReceivedBobCommit' = bobCommitment,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendBobCommitToCharlie: bool = all {
    phase == "commit",
    bobCommitted,
    charlieReceivedBobCommit' = bobCommitment,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendCharlieCommitToAlice: bool = all {
    phase == "commit",
    charlieCommitted,
    aliceReceivedCharCommit' = charlieCommitment,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendCharlieCommitToBob: bool = all {
    phase == "commit",
    charlieCommitted,
    bobReceivedCharCommit' = charlieCommitment,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  // Simplified transition to reveal phase
  action transitionToReveal: bool = all {
    phase == "commit",
    // Everyone has commitments from everyone else
    aliceReceivedBobCommit > 0,
    aliceReceivedCharCommit > 0,
    bobReceivedAliceCommit > 0,
    bobReceivedCharCommit > 0,
    charlieReceivedAliceCommit > 0,
    charlieReceivedBobCommit > 0,
    phase' = "reveal",
    currentRound' = 0,
    // Keep all other state unchanged
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  // ==================== DKD CRYPTOGRAPHIC FUNCTIONS ====================
  
  // Compute DKD point P_i = H_i * G where H_i = BLAKE3("aura.dkd.v1" || share_i || context_id)
  pure def computeDKDPoint(shareBytes: int, contextId: str): int = {
    // H_i = BLAKE3("aura.dkd.v1" || share_i || context_id)
    // Simulated as deterministic combination
    val H_i = (shareBytes * 37 + contextId.length() * 23 + 42) % 2147483647
    // P_i = H_i * G (scalar multiplication on Ed25519)
    // Simulated as H_i squared for deterministic behavior
    (H_i * H_i) % 2147483647
  }
  
  // Compute commitment C_i = BLAKE3(P_i)
  pure def computeDKDCommitment(point: int): int = {
    // Real implementation: BLAKE3(compressed_point)
    // Simulated as deterministic hash
    (point * 31 + 17) % 2147483647
  }
  
  // Aggregate DKD points to derive final key
  pure def aggregateDKDPoints(point1: int, point2: int, point3: int): int = {
    // Real implementation:
    // 1. P = P_1 + P_2 + P_3 (elliptic curve point addition)
    // 2. P' = [8]P (clear cofactor)
    // 3. seed = x-coordinate of P' mod ℓ
    // 4. key = HKDF(seed, context_id, "aura.dkd.identity.v1")
    // Simulated as deterministic combination
    val P = point1 + point2 + point3
    val P_cleared = (P * 8) % 2147483647  // Simulate cofactor clearing
    // Check for identity point (should retry with fresh nonce in real impl)
    if (P_cleared == 0) {
      1  // Fallback to avoid identity
    } else {
      // Simulate HKDF derivation
      (P_cleared * 67 + 89) % 2147483647
    }
  }
  
  // Verify commitment matches revealed point
  pure def verifyDKDReveal(commitment: int, point: int): bool = {
    commitment == computeDKDCommitment(point)
  }
  
  // Generate deterministic "random" salt for testing
  pure def generateDKDSalt(participant: str, round: int): int = {
    // Real implementation uses secure randomness
    // For spec: deterministic based on participant and round
    (participant.length() * 101 + round * 23 + 7) % 1000000
  }
  
  // ==================== REVEAL PHASE ====================
  
  action sendAliceSaltToBob: bool = all {
    phase == "reveal",
    aliceSalt > 0,
    bobReceivedAliceSalt' = aliceSalt,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendAliceSaltToCharlie: bool = all {
    phase == "reveal",
    aliceSalt > 0,
    charlieReceivedAliceSalt' = aliceSalt,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendBobSaltToAlice: bool = all {
    phase == "reveal",
    bobSalt > 0,
    aliceReceivedBobSalt' = bobSalt,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendBobSaltToCharlie: bool = all {
    phase == "reveal",
    bobSalt > 0,
    charlieReceivedBobSalt' = bobSalt,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendCharlieSaltToAlice: bool = all {
    phase == "reveal",
    charlieSalt > 0,
    aliceReceivedCharSalt' = charlieSalt,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  action sendCharlieSaltToBob: bool = all {
    phase == "reveal",
    charlieSalt > 0,
    bobReceivedCharSalt' = charlieSalt,
    // Keep all other state unchanged
    phase' = phase,
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  // Transition to derive phase when all salts are revealed
  action transitionToDerive: bool = all {
    phase == "reveal",
    // Everyone has salts from everyone else
    aliceReceivedBobSalt > 0,
    aliceReceivedCharSalt > 0,
    bobReceivedAliceSalt > 0,
    bobReceivedCharSalt > 0,
    charlieReceivedAliceSalt > 0,
    charlieReceivedBobSalt > 0,
    phase' = "derive",
    currentRound' = 0,
    // Keep all other state unchanged
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  // ==================== DERIVE PHASE ====================
  
  action aliceDeriveKey: bool = {
    // Compute points for all participants
    val alicePoint = computeDKDPoint(ROOT_KEY, context)
    val bobPoint = computeDKDPoint(ROOT_KEY, context)
    val charliePoint = computeDKDPoint(ROOT_KEY, context)
    
    // Verify commitments match revealed points
    val bobValid = verifyDKDReveal(aliceReceivedBobCommit, bobPoint)
    val charlieValid = verifyDKDReveal(aliceReceivedCharCommit, charliePoint)
    
    all {
      phase == "derive",
      aliceReceivedBobSalt > 0,
      aliceReceivedCharSalt > 0,
      bobValid,
      charlieValid,
      aliceDerivedKey' = aggregateDKDPoints(alicePoint, bobPoint, charliePoint),
      // Keep all other state unchanged
      phase' = phase,
      currentRound' = currentRound,
      aliceCommitted' = aliceCommitted,
      aliceSalt' = aliceSalt,
      aliceCommitment' = aliceCommitment,
      aliceReceivedBobCommit' = aliceReceivedBobCommit,
      aliceReceivedCharCommit' = aliceReceivedCharCommit,
      aliceReceivedBobSalt' = aliceReceivedBobSalt,
      aliceReceivedCharSalt' = aliceReceivedCharSalt,
      bobCommitted' = bobCommitted,
      bobSalt' = bobSalt,
      bobCommitment' = bobCommitment,
      bobDerivedKey' = bobDerivedKey,
      bobReceivedAliceCommit' = bobReceivedAliceCommit,
      bobReceivedCharCommit' = bobReceivedCharCommit,
      bobReceivedAliceSalt' = bobReceivedAliceSalt,
      bobReceivedCharSalt' = bobReceivedCharSalt,
      charlieCommitted' = charlieCommitted,
      charlieSalt' = charlieSalt,
      charlieCommitment' = charlieCommitment,
      charlieDerivedKey' = charlieDerivedKey,
      charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
      charlieReceivedBobCommit' = charlieReceivedBobCommit,
      charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
      charlieReceivedBobSalt' = charlieReceivedBobSalt,
      charlieByzantine' = charlieByzantine,
      appId' = appId,
      context' = context
    }
  }
  
  action bobDeriveKey: bool = {
    // Compute points for all participants
    val alicePoint = computeDKDPoint(ROOT_KEY, context)
    val bobPoint = computeDKDPoint(ROOT_KEY, context)
    val charliePoint = computeDKDPoint(ROOT_KEY, context)
    
    // Verify commitments match revealed points
    val aliceValid = verifyDKDReveal(bobReceivedAliceCommit, alicePoint)
    val charlieValid = verifyDKDReveal(bobReceivedCharCommit, charliePoint)
    
    all {
      phase == "derive",
      bobReceivedAliceSalt > 0,
      bobReceivedCharSalt > 0,
      aliceValid,
      charlieValid,
      bobDerivedKey' = aggregateDKDPoints(alicePoint, bobPoint, charliePoint),
      // Keep all other state unchanged
      phase' = phase,
      currentRound' = currentRound,
      aliceCommitted' = aliceCommitted,
      aliceSalt' = aliceSalt,
      aliceCommitment' = aliceCommitment,
      aliceDerivedKey' = aliceDerivedKey,
      aliceReceivedBobCommit' = aliceReceivedBobCommit,
      aliceReceivedCharCommit' = aliceReceivedCharCommit,
      aliceReceivedBobSalt' = aliceReceivedBobSalt,
      aliceReceivedCharSalt' = aliceReceivedCharSalt,
      bobCommitted' = bobCommitted,
      bobSalt' = bobSalt,
      bobCommitment' = bobCommitment,
      bobReceivedAliceCommit' = bobReceivedAliceCommit,
      bobReceivedCharCommit' = bobReceivedCharCommit,
      bobReceivedAliceSalt' = bobReceivedAliceSalt,
      bobReceivedCharSalt' = bobReceivedCharSalt,
      charlieCommitted' = charlieCommitted,
      charlieSalt' = charlieSalt,
      charlieCommitment' = charlieCommitment,
      charlieDerivedKey' = charlieDerivedKey,
      charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
      charlieReceivedBobCommit' = charlieReceivedBobCommit,
      charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
      charlieReceivedBobSalt' = charlieReceivedBobSalt,
      charlieByzantine' = charlieByzantine,
      appId' = appId,
      context' = context
    }
  }
  
  action charlieDeriveKey: bool = {
    // Compute points for all participants
    val alicePoint = computeDKDPoint(ROOT_KEY, context)
    val bobPoint = computeDKDPoint(ROOT_KEY, context)
    val charliePoint = computeDKDPoint(ROOT_KEY, context)
    
    // Verify commitments match revealed points
    val aliceValid = verifyDKDReveal(charlieReceivedAliceCommit, alicePoint)
    val bobValid = verifyDKDReveal(charlieReceivedBobCommit, bobPoint)
    
    all {
      phase == "derive",
      charlieReceivedAliceSalt > 0,
      charlieReceivedBobSalt > 0,
      aliceValid,
      bobValid,
      charlieDerivedKey' = aggregateDKDPoints(alicePoint, bobPoint, charliePoint),
      // Keep all other state unchanged
      phase' = phase,
      currentRound' = currentRound,
      aliceCommitted' = aliceCommitted,
      aliceSalt' = aliceSalt,
      aliceCommitment' = aliceCommitment,
      aliceDerivedKey' = aliceDerivedKey,
      aliceReceivedBobCommit' = aliceReceivedBobCommit,
      aliceReceivedCharCommit' = aliceReceivedCharCommit,
      aliceReceivedBobSalt' = aliceReceivedBobSalt,
      aliceReceivedCharSalt' = aliceReceivedCharSalt,
      bobCommitted' = bobCommitted,
      bobSalt' = bobSalt,
      bobCommitment' = bobCommitment,
      bobDerivedKey' = bobDerivedKey,
      bobReceivedAliceCommit' = bobReceivedAliceCommit,
      bobReceivedCharCommit' = bobReceivedCharCommit,
      bobReceivedAliceSalt' = bobReceivedAliceSalt,
      bobReceivedCharSalt' = bobReceivedCharSalt,
      charlieCommitted' = charlieCommitted,
      charlieSalt' = charlieSalt,
      charlieCommitment' = charlieCommitment,
      charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
      charlieReceivedBobCommit' = charlieReceivedBobCommit,
      charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
      charlieReceivedBobSalt' = charlieReceivedBobSalt,
      charlieByzantine' = charlieByzantine,
      appId' = appId,
      context' = context
    }
  }
  
  // Complete protocol when all have derived keys
  action transitionToDone: bool = all {
    phase == "derive",
    allDerived,
    phase' = "done",
    // Keep all other state unchanged
    currentRound' = currentRound,
    aliceCommitted' = aliceCommitted,
    aliceSalt' = aliceSalt,
    aliceCommitment' = aliceCommitment,
    aliceDerivedKey' = aliceDerivedKey,
    aliceReceivedBobCommit' = aliceReceivedBobCommit,
    aliceReceivedCharCommit' = aliceReceivedCharCommit,
    aliceReceivedBobSalt' = aliceReceivedBobSalt,
    aliceReceivedCharSalt' = aliceReceivedCharSalt,
    bobCommitted' = bobCommitted,
    bobSalt' = bobSalt,
    bobCommitment' = bobCommitment,
    bobDerivedKey' = bobDerivedKey,
    bobReceivedAliceCommit' = bobReceivedAliceCommit,
    bobReceivedCharCommit' = bobReceivedCharCommit,
    bobReceivedAliceSalt' = bobReceivedAliceSalt,
    bobReceivedCharSalt' = bobReceivedCharSalt,
    charlieCommitted' = charlieCommitted,
    charlieSalt' = charlieSalt,
    charlieCommitment' = charlieCommitment,
    charlieDerivedKey' = charlieDerivedKey,
    charlieReceivedAliceCommit' = charlieReceivedAliceCommit,
    charlieReceivedBobCommit' = charlieReceivedBobCommit,
    charlieReceivedAliceSalt' = charlieReceivedAliceSalt,
    charlieReceivedBobSalt' = charlieReceivedBobSalt,
    charlieByzantine' = charlieByzantine,
    appId' = appId,
    context' = context
  }
  
  // ==================== HELPERS AND INVARIANTS ====================
  
  pure def allCommitted: bool =
    aliceCommitted and bobCommitted and charlieCommitted
    
  pure def allHaveAllCommitments: bool =
    aliceReceivedBobCommit > 0 and aliceReceivedCharCommit > 0 and
    bobReceivedAliceCommit > 0 and bobReceivedCharCommit > 0 and
    charlieReceivedAliceCommit > 0 and charlieReceivedBobCommit > 0
  
  pure def allDerived: bool =
    aliceDerivedKey > 0 and bobDerivedKey > 0 and charlieDerivedKey > 0
  
  // Safety invariant: commitments come before reveals
  val CommitBeforeReveal = 
    (phase == "reveal" or phase == "derive") implies allHaveAllCommitments
  
  // Progress invariant
  val ProtocolProgress = 
    phase.in(Set("done", "failed")) or currentRound < TIMEOUT_ROUNDS
  
  // Main step action
  action step = any {
    setupProtocol,
    // Commitment phase
    aliceGenerateCommitment,
    bobGenerateCommitment,
    charlieGenerateCommitment,
    sendAliceCommitToBob,
    sendAliceCommitToCharlie,
    sendBobCommitToAlice,
    sendBobCommitToCharlie,
    sendCharlieCommitToAlice,
    sendCharlieCommitToBob,
    transitionToReveal,
    // Reveal phase
    sendAliceSaltToBob,
    sendAliceSaltToCharlie,
    sendBobSaltToAlice,
    sendBobSaltToCharlie,
    sendCharlieSaltToAlice,
    sendCharlieSaltToBob,
    transitionToDerive,
    // Derive phase
    aliceDeriveKey,
    bobDeriveKey,
    charlieDeriveKey,
    transitionToDone
  }
}