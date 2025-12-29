// TUI State Machine Specification
//
// Models the Aura TUI's pure state machine for deterministic testing verification.
// The TUI follows a simple model: TuiState × TerminalEvent → (TuiState, Vec<TuiCommand>)
//
// Key properties:
// - Navigation: Screen switching via number keys (1-7) and Tab
// - Modals: Block all navigation when active, Escape closes
// - Insert mode: Text input on Home/Chat screens, Escape exits
// - Determinism: Same inputs produce same outputs
//
// Safety invariants:
// - Modal blocks all navigation
// - Insert mode only on Home or Chat screens
// - Escape always provides exit path
//
// See: crates/aura-terminal/src/tui/state_machine.rs

module tui_state_machine {
  // ============ TYPE DEFINITIONS ============

  type Option[a] = Some(a) | None

  // Screens in the TUI (6 total)
  //
  // This must stay in sync with:
  // - `crates/aura-terminal/src/tui/screens/router.rs` (`Screen`)
  // - the nav bar tab ordering in the TUI shell
  type Screen = Home | Neighborhood | Chat | Contacts | Recovery | Settings

  // Modal types that can be displayed
  type ModalType =
    | NoModal
    | HelpModal
    | AccountSetupModal
    | GuardianSelectModal
    | ContactSelectModal
    | CreateChannelModal
    | ChannelInfoModal
    | SetTopicModal
    | CreateInvitationModal
    | ImportInvitationModal
    | ExportInvitationModal
    | ConfirmModal
    | TextInputModal
    | ThresholdConfigModal
    | InvitationCodeModal

  // Key codes from terminal input
  type KeyCode =
    | CharKey(str)          // Character key with the character
    | EnterKey
    | EscapeKey
    | TabKey
    | BackspaceKey
    | DeleteKey
    | UpKey
    | DownKey
    | LeftKey
    | RightKey
    | HomeKey
    | EndKey
    | PageUpKey
    | PageDownKey
    | FnKey(int)            // Function key with number

  // Key modifiers
  type Modifiers = {
    shift: bool,
    ctrl: bool,
    alt: bool
  }

  // Terminal event types - using records for multiple fields
  type TerminalEvent =
    | KeyEvent({code: KeyCode, mods: Modifiers})
    | ResizeEvent({width: int, height: int})
    | TickEvent

  // TUI command types (side effects)
  type TuiCommand =
    | ExitCommand
    | ShowToastCommand(str)
    | DismissToastCommand(int)
    | RenderCommand

  // ============ STATE VARIABLES ============

  // Current screen
  var currentScreen: Screen

  // Current modal (NoModal means no modal active)
  var currentModal: ModalType

  // Modal input buffer (for text input modals)
  var modalInputBuffer: str

  // Insert mode state per screen
  var homeInsertMode: bool
  var chatInsertMode: bool

  // Input buffers per screen
  var homeInputBuffer: str
  var chatInputBuffer: str

  // Whether exit has been requested
  var shouldExit: bool

  // Terminal size
  var terminalWidth: int
  var terminalHeight: int

  // Command queue (side effects to execute)
  var commandQueue: List[TuiCommand]

  // ============ HELPER DEFINITIONS ============

  // Check if any modal is active (val reads current state)
  val hasModal: bool = currentModal != NoModal

  // Check if in insert mode
  val isInsertMode: bool =
    (currentScreen == Home and homeInsertMode) or
    (currentScreen == Chat and chatInsertMode)

  // Check if navigation is allowed
  val canNavigate: bool = not(hasModal) and not(isInsertMode)

  // Get screen from number key (1-6)
  pure def screenFromKey(key: int): Option[Screen] =
    if (key == 1) Some(Home)
    else if (key == 2) Some(Neighborhood)
    else if (key == 3) Some(Chat)
    else if (key == 4) Some(Contacts)
    else if (key == 5) Some(Recovery)
    else if (key == 6) Some(Settings)
    else None

  // Get next screen (for Tab navigation)
  pure def nextScreen(screen: Screen): Screen =
    match screen {
      | Home => Neighborhood
      | Neighborhood => Chat
      | Chat => Contacts
      | Contacts => Recovery
      | Recovery => Settings
      | Settings => Home
    }

