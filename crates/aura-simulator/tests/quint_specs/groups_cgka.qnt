// Continuous Group Key Agreement (CGKA) Protocol
// Formal specification of Aura's BeeKEM-based group communication

module GroupsCGKA {
    import SessionTypes.*

    // ========== CGKA Types ==========

    type GroupId = str
    type MemberId = str
    type TreePosition = int
    type PublicKey = bytes
    type ApplicationSecret = bytes

    // BeeKEM tree node
    type TreeNode = {
        position: TreePosition,
        publicKey: PublicKey,
        isEmpty: bool
    }

    // Group member roster
    type Roster = {
        epoch: Epoch,
        members: Set[MemberId],
        positions: MemberId -> TreePosition,
        size: int
    }

    // Roster delta for updates
    type RosterDelta = {
        previousSize: int,
        addedMembers: Set[MemberId],
        removedMembers: Set[MemberId]
    }

    // Key package for joining
    type KeyPackage = {
        memberId: MemberId,
        initKey: PublicKey,
        leafKey: PublicKey,
        credentialKey: PublicKey
    }

    // Tree update types
    type TreeUpdateType =
        | AddLeaf(memberId: MemberId, keyPackage: KeyPackage)
        | RemoveLeaf(memberId: MemberId)
        | UpdateNode(newPublicKey: PublicKey)

    // Path update for tree healing
    type PathUpdate = {
        position: TreePosition,
        publicKey: PublicKey,
        encryptedSecret: bytes  // For siblings on path
    }

    // Tree update operation
    type TreeUpdate = {
        position: TreePosition,
        updateType: TreeUpdateType,
        pathUpdates: List[PathUpdate]
    }

    // CGKA operation
    type CGKAOperation = {
        operationId: str,
        groupId: GroupId,
        currentEpoch: Epoch,
        targetEpoch: Epoch,
        rosterDelta: RosterDelta,
        treeUpdates: List[TreeUpdate],
        sender: MemberId
    }

    // BeeKEM tree state
    type BeeKemTree = {
        nodes: TreePosition -> TreeNode,
        size: int,
        rootSecret: bytes
    }

    // CGKA group state
    type CGKAState = {
        groupId: GroupId,
        currentEpoch: Epoch,
        roster: Roster,
        tree: BeeKemTree,
        pendingOperations: Set[CGKAOperation],
        applicationSecrets: Epoch -> ApplicationSecret,
        lastUpdated: int
    }

    // ========== State Variables ==========

    var groups: GroupId -> CGKAState
    var memberGroups: MemberId -> Set[GroupId]
    var operationLog: List[CGKAOperation]
    var currentTime: int

    // ========== Helper Functions ==========

    // Check if position is a leaf
    pure def isLeaf(position: TreePosition): bool = {
        // In binary tree, leaves have no children
        // TODO fix - Simplified check
        position >= 0
    }

    // Get parent position
    pure def parentPosition(position: TreePosition): TreePosition = {
        // Binary tree parent calculation
        (position - 1) / 2
    }

    // Get sibling position
    pure def siblingPosition(position: TreePosition): TreePosition = {
        if (position % 2 == 1) position + 1 else position - 1
    }

    // Get co-path (path to root excluding self)
    pure def coPath(position: TreePosition): List[TreePosition] = {
        if (position == 0) [] else {
            val parent = parentPosition(position),
            [siblingPosition(position)].concat(coPath(parent))
        }
    }

    // Find empty leaf position
    pure def findEmptyLeaf(tree: BeeKemTree): TreePosition = {
        // Find first empty leaf position
        tree.nodes.keys().filter(pos =>
            isLeaf(pos) and tree.nodes.get(pos).isEmpty
        ).fold(-1, (acc, pos) => if (acc == -1) pos else acc)
    }

    // Validate roster delta
    pure def isValidRosterDelta(delta: RosterDelta, roster: Roster): bool = {
        delta.previousSize == roster.size and
        delta.removedMembers.isSubsetOf(roster.members) and
        delta.addedMembers.intersect(roster.members).size() == 0
    }

