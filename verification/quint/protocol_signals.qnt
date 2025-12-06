// Protocol Signal Constants
//
// Shared signal names for protocol lifecycle coordination.
// These constants ensure consistent naming across protocol modules.

module protocol_signals {
  // ============ COMMON LIFECYCLE SIGNALS ============
  // Used by all protocol modules to signal completion/abort.

  pure val COMPLETE: str = "complete"   // Protocol finished successfully
  pure val ABORT: str = "abort"         // Protocol aborted/failed

  // ============ GROUP-SPECIFIC SIGNALS ============
  // Operation signals for group membership changes.

  pure val GROUP_CREATE: str = "create_group"        // New group created
  pure val GROUP_ADD: str = "add_members"            // Members added
  pure val GROUP_REMOVE: str = "remove_members"      // Members removed
  pure val GROUP_ARCHIVE: str = "archive_group"      // Group archived
  pure val GROUP_ROTATE_EPOCH: str = "rotate_epoch"  // Epoch rotated
}