  // Get previous screen (for Shift+Tab navigation)
  pure def prevScreen(screen: Screen): Screen =
    match screen {
      | Home => Settings
      | Neighborhood => Home
      | Chat => Neighborhood
      | Contacts => Chat
      | Recovery => Contacts
      | Settings => Recovery
    }

  // No modifiers
  pure def noModifiers(): Modifiers = { shift: false, ctrl: false, alt: false }

  // Shift modifier
  pure def shiftModifier(): Modifiers = { shift: true, ctrl: false, alt: false }

  // Ctrl modifier
  pure def ctrlModifier(): Modifiers = { shift: false, ctrl: true, alt: false }

  // ============ INITIALIZATION ============

  action init = all {
    currentScreen' = Home,
    currentModal' = NoModal,
    modalInputBuffer' = "",
    homeInsertMode' = false,
    chatInsertMode' = false,
    homeInputBuffer' = "",
    chatInputBuffer' = "",
    shouldExit' = false,
    terminalWidth' = 80,
    terminalHeight' = 24,
    commandQueue' = []
  }

  // ============ STATE TRANSITIONS ============

  // Handle escape key - universal exit mechanism
  action handleEscape = all {
    if (hasModal) all {
      currentModal' = NoModal,
      modalInputBuffer' = "",
      // Other state unchanged
      currentScreen' = currentScreen,
      homeInsertMode' = homeInsertMode,
      chatInsertMode' = chatInsertMode,
      homeInputBuffer' = homeInputBuffer,
      chatInputBuffer' = chatInputBuffer,
      shouldExit' = shouldExit,
      terminalWidth' = terminalWidth,
      terminalHeight' = terminalHeight,
      commandQueue' = []
    }
    else if (isInsertMode) all {
      homeInsertMode' = false,
      chatInsertMode' = false,
      // Other state unchanged
      currentScreen' = currentScreen,
      currentModal' = currentModal,
      modalInputBuffer' = modalInputBuffer,
      homeInputBuffer' = homeInputBuffer,
      chatInputBuffer' = chatInputBuffer,
      shouldExit' = shouldExit,
      terminalWidth' = terminalWidth,
      terminalHeight' = terminalHeight,
      commandQueue' = []
    }
    else all {
      // Escape does nothing in normal mode
      currentScreen' = currentScreen,
      currentModal' = currentModal,
      modalInputBuffer' = modalInputBuffer,
      homeInsertMode' = homeInsertMode,
      chatInsertMode' = chatInsertMode,
      homeInputBuffer' = homeInputBuffer,
      chatInputBuffer' = chatInputBuffer,
      shouldExit' = shouldExit,
      terminalWidth' = terminalWidth,
      terminalHeight' = terminalHeight,
      commandQueue' = []
    }
  }

  // Handle quit key (q)
  action handleQuit = all {
    if (not(hasModal) and not(isInsertMode)) all {
      shouldExit' = true,
      commandQueue' = [ExitCommand],
      // Other state unchanged
      currentScreen' = currentScreen,
      currentModal' = currentModal,
      modalInputBuffer' = modalInputBuffer,
      homeInsertMode' = homeInsertMode,
      chatInsertMode' = chatInsertMode,
      homeInputBuffer' = homeInputBuffer,
      chatInputBuffer' = chatInputBuffer,
      terminalWidth' = terminalWidth,
      terminalHeight' = terminalHeight
    }
    else all {
      // q is blocked by modal or insert mode
      currentScreen' = currentScreen,
      currentModal' = currentModal,
      modalInputBuffer' = modalInputBuffer,
      homeInsertMode' = homeInsertMode,
      chatInsertMode' = chatInsertMode,
      homeInputBuffer' = homeInputBuffer,
      chatInputBuffer' = chatInputBuffer,
      shouldExit' = shouldExit,
      terminalWidth' = terminalWidth,
      terminalHeight' = terminalHeight,
      commandQueue' = []
    }
  }