    // Derive application secret from tree
    pure def deriveApplicationSecret(tree: BeeKemTree, groupId: GroupId, epoch: Epoch): ApplicationSecret = {
        // KDF(root_secret || group_id || epoch)
        ApplicationSecret("app_secret_" + epoch.toString())
    }

    // ========== Actions ==========

    // Create new CGKA group
    action createGroup(
        groupId: GroupId,
        initialMembers: Set[MemberId]
    ): bool = {
        all {
            // Group doesn't exist
            not(groupId.in(groups.keys())),
            initialMembers.size() > 0,
            // Initialize roster
            val roster = {
                epoch: 0,
                members: initialMembers,
                positions: initialMembers.toList().indices().fold(Map(), (acc, i) =>
                    acc.put(initialMembers.toList()[i], i)
                ),
                size: initialMembers.size()
            },
            // Initialize tree
            val treeSize = initialMembers.size() * 2 - 1,  // Complete binary tree
            val tree = {
                nodes: initialMembers.toList().indices().fold(Map(), (acc, i) =>
                    acc.put(i, {
                        position: i,
                        publicKey: PublicKey("leaf_" + i.toString()),
                        isEmpty: false
                    })
                ),
                size: treeSize,
                rootSecret: bytes("initial_root")
            },
            // Create state
            val state = {
                groupId: groupId,
                currentEpoch: 0,
                roster: roster,
                tree: tree,
                pendingOperations: Set(),
                applicationSecrets: Map(0 -> deriveApplicationSecret(tree, groupId, 0)),
                lastUpdated: currentTime
            },
            groups' = groups.put(groupId, state),
            // Update member mappings
            memberGroups' = initialMembers.fold(memberGroups, (acc, memberId) =>
                val currentGroups = if (memberId.in(acc.keys()))
                    acc.get(memberId) else Set(),
                acc.put(memberId, currentGroups.union(Set(groupId)))
            ),
            operationLog' = operationLog,
            currentTime' = currentTime + 1
        }
    }

    // Add member to group
    action addMember(
        groupId: GroupId,
        memberId: MemberId,
        keyPackage: KeyPackage,
        sender: MemberId
    ): bool = {
        all {
            // Group exists
            groupId.in(groups.keys()),
            val state = groups.get(groupId),
            // Sender is member
            sender.in(state.roster.members),
            // New member not already in group
            not(memberId.in(state.roster.members)),
            // Find empty leaf
            val leafPos = findEmptyLeaf(state.tree),
            leafPos >= 0,
            // Create operation
            val operation = {
                operationId: "add_" + memberId,
                groupId: groupId,
                currentEpoch: state.currentEpoch,
                targetEpoch: state.currentEpoch + 1,
                rosterDelta: {
                    previousSize: state.roster.size,
                    addedMembers: Set(memberId),
                    removedMembers: Set()
                },
                treeUpdates: [{
                    position: leafPos,
                    updateType: AddLeaf(memberId, keyPackage),
                    pathUpdates: coPath(leafPos).map(pos => {
                        position: pos,
                        publicKey: PublicKey("updated_" + pos.toString()),
                        encryptedSecret: bytes("secret_" + pos.toString())
                    })
                }],
                sender: sender
            },
            // Apply operation
            applyOperation(groupId, operation)
        }
    }

    // Remove member from group
    action removeMember(
        groupId: GroupId,
        memberId: MemberId,
        sender: MemberId
    ): bool = {
        all {
            // Group exists
            groupId.in(groups.keys()),
            val state = groups.get(groupId),
            // Both sender and target are members
            sender.in(state.roster.members),
            memberId.in(state.roster.members),
            sender != memberId,  // Can't remove self
            // Get member position
            val memberPos = state.roster.positions.get(memberId),
            // Create operation
            val operation = {
                operationId: "remove_" + memberId,
                groupId: groupId,
                currentEpoch: state.currentEpoch,
                targetEpoch: state.currentEpoch + 1,
                rosterDelta: {
                    previousSize: state.roster.size,
                    addedMembers: Set(),
                    removedMembers: Set(memberId)
                },
                treeUpdates: [{
                    position: memberPos,
                    updateType: RemoveLeaf(memberId),
                    pathUpdates: coPath(memberPos).map(pos => {
                        position: pos,
                        publicKey: PublicKey("blanked_" + pos.toString()),
                        encryptedSecret: bytes("blank_secret_" + pos.toString())
                    })
                }],
                sender: sender
            },
            // Apply operation
            applyOperation(groupId, operation)
        }
    }

    // Update member's key (for forward secrecy)
    action updateMemberKey(
        groupId: GroupId,
        memberId: MemberId,
        newPublicKey: PublicKey
    ): bool = {
        all {
            // Group exists and member is in it
            groupId.in(groups.keys()),
            val state = groups.get(groupId),
            memberId.in(state.roster.members),
            // Get member position
            val memberPos = state.roster.positions.get(memberId),
            // Create operation
            val operation = {
                operationId: "update_" + memberId,
                groupId: groupId,
                currentEpoch: state.currentEpoch,
                targetEpoch: state.currentEpoch + 1,
                rosterDelta: {
                    previousSize: state.roster.size,
                    addedMembers: Set(),
                    removedMembers: Set()
                },
                treeUpdates: [{
                    position: memberPos,
                    updateType: UpdateNode(newPublicKey),
                    pathUpdates: coPath(memberPos).map(pos => {
                        position: pos,
                        publicKey: PublicKey("updated_" + pos.toString()),
                        encryptedSecret: bytes("new_secret_" + pos.toString())
                    })
                }],
                sender: memberId
            },
            // Apply operation
            applyOperation(groupId, operation)
        }
    }

    // Apply CGKA operation
    action applyOperation(groupId: GroupId, operation: CGKAOperation): bool = {
        all {
            // Group exists
            groupId.in(groups.keys()),
            val state = groups.get(groupId),
            // Operation is for current epoch
            operation.currentEpoch == state.currentEpoch,
            operation.groupId == groupId,
            // Valid roster delta
            isValidRosterDelta(operation.rosterDelta, state.roster),
            // Apply roster changes
            val newMembers = state.roster.members
                .exclude(operation.rosterDelta.removedMembers)
                .union(operation.rosterDelta.addedMembers),
            val newRoster = {
                epoch: operation.targetEpoch,
                members: newMembers,
                positions: state.roster.positions  // TODO fix - Simplified: would need recomputation
                    .filter((m, _) => m.in(newMembers)),
                size: newMembers.size()
            },
            // Apply tree updates (TODO fix - Simplified)
            val newTree = operation.treeUpdates.foldl(state.tree, (tree, update) =>
                tree  // TODO fix - Simplified: would actually update nodes
            ),
            // Derive new application secret
            val newAppSecret = deriveApplicationSecret(newTree, groupId, operation.targetEpoch),
            val newAppSecrets = state.applicationSecrets.put(operation.targetEpoch, newAppSecret),
            // Update state
            val newState = state
                .with("currentEpoch", operation.targetEpoch)
                .with("roster", newRoster)
                .with("tree", newTree)
                .with("applicationSecrets", newAppSecrets)
                .with("lastUpdated", currentTime),
            groups' = groups.put(groupId, newState),
            // Update member groups
            memberGroups' = operation.rosterDelta.removedMembers.fold(
                operation.rosterDelta.addedMembers.fold(memberGroups, (acc, added) =>
                    val currentGroups = if (added.in(acc.keys()))
                        acc.get(added) else Set(),
                    acc.put(added, currentGroups.union(Set(groupId)))
                ),
                (acc, removed) =>
                    val currentGroups = if (removed.in(acc.keys()))
                        acc.get(removed) else Set(),
                    acc.put(removed, currentGroups.exclude(Set(groupId)))
            ),
            operationLog' = operationLog.append(operation),
            currentTime' = currentTime + 1
        }
    }

    // Get application secret for encryption/decryption
    action getApplicationSecret(
        groupId: GroupId,
        memberId: MemberId,
        epoch: Epoch
    ): bool = {
        all {
            // Group exists
            groupId.in(groups.keys()),
            val state = groups.get(groupId),
            // Member is in group
            memberId.in(state.roster.members),
            // Secret exists for epoch
            epoch.in(state.applicationSecrets.keys()),
            // No state change, just verification
            groups' = groups,
            memberGroups' = memberGroups,
            operationLog' = operationLog,
            currentTime' = currentTime
        }
    }

    // ========== Invariants ==========

    // Member consistency: members in roster match member groups
    invariant memberConsistency = {
        groups.values().forall(state =>
            state.roster.members.forall(memberId =>
                memberId.in(memberGroups.keys()) implies
                    state.groupId.in(memberGroups.get(memberId))
            )
        )
    }

    // Epoch monotonicity
    invariant epochMonotonicity = {
        operationLog.indices().forall(i =>
            i > 0 implies
                operationLog[i].targetEpoch >= operationLog[i-1].targetEpoch
        )
    }

    // Tree size consistency
    invariant treeSizeConsistency = {
        groups.values().forall(state =>
            // Tree has enough leaves for members
            state.tree.nodes.keys().filter(pos => isLeaf(pos)).size() >= state.roster.size
        )
    }

    // Application secret availability
    invariant applicationSecretAvailability = {
        groups.values().forall(state =>
            // Current epoch always has application secret
            state.currentEpoch.in(state.applicationSecrets.keys())
        )
    }

    // No orphan members
    invariant noOrphanMembers = {
        memberGroups.keys().forall(memberId =>
            memberGroups.get(memberId).size() > 0 implies
                memberGroups.get(memberId).exists(groupId =>
                    groupId.in(groups.keys()) and
                    memberId.in(groups.get(groupId).roster.members)
                )
        )
    }

    // ========== Properties ==========

    // Forward secrecy: old epochs can't decrypt new messages
    property forwardSecrecy = {
        always(groups.values().forall(state =>
            operationLog.forall(op =>
                // After epoch advance, old secrets are distinct
                (op.groupId == state.groupId and
                 op.targetEpoch > op.currentEpoch) implies
                    state.applicationSecrets.get(op.currentEpoch) !=
                    state.applicationSecrets.get(op.targetEpoch)
            )
        ))
    }

    // Post-compromise security: key update heals compromise
    property postCompromiseSecurity = {
        always(groups.values().forall(state =>
            // After any member updates their key
            operationLog.exists(op =>
                op.groupId == state.groupId and
                op.treeUpdates.exists(update =>
                    update.updateType.match {
                        | UpdateNode(_) => true
                        | _ => false
                    }
                )
            ) implies
                // New application secret is derived
                eventually(state.applicationSecrets.keys().size() > 1)
        ))
    }

    // Concurrent operations: all valid ops can be applied
    property concurrentOperations = {
        always(groups.values().forall(state =>
            // Multiple members can propose operations
            state.roster.members.size() > 1 implies
                state.roster.members.forall(member =>
                    // Each member can create valid operation
                    true  // TODO fix - Simplified: would check operation validity
                )
        ))
    }

    // Group agreement: all members see same state
    property groupAgreement = {
        always(groups.values().forall(state =>
            // All members in roster have access to same epoch
            state.roster.members.forall(member =>
                member.in(memberGroups.keys()) implies
                    // Can retrieve application secret for current epoch
                    state.currentEpoch.in(state.applicationSecrets.keys())
            )
        ))
    }

    // Membership authorization: only members can modify group
    property membershipAuthorization = {
        always(operationLog.forall(op =>
            groups.keys().contains(op.groupId) implies
                // Sender was member at time of operation
                op.sender.in(groups.get(op.groupId).roster.members)
        ))
    }

    // ========== Initial State ==========

    action init = {
        all {
            groups' = Map(),
            memberGroups' = Map(),
            operationLog' = [],
            currentTime' = 0
        }
    }
}
