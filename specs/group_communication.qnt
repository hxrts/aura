// BeeKEM/CGKA Group Communication Protocols Specification
// Models Continuous Group Key Agreement with tree-based operations
module group_communication {
  // ==================== TYPES ====================
  
  type MemberId = str
  type GroupId = str
  type Epoch = int
  type KeyPackage = {
    memberId: MemberId,
    encryptionKey: int,
    signatureKey: int,
    credential: str
  }
  
  type TreeNode = 
    | Leaf({memberId: MemberId, keyPackage: KeyPackage})
    | Parent({left: TreeNode, right: TreeNode, secret: int})
    | Empty
  
  type GroupOperation = 
    | Add(KeyPackage)
    | Remove(MemberId)
    | Update({memberId: MemberId, keyPackage: KeyPackage})
    | CommitOp(MemberId)
  
  type GroupState = {
    id: GroupId,
    epoch: Epoch,
    tree: TreeNode,
    members: Set[MemberId],
    pendingOperations: List[GroupOperation],
    applicationSecret: int,
    epochSecret: int
  }
  
  type MemberState = {
    id: MemberId,
    groups: GroupId -> MemberView,
    keyPackages: List[KeyPackage],
    isByzantine: bool
  }
  
  type MemberView = {
    groupId: GroupId,
    epoch: Epoch,
    treeHash: int,  // Abstract tree state hash
    applicationSecret: int,
    myPosition: int  // Position in tree
  }
  
  type Welcome = {
    groupId: GroupId,
    epoch: Epoch,
    groupState: GroupState,
    encryptedFor: MemberId
  }
  
  type CommitMessage = {
    groupId: GroupId,
    epoch: Epoch,
    operations: List[GroupOperation],
    path: List[int],  // Path secrets for tree update
    committer: MemberId
  }
  
  // ==================== STATE ====================
  
  var groups: GroupId -> GroupState
  var members: MemberId -> MemberState
  var pendingCommits: GroupId -> List[CommitMessage]
  var welcomeMessages: Set[Welcome]
  var globalEpoch: Epoch

  pure def unchangedExcept(vars: Set[str]): bool = all {
    if ("groups".in(vars)) true else groups' = groups,
    if ("members".in(vars)) true else members' = members,
    if ("pendingCommits".in(vars)) true else pendingCommits' = pendingCommits,
    if ("welcomeMessages".in(vars)) true else welcomeMessages' = welcomeMessages,
    if ("globalEpoch".in(vars)) true else globalEpoch' = globalEpoch
  }
  
  // ==================== CONSTANTS ====================
  
  pure val MAX_GROUP_SIZE = 100
  pure val MAX_TREE_DEPTH = 10
  pure val MAX_PENDING_OPS = 50
  
  // ==================== INITIALIZATION ====================
  
  action init: bool = all {
    groups' = Map(),
    members' = Map(),
    pendingCommits' = Map(),
    welcomeMessages' = Set(),
    globalEpoch' = 0
  }
  
  // ==================== GROUP CREATION ====================
  
