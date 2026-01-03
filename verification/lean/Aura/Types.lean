import Aura.Types.ByteArray32
import Aura.Types.Identifiers
import Aura.Types.OrderTime
import Aura.Types.TimeStamp
import Aura.Types.Namespace
import Aura.Types.TreeOp
import Aura.Types.AttestedOp
import Aura.Types.ProtocolFacts
import Aura.Types.FactContent

/-! # Aura.Types

Re-export module for all Aura type definitions.

## Module Hierarchy

- `ByteArray32`: Foundation 32-byte arrays
- `Identifiers`: Hash32, AuthorityId, ContextId, ChannelId
- `OrderTime`: Opaque ordering key
- `TimeStamp`: 4-variant time enum
- `Namespace`: JournalNamespace (Authority/Context)
- `TreeOp`: Tree operations (AddLeaf, etc.)
- `AttestedOp`: Attested tree operations
- `ProtocolFacts`: 12-variant protocol facts
- `FactContent`: Fact payload types
-/
