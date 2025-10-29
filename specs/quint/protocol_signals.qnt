module protocol_signals {
  // Common lifecycle signals
  pure val COMPLETE: str = "complete"
  pure val ABORT: str = "abort"

  // Group lifecycle signal names
  pure val GROUP_CREATE: str = "create_group"
  pure val GROUP_ADD: str = "add_members"
  pure val GROUP_REMOVE: str = "remove_members"
  pure val GROUP_ARCHIVE: str = "archive_group"
  pure val GROUP_ROTATE_EPOCH: str = "rotate_epoch"
}