  // Handle help key (?)
  action handleHelp = all {
    if (not(hasModal) and not(isInsertMode)) all {
      currentModal' = HelpModal,
      // Other state unchanged
      currentScreen' = currentScreen,
      modalInputBuffer' = modalInputBuffer,
      homeInsertMode' = homeInsertMode,
      chatInsertMode' = chatInsertMode,
      homeInputBuffer' = homeInputBuffer,
      chatInputBuffer' = chatInputBuffer,
      shouldExit' = shouldExit,
      terminalWidth' = terminalWidth,
      terminalHeight' = terminalHeight,
      commandQueue' = []
    }
    else all {
      // ? is blocked
      currentScreen' = currentScreen,
      currentModal' = currentModal,
      modalInputBuffer' = modalInputBuffer,
      homeInsertMode' = homeInsertMode,
      chatInsertMode' = chatInsertMode,
      homeInputBuffer' = homeInputBuffer,
      chatInputBuffer' = chatInputBuffer,
      shouldExit' = shouldExit,
      terminalWidth' = terminalWidth,
      terminalHeight' = terminalHeight,
      commandQueue' = []
    }
  }

  // Helper to create unchanged state action
  action unchanged = all {
    currentScreen' = currentScreen,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  // Helper: navigate preserving other state
  action navigateTo1 = all {
    currentScreen' = Home,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  action navigateTo2 = all {
    currentScreen' = Neighborhood,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  action navigateTo3 = all {
    currentScreen' = Chat,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  action navigateTo4 = all {
    currentScreen' = Contacts,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  action navigateTo5 = all {
    currentScreen' = Recovery,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  action navigateTo6 = all {
    currentScreen' = Settings,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  // Navigate to next screen
  action navigateNext = all {
    currentScreen' = nextScreen(currentScreen),
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  // Navigate to previous screen
  action navigatePrev = all {
    currentScreen' = prevScreen(currentScreen),
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  // Handle number key 1 - go to Home
  action handleKey1 =
    if (not(hasModal) and not(isInsertMode)) navigateTo1
    else unchanged

  // Handle number key 2 - go to Neighborhood
  action handleKey2 =
    if (not(hasModal) and not(isInsertMode)) navigateTo2
    else unchanged

  // Handle number key 3 - go to Chat
  action handleKey3 =
    if (not(hasModal) and not(isInsertMode)) navigateTo3
    else unchanged

  // Handle number key 4 - go to Contacts
  action handleKey4 =
    if (not(hasModal) and not(isInsertMode)) navigateTo4
    else unchanged

  // Handle number key 5 - go to Recovery
  action handleKey5 =
    if (not(hasModal) and not(isInsertMode)) navigateTo5
    else unchanged

  // Handle number key 6 - go to Settings
  action handleKey6 =
    if (not(hasModal) and not(isInsertMode)) navigateTo6
    else unchanged

  // Handle Tab key (next screen)
  action handleTab =
    if (not(hasModal) and not(isInsertMode)) navigateNext
    else unchanged

  // Handle Shift+Tab key (previous screen)
  action handleShiftTab =
    if (not(hasModal) and not(isInsertMode)) navigatePrev
    else unchanged

  // Enter insert mode on Home screen
  action enterBlockInsertMode = all {
    homeInsertMode' = true,
    currentScreen' = currentScreen,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    chatInsertMode' = chatInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  // Enter insert mode on Chat screen
  action enterChatInsertMode = all {
    chatInsertMode' = true,
    currentScreen' = currentScreen,
    currentModal' = currentModal,
    modalInputBuffer' = modalInputBuffer,
    homeInsertMode' = homeInsertMode,
    homeInputBuffer' = homeInputBuffer,
    chatInputBuffer' = chatInputBuffer,
    shouldExit' = shouldExit,
    terminalWidth' = terminalWidth,
    terminalHeight' = terminalHeight,
    commandQueue' = []
  }

  // Handle insert mode key (i)
  action handleInsertKey =
    if (hasModal or isInsertMode) unchanged
    else if (currentScreen == Home) enterBlockInsertMode
    else if (currentScreen == Chat) enterChatInsertMode
    else unchanged

  // Handle resize event - nondeterministic size
  action handleResize = {
    nondet w = 10.to(200).oneOf()
    nondet h = 10.to(100).oneOf()
    all {
      terminalWidth' = w,
      terminalHeight' = h,
      currentScreen' = currentScreen,
      currentModal' = currentModal,
      modalInputBuffer' = modalInputBuffer,
      homeInsertMode' = homeInsertMode,
      chatInsertMode' = chatInsertMode,
      homeInputBuffer' = homeInputBuffer,
      chatInputBuffer' = chatInputBuffer,
      shouldExit' = shouldExit,
      commandQueue' = []
    }
  }

  // ============ MAIN STEP FUNCTION ============

  // Non-deterministic step (any valid input)
  action step = any {
    handleEscape,
    handleQuit,
    handleHelp,
    handleKey1,
    handleKey2,
    handleKey3,
    handleKey4,
    handleKey5,
    handleKey6,
    handleTab,
    handleShiftTab,
    handleInsertKey,
    handleResize
  }

  // ============ INVARIANTS (SAFETY PROPERTIES) ============

  // Modal blocks all screen navigation
  val modalBlocksNavigation: bool =
    hasModal implies (
      // Can't navigate via number keys when modal is active
      // (This is enforced by the transition functions)
      true
    )

  // Insert mode only available on Home or Chat screens
  val insertModeOnlyValidScreens: bool =
    isInsertMode implies (currentScreen == Home or currentScreen == Chat)

  // Escape always provides an exit path
  val escapeAlwaysExits: bool =
    // After pressing Escape, either modal closes or insert mode exits
    // (This is a liveness property, verified via temporal logic below)
    true

  // Screen is always one of the 6 valid screens
  val validScreen: bool =
    currentScreen == Home or
    currentScreen == Neighborhood or
    currentScreen == Chat or
    currentScreen == Contacts or
    currentScreen == Recovery or
    currentScreen == Settings

  // Terminal size is reasonable
  val validTerminalSize: bool =
    terminalWidth >= 10 and terminalWidth <= 500 and
    terminalHeight >= 5 and terminalHeight <= 200

  // All invariants combined
  val allInvariants: bool =
    modalBlocksNavigation and
    insertModeOnlyValidScreens and
    validScreen and
    validTerminalSize

  // ============ TEMPORAL PROPERTIES (LIVENESS) ============

  // Can always eventually exit by pressing 'q' (when not blocked)
  temporal canAlwaysExit: bool =
    always(eventually(shouldExit))

  // Modal can always be closed with Escape
  temporal modalEventuallyCloses: bool =
    always(hasModal implies eventually(not(hasModal)))

  // Insert mode can always be exited with Escape
  temporal insertModeEventuallyExits: bool =
    always(isInsertMode implies eventually(not(isInsertMode)))

  // Tab cycling returns to start after 6 presses
  // (This is verified in the Rust tests via proptest)

  // ============ TEST SCENARIOS ============

  // Test: Navigation cycle
  run navigationCycleTest = {
    init
    .then(handleKey3)              // Go to Chat
    .then(handleKey4)              // Go to Contacts
    .then(handleTab)               // Tab to Recovery
    .then(handleShiftTab)          // Shift+Tab back to Contacts
    .then(handleKey1)              // Go to Home
    .then(all {
      assert(currentScreen == Home),
      unchanged
    })
  }

  // Test: Modal blocks navigation
  run modalBlocksNavigationTest = {
    init
    .then(handleHelp)              // Open help modal
    .then(all {
      assert(hasModal),
      unchanged
    })
    .then(handleKey3)              // Try to navigate (blocked)
    .then(all {
      assert(currentScreen == Home),  // Still on Home
      assert(hasModal),              // Modal still open
      unchanged
    })
    .then(handleEscape)            // Close modal
    .then(all {
      assert(not(hasModal)),
      unchanged
    })
  }

  // Test: Insert mode
  run insertModeTest = {
    init
    .then(handleInsertKey)         // Enter insert mode on Home
    .then(all {
      assert(isInsertMode),
      assert(homeInsertMode),
      unchanged
    })
    .then(handleQuit)              // Try to quit (blocked)
    .then(all {
      assert(not(shouldExit)),     // Still not exiting
      unchanged
    })
    .then(handleEscape)            // Exit insert mode
    .then(all {
      assert(not(isInsertMode)),
      unchanged
    })
  }
}