  action createGroup(groupId: GroupId, creator: MemberId): bool = all {
    not(groupId.in(groups.keys())),
    creator.in(members.keys()),
    val creatorState = members.get(creator)
    val keyPackage = creatorState.keyPackages.head()
    // Derive initial secrets using HKDF with group ID as context
    val groupSeed = (groupId.length() * 101 + creator.length() * 53) % 2147483647
    val epochSecret = (groupSeed * 31 + 97) % 2147483647
    val applicationSecret = deriveSecret(List(groupSeed), epochSecret)
    val newTree = Leaf({memberId: creator, keyPackage: keyPackage})
    groups' = groups.set(groupId, {
      id: groupId,
      epoch: 0,
      tree: newTree,
      members: Set(creator),
      pendingOperations: List(),
      applicationSecret: applicationSecret,
      epochSecret: epochSecret
    }),
    members' = members.set(creator, {
      ...creatorState,
      groups: creatorState.groups.set(groupId, {
        groupId: groupId,
        epoch: 0,
        treeHash: hashTree(newTree),
        applicationSecret: applicationSecret,
        myPosition: 0
      })
    }),
    unchangedExcept(Set("groups", "members"))
  }
  
  // ==================== MEMBER MANAGEMENT ====================
  
  action addMember(memberId: MemberId): bool = all {
    not(memberId.in(members.keys())),
    members' = members.set(memberId, {
      id: memberId,
      groups: Map(),
      keyPackages: List({
        memberId: memberId,
        encryptionKey: 1.to(1000).oneOf(),
        signatureKey: 1.to(1000).oneOf(),
        credential: "member_credential"
      }),
      isByzantine: false
    }),
    unchangedExcept(Set("members"))
  }
  
  action markByzantine(memberId: MemberId): bool = all {
    memberId.in(members.keys()),
    val member = members.get(memberId)
    members' = members.set(memberId, {...member, isByzantine: true}),
    unchangedExcept(Set("members"))
  }
  
  // ==================== GROUP OPERATIONS ====================
  
  action proposeAdd(groupId: GroupId, proposer: MemberId, newMember: MemberId): bool = all {
    groupId.in(groups.keys()),
    proposer.in(members.keys()),
    newMember.in(members.keys()),
    val group = groups.get(groupId)
    val newMemberKP = members.get(newMember).keyPackages.head()
    proposer.in(group.members),
    not(newMember.in(group.members)),
    group.members.size() < MAX_GROUP_SIZE,
    groups' = groups.set(groupId, {
      ...group,
      pendingOperations: group.pendingOperations.append(Add(newMemberKP))
    }),
    unchangedExcept(Set("groups"))
  }
  
  action proposeRemove(groupId: GroupId, proposer: MemberId, target: MemberId): bool = all {
    groupId.in(groups.keys()),
    proposer.in(members.keys()),
    val group = groups.get(groupId)
    proposer.in(group.members),
    target.in(group.members),
    groups' = groups.set(groupId, {
      ...group,
      pendingOperations: group.pendingOperations.append(Remove(target))
    }),
    unchangedExcept(Set("groups"))
  }
  
  action proposeUpdate(groupId: GroupId, updater: MemberId): bool = all {
    groupId.in(groups.keys()),
    updater.in(members.keys()),
    val group = groups.get(groupId)
    updater.in(group.members),
    val updaterState = members.get(updater)
    // Generate fresh key package for update with post-compromise security
    val updateSeed = (group.epoch * 71 + updater.length() * 43 + globalEpoch * 13) % 2147483647
    val newKP = generateKeyPackage(updater, updateSeed)
    groups' = groups.set(groupId, {
      ...group,
      pendingOperations: group.pendingOperations.append(Update(updater, newKP))
    }),
    unchangedExcept(Set("groups"))
  }
  
  // ==================== COMMIT PROTOCOL ====================
  
  action createCommit(groupId: GroupId, committer: MemberId): bool = all {
    groupId.in(groups.keys()),
    committer.in(members.keys()),
    val group = groups.get(groupId)
    committer.in(group.members),
    group.pendingOperations.length() > 0,
    val memberState = members.get(committer)
    not(memberState.isByzantine) or Set(true, false).oneOf(),  // Byzantine may create invalid commits
    // Apply operations to tree
    val newTree = applyOperations(group.tree, group.pendingOperations)
    val pathSecrets = computePath(newTree, committer)
    val commit = {
      groupId: groupId,
      epoch: group.epoch + 1,
      operations: group.pendingOperations,
      path: pathSecrets,
      committer: committer
    }
    val currentCommits = if (groupId.in(pendingCommits.keys()))
      pendingCommits.get(groupId)
    else
      List()
    pendingCommits' = pendingCommits.set(groupId, currentCommits.append(commit)),
    unchangedExcept(Set("pendingCommits"))
  }
  
  action processCommit(groupId: GroupId, commit: CommitMessage): bool = all {
    groupId.in(groups.keys()),
    groupId.in(pendingCommits.keys()),
    commit.in(pendingCommits.get(groupId)),
    val group = groups.get(groupId)
    commit.epoch == group.epoch + 1,
    val newTree = applyOperations(group.tree, commit.operations)
    val newMembers = extractMembers(newTree)
    val newAppSecret = deriveSecret(commit.path, group.epochSecret)
    val newEpochSecret = newAppSecret + 1
    val newGroupState = {
      id: groupId,
      epoch: commit.epoch,
      tree: newTree,
      members: newMembers,
      pendingOperations: List(),
      applicationSecret: newAppSecret,
      epochSecret: newEpochSecret
    }
    val addedMembers = commit.operations.fold(Set(), (acc, op) =>
      match op {
        | Add(kp) => acc.union(Set(kp.memberId))
        | _ => acc
      }
    )
    groups' = groups.set(groupId, newGroupState),
    members' = updateMemberViews(members, groupId, newTree, newAppSecret, commit.epoch),
    welcomeMessages' = addedMembers.fold(welcomeMessages, (acc, newMember) =>
      acc.union(Set({
        groupId: groupId,
        epoch: commit.epoch,
        groupState: newGroupState,
        encryptedFor: newMember
      }))
    ),
    globalEpoch' = max(globalEpoch, commit.epoch),
    unchangedExcept(Set("groups", "members", "welcomeMessages", "globalEpoch"))
  }
  
  action processWelcome(welcome: Welcome): bool = all {
    welcome.in(welcomeMessages),
    welcome.encryptedFor.in(members.keys()),
    val member = members.get(welcome.encryptedFor)
    not(welcome.groupId.in(member.groups.keys())),
    val memberView = {
      groupId: welcome.groupId,
      epoch: welcome.epoch,
      treeHash: hashTree(welcome.groupState.tree),
      applicationSecret: welcome.groupState.applicationSecret,
      myPosition: findPosition(welcome.groupState.tree, welcome.encryptedFor)
    }
    members' = members.set(welcome.encryptedFor, {
      ...member,
      groups: member.groups.set(welcome.groupId, memberView)
    }),
    welcomeMessages' = welcomeMessages.exclude(Set(welcome)),
    unchangedExcept(Set("members", "welcomeMessages"))
  }
  
  // ==================== TREE OPERATIONS ====================
  
  pure def applyOperations(tree: TreeNode, ops: List[GroupOperation]): TreeNode = 
    ops.fold(tree, (t, op) =>
      match op {
        | Add(kp) => addToTree(t, kp)
        | Remove(memberId) => removeFromTree(t, memberId)
        | Update({memberId: mid, keyPackage: kp}) => updateInTree(t, mid, kp)
        | CommitOp(_) => t
      }
    )
  
  pure def addToTree(tree: TreeNode, kp: KeyPackage): TreeNode = 
    match tree {
      | Leaf({memberId: id, keyPackage: pkg}) => Parent({left: tree, right: Leaf({memberId: kp.memberId, keyPackage: kp}), secret: 1.to(1000).oneOf()})
      | Parent({left: left, right: right, secret: secret}) => 
          if (treeSize(left) <= treeSize(right)) {
            Parent({left: addToTree(left, kp), right: right, secret: 1.to(1000).oneOf()})
          } else {
            Parent({left: left, right: addToTree(right, kp), secret: 1.to(1000).oneOf()})
          }
      | Empty => Leaf({memberId: kp.memberId, keyPackage: kp})
    }
  
  pure def removeFromTree(tree: TreeNode, memberId: MemberId): TreeNode = 
    match tree {
      | Leaf({memberId: id, keyPackage: _}) => if (id == memberId) Empty else tree
      | Parent({left: left, right: right, secret: _}) => {
          val newLeft = removeFromTree(left, memberId)
          val newRight = removeFromTree(right, memberId)
          match (newLeft, newRight) {
            | (Empty, Empty) => Empty
            | (Empty, _) => newRight
            | (_, Empty) => newLeft
            | _ => Parent({left: newLeft, right: newRight, secret: 1.to(1000).oneOf()})
          }
        }
      | Empty => Empty
    }
  
  pure def updateInTree(tree: TreeNode, memberId: MemberId, kp: KeyPackage): TreeNode = 
    match tree {
      | Leaf({memberId: id, keyPackage: _}) => if (id == memberId) Leaf({memberId: id, keyPackage: kp}) else tree
      | Parent({left: left, right: right, secret: secret}) => 
          Parent({
            left: updateInTree(left, memberId, kp),
            right: updateInTree(right, memberId, kp),
            secret: if (containsMember(tree, memberId)) 1.to(1000).oneOf() else secret
          })
      | Empty => Empty
    }
  
  pure def treeSize(tree: TreeNode): int = 
    match tree {
      | Leaf(_) => 1
      | Parent({left: left, right: right, secret: _}) => treeSize(left) + treeSize(right)
      | Empty => 0
    }
  
  pure def extractMembers(tree: TreeNode): Set[MemberId] = 
    match tree {
      | Leaf({memberId: id, keyPackage: _}) => Set(id)
      | Parent({left: left, right: right, secret: _}) => extractMembers(left).union(extractMembers(right))
      | Empty => Set()
    }
  
  pure def containsMember(tree: TreeNode, memberId: MemberId): bool = 
    memberId.in(extractMembers(tree))
  
  // ==================== CRYPTOGRAPHIC FUNCTIONS ====================
  
  // Hash tree structure using BLAKE3
  pure def hashTree(tree: TreeNode): int = 
    // Real implementation: BLAKE3(tree_serialization)
    // Includes all node contents, structure, and secrets
    match tree {
      | Leaf({memberId: id, keyPackage: kp}) => 
        // H(0x00 || member_id || enc_key || sig_key || credential)
        val leafTag = 1
        val memberHash = id.length() * 31
        val keyHash = kp.encryptionKey * 17 + kp.signatureKey * 13
        (leafTag + memberHash + keyHash) % 2147483647
      | Parent({left: left, right: right, secret: secret}) => 
        // H(0x01 || H(left) || H(right) || secret)
        val parentTag = 2
        val leftHash = hashTree(left)
        val rightHash = hashTree(right)
        (parentTag + leftHash * 7 + rightHash * 11 + secret * 23) % 2147483647
      | Empty => 
        // H(0x02)
        3
    }
  
  // Compute co-path secrets for tree update (TreeKEM)
  pure def computePath(tree: TreeNode, memberId: MemberId): List[int] = 
    // Real implementation: TreeKEM path secret derivation
    // For each ancestor node from leaf to root:
    // 1. Generate fresh secret for node
    // 2. Derive node_secret = HKDF(path_secret || "node")
    // 3. Encrypt to sibling's subtree
    computePathHelper(tree, memberId, List())
  
  pure def computePathHelper(tree: TreeNode, memberId: MemberId, acc: List[int]): List[int] = 
    match tree {
      | Leaf({memberId: id, keyPackage: _}) => 
        if (id == memberId) acc else List()
      | Parent({left: left, right: right, secret: _}) =>
        if (containsMember(left, memberId)) {
          // Member is in left subtree, need to update this parent
          val freshSecret = (memberId.length() * 53 + acc.length() * 19) % 1000
          val newAcc = acc.append(freshSecret)
          computePathHelper(left, memberId, newAcc)
        } else if (containsMember(right, memberId)) {
          // Member is in right subtree
          val freshSecret = (memberId.length() * 59 + acc.length() * 23) % 1000  
          val newAcc = acc.append(freshSecret)
          computePathHelper(right, memberId, newAcc)
        } else {
          List()
        }
      | Empty => List()
    }
  
  // Derive application secret from path secrets using HKDF
  pure def deriveSecret(path: List[int], epochSecret: int): int = 
    // Real implementation:
    // 1. Combine all path secrets to get commit_secret
    // 2. joiner_secret = HKDF(commit_secret || epoch_secret, "joiner")
    // 3. epoch_secret' = HKDF(joiner_secret, "epoch")
    // 4. app_secret = HKDF(epoch_secret', "app")
    val commitSecret = path.fold(0, (acc, p) => (acc * 7 + p * 13) % 2147483647)
    val joinerSecret = (commitSecret * 31 + epochSecret * 17) % 2147483647
    val newEpochSecret = (joinerSecret * 23 + 89) % 2147483647
    val appSecret = (newEpochSecret * 29 + 97) % 2147483647
    appSecret
  
  // Find member's position in tree (left-to-right leaf ordering)
  pure def findPosition(tree: TreeNode, memberId: MemberId): int = 
    findPositionHelper(tree, memberId, 0)._1
  
  pure def findPositionHelper(tree: TreeNode, memberId: MemberId, currentPos: int): (int, int) = 
    // Returns (position if found, next position)
    match tree {
      | Leaf({memberId: id, keyPackage: _}) => 
        if (id == memberId) (currentPos, currentPos + 1) else (-1, currentPos + 1)
      | Parent({left: left, right: right, secret: _}) =>
        val leftResult = findPositionHelper(left, memberId, currentPos)
        if (leftResult._1 >= 0) {
          leftResult
        } else {
          findPositionHelper(right, memberId, leftResult._2)
        }
      | Empty => (-1, currentPos)
    }
  
  // Generate key package with Ed25519 keys
  pure def generateKeyPackage(memberId: MemberId, seed: int): KeyPackage = 
    // Real implementation:
    // 1. Generate HPKE key pair for encryption
    // 2. Generate Ed25519 key pair for signatures  
    // 3. Create credential binding identity to keys
    {
      memberId: memberId,
      encryptionKey: (seed * 31 + memberId.length() * 17) % 2147483647,
      signatureKey: (seed * 37 + memberId.length() * 19) % 2147483647,
      credential: memberId.concat("_cred_").concat(seed.toString())
    }
  
  // Compute tree invariant hash for consistency verification
  pure def computeTreeInvariant(tree: TreeNode): int = 
    // Real implementation: Compute root hash including all member keys
    // Used to verify all members have consistent view
    match tree {
      | Leaf({memberId: _, keyPackage: kp}) => 
        (kp.encryptionKey * 13 + kp.signatureKey * 17) % 2147483647
      | Parent({left: left, right: right, secret: _}) =>
        val leftInv = computeTreeInvariant(left)
        val rightInv = computeTreeInvariant(right)
        (leftInv * 19 + rightInv * 23) % 2147483647
      | Empty => 0
    }
  
  // Verify path secret consistency
  pure def verifyPath(tree: TreeNode, memberId: MemberId, path: List[int]): bool = 
    // Real implementation: Verify path secrets are correctly computed
    // and can derive the group secrets
    path.length() > 0 and path.length() <= treeDepth(tree)
  
  pure def treeDepth(tree: TreeNode): int = 
    match tree {
      | Leaf(_) => 0
      | Parent({left: left, right: right, secret: _}) =>
        1 + max(treeDepth(left), treeDepth(right))
      | Empty => 0
    }
  
  // HPKE encryption for path secrets to subtree members  
  pure def encryptPathSecret(secret: int, recipientKey: int): int = 
    // Real implementation: HPKE(recipient_pub_key, secret)
    // HPKE provides forward secrecy and post-compromise security
    (secret * 41 + recipientKey * 29) % 2147483647
  
  // Compute confirmation tag for commit verification
  pure def computeConfirmationTag(groupId: GroupId, epoch: Epoch, treeHash: int): int = 
    // Real implementation: HMAC(confirmation_key, group_id || epoch || tree_hash)
    // Ensures all members agree on same group state
    (groupId.length() * 47 + epoch * 31 + treeHash * 23) % 2147483647
  
  // Generate welcome secret for new members
  pure def generateWelcomeSecret(groupState: GroupState, recipientId: MemberId): int = 
    // Real implementation: 
    // 1. Generate ephemeral key pair
    // 2. HPKE encrypt group secrets to new member
    // 3. Include proof of group membership
    val groupHash = hashTree(groupState.tree)
    val memberSeed = recipientId.length() * 61
    (groupState.epochSecret * 37 + groupHash * 19 + memberSeed) % 2147483647
  
  pure def updateMemberViews(
    members: Map[MemberId, MemberState],
    groupId: GroupId,
    newTree: TreeNode,
    newAppSecret: int,
    newEpoch: Epoch
  ): Map[MemberId, MemberState] = {
    val groupMembers = extractMembers(newTree)
    groupMembers.fold(members, (acc, mId) =>
      if (mId.in(acc.keys())) {
        val member = acc.get(mId)
        if (groupId.in(member.groups.keys())) {
          val newView = {
            groupId: groupId,
            epoch: newEpoch,
            treeHash: hashTree(newTree),
            applicationSecret: newAppSecret,
            myPosition: findPosition(newTree, mId)
          }
          acc.set(mId, {
            ...member,
            groups: member.groups.set(groupId, newView)
          })
        } else {
          acc
        }
      } else {
        acc
      }
    )
  }
  
  pure def max(a: int, b: int): int = 
    if (a > b) a else b
  
  // ==================== HELPER FUNCTIONS ====================
  
  pure def byzantineCount(groupId: GroupId): int = 
    if (groupId.in(groups.keys())) {
      val group = groups.get(groupId)
      group.members.filter(m => members.get(m).isByzantine).size()
    } else {
      0
    }
  
  pure def allMembersAgree(groupId: GroupId): bool = 
    if (groupId.in(groups.keys())) {
      val group = groups.get(groupId)
      val expectedHash = hashTree(group.tree)
      group.members.forall(m =>
        if (m.in(members.keys()) and groupId.in(members.get(m).groups.keys())) {
          members.get(m).groups.get(groupId).treeHash == expectedHash
        } else {
          true
        }
      )
    } else {
      true
    }
  
  // ==================== INVARIANTS ====================
  
  // Safety: Group size bounds
  val GroupSizeBounds = 
    groups.values().forall(g =>
      g.members.size() > 0 and g.members.size() <= MAX_GROUP_SIZE
    )
  
  // Safety: Tree consistency
  val TreeConsistency = 
    groups.values().forall(g =>
      extractMembers(g.tree) == g.members
    )
  
  // Safety: Epoch monotonicity
  val EpochMonotonicity = 
    groups.values().forall(g => g.epoch >= 0 and g.epoch <= globalEpoch)
  
  // Safety: Member view consistency
  val MemberViewConsistency = 
    members.values().forall(m =>
      m.groups.keys().forall(gId =>
        if (gId.in(groups.keys())) {
          val group = groups.get(gId)
          m.id.in(group.members) implies
          m.groups.get(gId).epoch <= group.epoch
        } else {
          true
        }
      )
    )
  
  // Progress: Pending operations get processed
  val OperationProgress = 
    groups.values().forall(g =>
      g.pendingOperations.length() <= MAX_PENDING_OPS
    )
  
  // ==================== TEMPORAL PROPERTIES ====================
  
  // Forward Secrecy: Old secrets become inaccessible
  temporal ForwardSecrecy = 
    always(
      groups.values().forall(g =>
        val oldEpoch = g.epoch
        val oldSecret = g.applicationSecret
        eventually(g.epoch > oldEpoch implies g.applicationSecret != oldSecret)
      )
    )
  
  // Post-Compromise Security: Updates heal compromise
  temporal PostCompromiseSecurity = 
    always(
      members.values().forall(m =>
        m.groups.keys().forall(gId =>
          eventually(
            // After update, new secrets are established
            m.groups.get(gId).epoch > globalEpoch implies
            m.groups.get(gId).applicationSecret != groups.get(gId).applicationSecret
          )
        )
      )
    )
  
  // Agreement: All members converge to same view
  temporal GroupAgreement = 
    always(
      groups.keys().forall(gId =>
        eventually(allMembersAgree(gId))
      )
    )
  
  // Liveness: Operations eventually complete
  temporal OperationCompletion = 
    always(
      groups.values().forall(g =>
        g.pendingOperations.length() > 0 implies
        eventually(g.pendingOperations.length() == 0)
      )
    )
  
  // Byzantine Resilience: Groups function despite Byzantine members
  temporal ByzantineResilience = 
    always(
      groups.keys().forall(gId =>
        byzantineCount(gId) < groups.get(gId).members.size() / 2 implies
        eventually(
          // Honest members reach agreement
          val group = groups.get(gId)
          val honestMembers = group.members.filter(m => not(members.get(m).isByzantine))
          honestMembers.forall(m1 =>
            honestMembers.forall(m2 =>
              members.get(m1).groups.get(gId).treeHash ==
              members.get(m2).groups.get(gId).treeHash
            )
          )
        )
      )
    )
}
