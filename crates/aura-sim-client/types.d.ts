// TypeScript definitions for Aura Simulation Client WASM module

export interface SimulationClient {
  new(serverUrl: string): SimulationClient;
  
  // Connection management
  connect(): Promise<void>;
  disconnect(): void;
  is_connected(): boolean;
  
  // Command interface
  send_command(command: any, callback: ResponseCallback): Promise<void>;
  
  // Event subscription
  subscribe(eventTypes: string[]): Promise<void>;
  unsubscribe(eventTypes: string[]): Promise<void>;
  
  // Event handling
  set_event_callback(callback: EventCallback | null): void;
  set_connection_callback(callback: ConnectionCallback | null): void;
  
  // Event buffer
  get_events_since(sinceEventId?: number): any[];
  clear_event_buffer(): void;
  get_buffer_stats(): BufferStats;
  
  // Branch management
  current_branch_id(): string | null;
}

export interface BufferStats {
  total_events: number;
  buffer_size: number;
  oldest_event_id?: number;
  newest_event_id?: number;
  memory_usage_estimate: number;
}

export interface TraceEvent {
  tick: number;
  event_id: number;
  event_type: EventType;
  participant: string;
  causality: CausalityInfo;
}

export interface EventType {
  EffectExecuted?: {
    effect_type: string;
    effect_data: number[];
  };
  ProtocolStateTransition?: {
    protocol: string;
    from_state: string;
    to_state: string;
    witness_data?: number[];
  };
  MessageSent?: {
    envelope_id: string;
    to: string[];
    message_type: string;
    size_bytes: number;
  };
  MessageReceived?: {
    envelope_id: string;
    from: string;
    message_type: string;
  };
  MessageDropped?: {
    envelope_id: string;
    reason: DropReason;
  };
  CrdtMerge?: {
    from_replica: string;
    heads_before: string[];
    heads_after: string[];
  };
  PropertyViolation?: {
    property: string;
    violation_details: string;
  };
  CheckpointCreated?: {
    checkpoint_id: string;
    label: string;
  };
}

export interface CausalityInfo {
  parent_events: number[];
  happens_before: number[];
  concurrent_with: number[];
}

export interface ConsoleCommand {
  // Simulation control
  Step?: { count: number };
  RunUntilIdle?: {};
  SeekToTick?: { tick: number };
  Checkpoint?: { label?: string };
  RestoreCheckpoint?: { checkpoint_id: string };
  
  // State queries
  QueryState?: { device_id: string };
  GetTopology?: {};
  GetLedger?: { device_id: string };
  GetViolations?: {};
  
  // Protocol operations
  InitiateDkd?: {
    participants: string[];
    context: string;
  };
  InitiateResharing?: {
    participants: string[];
  };
  InitiateRecovery?: {
    guardians: string[];
  };
  
  // Network manipulation
  CreatePartition?: { devices: string[] };
  SetDeviceOffline?: { device_id: string };
  EnableByzantine?: {
    device_id: string;
    strategy: string;
  };
  
  // Message operations
  InjectMessage?: {
    to: string;
    message: string;
  };
  BroadcastMessage?: {
    message: string;
  };
  
  // Branch management
  ListBranches?: {};
  CheckoutBranch?: { branch_id: string };
  ForkBranch?: { label?: string };
  DeleteBranch?: { branch_id: string };
  ExportScenario?: {
    branch_id: string;
    filename: string;
  };
  
  // Scenario management
  LoadScenario?: { filename: string };
  LoadTrace?: { filename: string };
  
  // Analysis
  GetCausalityPath?: { event_id: number };
  GetEventsInRange?: {
    start: number;
    end: number;
  };
}

export interface ConsoleResponse {
  ExportScenario?: {
    toml_content: string;
    filename: string;
  };
  Error?: {
    message: string;
  };
  // ... other response types
}

export interface ConsoleEvent {
  TraceEvent?: {
    event: TraceEvent;
  };
  BranchSwitched?: {
    new_branch_id: string;
    previous_branch_id?: string;
  };
  SubscriptionChanged?: {
    subscribed: string[];
    unsubscribed: string[];
  };
  SimulationStateChanged?: {
    branch_id: string;
    new_tick: number;
    new_time: number;
  };
}

// Callback function types
export type EventCallback = (event: ConsoleEvent) => void;
export type ResponseCallback = (response: ConsoleResponse) => void;
export type ConnectionCallback = (connected: boolean) => void;

// Drop reason enum
export enum DropReason {
  NetworkPartition = "NetworkPartition",
  ByzantineBehavior = "ByzantineBehavior", 
  MessageTimeout = "MessageTimeout",
  BufferOverflow = "BufferOverflow",
}

// Export the main module initialization
export default function init(input?: InitInput | Promise<InitInput>): Promise<InitOutput>;

export interface InitInput {
  module?: WebAssembly.Module;
}

export interface InitOutput {
  memory: WebAssembly.Memory;
}