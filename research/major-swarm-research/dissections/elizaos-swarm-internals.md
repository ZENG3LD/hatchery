# ElizaOS Swarm Internals Dissection

**Analysis Date**: 2026-02-08
**Source**: elizaOS repository (TypeScript/Bun monorepo)
**Focus**: Multi-agent coordination patterns for Rust swarm implementation

---

## Executive Summary

ElizaOS implements a **room-based multi-agent coordination system** using:
- **World/Room hierarchy** for organizing agent interactions
- **Entity-Component-System (ECS)** architecture for agent state
- **Plugin-based extensibility** for actions/services/evaluators
- **Deterministic UUID swizzling** for cross-agent identity
- **Message-driven coordination** with room-scoped memory
- **NO explicit voting/consensus mechanism** (contrary to some claims)

**Key Insight**: ElizaOS focuses on **multi-agent presence in shared spaces** rather than swarm consensus. It's more of a "shared environment" model than a "voting swarm" model.

---

## 1. Worlds/Rooms System — Multi-Agent Coordination Spaces

### 1.1 Core Types

**File**: `packages/core/src/types/environment.ts`

```typescript
export type World = {
  id: UUID;
  name?: string;
  agentId: UUID;
  messageServerId?: UUID;
  serverId?: UUID; // @deprecated - Use messageServerId
  metadata?: {
    ownership?: {
      ownerId: string;
    };
    roles?: {
      [entityId: UUID]: Role; // OWNER | ADMIN | NONE
    };
    [key: string]: unknown;
  };
};

export enum ChannelType {
  SELF = 'SELF',       // Messages to self
  DM = 'DM',           // Direct messages between two participants
  GROUP = 'GROUP',     // Group messages with multiple participants
  VOICE_DM = 'VOICE_DM',
  VOICE_GROUP = 'VOICE_GROUP',
  FEED = 'FEED',       // Social media feed
  THREAD = 'THREAD',   // Threaded conversation
  WORLD = 'WORLD',     // World channel
  FORUM = 'FORUM',
  API = 'API',         // @deprecated
}

export type Room = {
  id: UUID;
  name?: string;
  agentId?: UUID;
  source: string;      // 'discord', 'twitter', 'elizaos', etc.
  type: ChannelType;
  channelId?: string;
  messageServerId?: UUID;
  serverId?: UUID;     // @deprecated
  worldId?: UUID;      // Parent world
  metadata?: Metadata;
};

export enum Role {
  OWNER = 'OWNER',
  ADMIN = 'ADMIN',
  NONE = 'NONE',
}
```

### 1.2 How Agents Join Rooms

**File**: `packages/core/src/runtime.ts` (lines 107-135)

```typescript
export interface IAgentRuntime extends IDatabaseAdapter {
  ensureConnections(
    entities: Entity[],
    rooms: Room[],
    source: string,
    world: World
  ): Promise<void>;

  ensureConnection({
    entityId,
    roomId,
    metadata,
    userName,
    worldName,
    name,
    source,
    channelId,
    messageServerId,
    type,
    worldId,
    userId,
  }: {
    entityId: UUID;
    roomId: UUID;
    userName?: string;
    name?: string;
    worldName?: string;
    source?: string;
    channelId?: string;
    messageServerId?: UUID;
    type?: ChannelType | string;
    worldId: UUID;
    userId?: UUID;
    metadata?: Record<string, unknown>;
  }): Promise<void>;

  ensureParticipantInRoom(entityId: UUID, roomId: UUID): Promise<void>;
  ensureWorldExists(world: World): Promise<void>;
  ensureRoomExists(room: Room): Promise<void>;
}
```

**Key Functions** (from runtime implementation):

```typescript
// Initialize agent's own room (lines 478-499)
const [agentEntity, existingRoom, participants] = await Promise.all([
  this.ensureEntity({
    id: this.agentId,
    names: [this.character.name],
    metadata: {},
    agentId: existingAgent.id!,
  }),
  this.getRoom(this.agentId),
  this.adapter.getParticipantsForRoom(this.agentId),
]);

if (!existingRoom) {
  await this.createRoom({
    id: this.agentId,
    name: this.character.name,
    source: 'elizaos',
    type: ChannelType.SELF,
    channelId: this.agentId,
    messageServerId: this.agentId,
    worldId: this.agentId,
  });
}
```

### 1.3 Room Membership & Participant Tracking

**File**: `packages/core/src/database.ts` (lines 84-111)

```typescript
abstract class DatabaseAdapter<DB = unknown> implements IDatabaseAdapter {
  abstract getEntitiesForRoom(
    roomId: UUID,
    includeComponents?: boolean
  ): Promise<Entity[]>;

  abstract getParticipantsForRoom(roomId: UUID): Promise<Participant[]>;

  abstract addParticipant(entityId: UUID, roomId: UUID): Promise<boolean>;

  abstract removeParticipant(entityId: UUID, roomId: UUID): Promise<boolean>;
}
```

**Implication for Rust**:
- Need a `RoomRegistry` with `HashMap<RoomId, Vec<EntityId>>`
- `ensure_*` pattern for idempotent room/participant creation
- Each agent has a "self" room (ChannelType::SELF) by default

---

## 2. Self-Consistency Voting — NOT FOUND

**Search Results**: Searched entire codebase for `vote|voting|consensus|self-consistency` (case-insensitive).

**Finding**: **NO voting mechanism exists in elizaOS core**.

The only match was in `packages/service-interfaces/src/interfaces/post.ts` (social media post interface — unrelated to swarm consensus).

**Conclusion**: ElizaOS does **NOT** implement self-consistency voting or multi-agent consensus. Claims about voting were likely:
1. Confused with **multi-step decision making** (iterative action execution)
2. Mistaken for **evaluator post-processing** (reflection, not voting)
3. Referring to external research papers (Wang et al. 2022 "Self-Consistency Improves Chain of Thought Reasoning") NOT implemented in elizaOS

---

## 3. Agent-to-Agent Communication

### 3.1 Message Bus Architecture

**File**: `packages/core/src/services/default-message-service.ts` (main message handler)

```typescript
export class DefaultMessageService implements IMessageService {
  async handleMessage(
    runtime: IAgentRuntime,
    message: Memory,
    callback?: HandlerCallback,
    options?: MessageProcessingOptions
  ): Promise<MessageProcessingResult> {
    // 1. Skip self-messages
    if (message.entityId === runtime.agentId) {
      return { didRespond: false, ... };
    }

    // 2. Save to memory
    const memoryId = await runtime.createMemory(message, 'messages');
    await runtime.queueEmbeddingGeneration(memoryToQueue, 'high');

    // 3. Check response decision
    const responseDecision = this.shouldRespond(
      runtime, message, room, mentionContext
    );

    // 4. Execute actions or generate response
    if (shouldRespondToMessage) {
      const result = opts.useMultiStep
        ? await this.runMultiStepCore(...)
        : await this.runSingleShotCore(...);

      await runtime.processActions(message, responseMessages, state, callback);
    }

    // 5. Run evaluators (post-interaction reflection)
    await runtime.evaluate(message, state, shouldRespondToMessage, callback);
  }
}
```

### 3.2 Message Schema

**File**: `packages/core/src/types/memory.ts`

```typescript
export interface Memory {
  id?: UUID;
  entityId: UUID;      // Who sent it
  agentId: UUID;       // Which agent processed it
  roomId: UUID;        // Where it was sent
  content: Content;
  embedding?: number[];
  createdAt?: number;
  metadata?: MemoryMetadata;
}

export interface Content {
  text?: string;
  thought?: string;         // Internal reasoning
  actions?: string[];       // Actions to execute
  providers?: string[];     // Data sources to use
  source?: string;          // 'discord', 'twitter', etc.
  target?: string;
  url?: string;
  inReplyTo?: UUID;         // Reply threading
  attachments?: Media[];
  channelType?: ChannelType;
  [key: string]: unknown;   // Extensible
}
```

### 3.3 Event System (NOT EventEmitter!)

**CRITICAL NOTE**: ElizaOS moved away from Node's EventEmitter to Bun's native EventTarget.

**File**: `packages/core/src/runtime.ts` (event handling)

```typescript
export class AgentRuntime implements IAgentRuntime {
  private eventHandlers: Map<string, ((data: unknown) => void)[]> = new Map();

  registerEvent<T extends keyof EventPayloadMap>(
    event: T,
    handler: EventHandler<T>
  ): void {
    if (!this.events[event]) this.events[event] = [];
    this.events[event].push(handler);
  }

  async emitEvent<T extends keyof EventPayloadMap>(
    event: T | T[],
    params: EventPayloadMap[T]
  ): Promise<void> {
    const events = Array.isArray(event) ? event : [event];
    for (const evt of events) {
      const handlers = this.events[evt] || [];
      await Promise.all(handlers.map(h => h(params)));
    }
  }
}
```

**Event Types** (from `packages/core/src/types/events.ts`):

```typescript
export enum EventType {
  RUN_STARTED = 'run_started',
  RUN_ENDED = 'run_ended',
  RUN_TIMEOUT = 'run_timeout',
  EMBEDDING_QUEUED = 'embedding_queued',
  EMBEDDING_COMPLETED = 'embedding_completed',
  // Custom events allowed via string keys
}

export interface RunEventPayload extends EventPayload {
  runtime: IAgentRuntime;
  source: string;
  runId: UUID;
  messageId?: UUID;
  roomId: UUID;
  entityId: UUID;
  startTime: number;
  status: 'started' | 'completed' | 'timeout';
  endTime?: number;
  duration?: number;
  error?: string;
}
```

**Rust Implication**: Use `tokio::sync::broadcast` or custom `EventBus` struct with `HashMap<EventType, Vec<Handler>>`.

---

## 4. Memory System — RAG Implementation

### 4.1 Memory Types

**File**: `packages/core/src/types/memory.ts`

```typescript
export enum MemoryType {
  MESSAGE = 'message',
  DOCUMENT = 'document',
  FRAGMENT = 'fragment',
  DESCRIPTION = 'description',
  CUSTOM = 'custom',
}

export interface MemoryMetadata {
  type: MemoryType;
  timestamp?: number;
  scope?: 'private' | 'shared';
  [key: string]: unknown;
}
```

### 4.2 Vector Search Interface

**File**: `packages/core/src/database.ts` (lines 246-257)

```typescript
abstract class DatabaseAdapter {
  abstract searchMemories(params: {
    tableName: string;
    embedding: number[];
    match_threshold?: number;
    count?: number;
    unique?: boolean;
    query?: string;
    roomId?: UUID;
    worldId?: UUID;
    entityId?: UUID;
  }): Promise<Memory[]>;

  abstract getCachedEmbeddings({
    query_table_name,
    query_threshold,
    query_input,
    query_field_name,
    query_field_sub_name,
    query_match_count,
  }: {
    query_table_name: string;
    query_threshold: number;
    query_input: string;
    query_field_name: string;
    query_field_sub_name: string;
    query_match_count: number;
  }): Promise<{ embedding: number[]; levenshtein_score: number }[]>;
}
```

### 4.3 Memory Creation & Retrieval

```typescript
abstract class DatabaseAdapter {
  abstract getMemories(params: {
    entityId?: UUID;
    agentId?: UUID;
    count?: number;
    offset?: number;
    unique?: boolean;
    tableName: string;
    start?: number;
    end?: number;
    roomId?: UUID;
    worldId?: UUID;
  }): Promise<Memory[]>;

  abstract createMemory(
    memory: Memory,
    tableName: string,
    unique?: boolean
  ): Promise<UUID>;

  abstract updateMemory(
    memory: Partial<Memory> & { id: UUID; metadata?: MemoryMetadata }
  ): Promise<boolean>;

  abstract deleteMemory(memoryId: UUID): Promise<void>;
}
```

### 4.4 BM25 Search (Fallback)

**File**: `packages/core/src/search.ts`

```typescript
export class BM25 {
  private documents: string[];
  private tokenizedDocs: string[][];
  private docFreq: Map<string, number>;
  private idf: Map<string, number>;

  constructor(documents: string[], k1 = 1.5, b = 0.75) { ... }

  search(query: string, topK = 10): { index: number; score: number }[] { ... }
}
```

**Rust Implication**:
- Use `pgvector` or `qdrant` for vector storage
- Implement `MemoryAdapter` trait with `search_similar`, `get_memories`, `create_memory`
- Consider hybrid search (vector + keyword BM25)

---

## 5. Character Files — Agent Personality Definition

### 5.1 Character Schema

**File**: `packages/core/src/types/agent.ts`

```typescript
export interface Character {
  id?: UUID;
  name: string;
  username?: string;
  system?: string;                    // System prompt
  templates?: {
    [key: string]: TemplateType;      // Prompt templates
  };
  bio: string | string[];
  messageExamples?: MessageExample[][]; // Conversation examples
  postExamples?: string[];
  topics?: string[];
  adjectives?: string[];
  knowledge?: (string | { path: string; shared?: boolean } | DirectoryItem)[];
  plugins?: string[];                 // Plugin names to load
  settings?: {
    [key: string]: string | boolean | number | Record<string, unknown>;
  };
  secrets?: {                         // API keys, etc.
    [key: string]: string | boolean | number;
  };
  style?: {
    all?: string[];
    chat?: string[];
    post?: string[];
  };
}
```

### 5.2 Example Character File (inferred structure)

```json
{
  "name": "Eliza",
  "bio": ["AI psychotherapist", "Empathetic listener"],
  "system": "You are Eliza, a compassionate AI assistant...",
  "plugins": [
    "@elizaos/plugin-sql",
    "@elizaos/plugin-discord"
  ],
  "messageExamples": [
    [
      { "name": "User", "content": { "text": "I feel sad today" } },
      { "name": "Eliza", "content": {
        "text": "I hear you're feeling sad. Can you tell me more?",
        "thought": "User expressing sadness, use empathetic reflection"
      }}
    ]
  ],
  "settings": {
    "USE_MULTI_STEP": true,
    "MAX_MULTISTEP_ITERATIONS": 6
  },
  "style": {
    "chat": ["empathetic", "reflective", "non-judgmental"]
  }
}
```

**Rust Implication**:
- Use `serde_json` with `#[derive(Deserialize)]` for character loading
- Store as `.toml` or `.json` in `configs/agents/` directory
- Support hot-reloading via file watcher

---

## 6. Plugin System — Extension Points

### 6.1 Plugin Interface

**File**: `packages/core/src/types/plugin.ts`

```typescript
export interface Plugin {
  name: string;
  description: string;
  actions?: Action[];          // User-facing commands
  services?: (typeof Service)[]; // State management
  providers?: Provider[];      // Context suppliers
  evaluators?: Evaluator[];    // Post-processing
  models?: {                   // Custom LLM handlers
    [modelType: string]: ModelHandler;
  };
  routes?: Route[];            // HTTP endpoints
  events?: {                   // Event handlers
    [eventName: string]: EventHandler[];
  };
  adapter?: IDatabaseAdapter;  // Custom DB
  init?: (config: Record<string, string>, runtime: IAgentRuntime) => Promise<void>;
  priority?: number;           // Load order
}
```

### 6.2 Action Interface (User Commands)

**File**: `packages/core/src/types/components.ts`

```typescript
export interface Action {
  name: string;
  description?: string;
  examples: ActionExample[][];

  validate: (
    runtime: IAgentRuntime,
    message: Memory,
    state?: State
  ) => Promise<boolean>;

  handler: (
    runtime: IAgentRuntime,
    message: Memory,
    state?: State,
    options?: HandlerOptions,
    callback?: HandlerCallback
  ) => Promise<ActionResult>;
}

export interface ActionResult {
  success: boolean;
  text?: string;
  error?: string | Error;
  data?: Record<string, unknown>;
  values?: Record<string, unknown>;
}
```

### 6.3 Service Interface (State Management)

**File**: `packages/core/src/types/service.ts`

```typescript
export abstract class Service {
  static serviceType: ServiceTypeName;

  abstract initialize(runtime: IAgentRuntime): Promise<void>;
  abstract stop(): Promise<void>;
}

// Example: MessageService
export interface IMessageService extends Service {
  handleMessage(
    runtime: IAgentRuntime,
    message: Memory,
    callback?: HandlerCallback,
    options?: MessageProcessingOptions
  ): Promise<MessageProcessingResult>;
}
```

### 6.4 Provider Interface (Context Suppliers)

**File**: `packages/core/src/types/components.ts`

```typescript
export interface Provider {
  name: string;
  description?: string;

  get: (
    runtime: IAgentRuntime,
    message: Memory,
    state?: State
  ) => Promise<{ text: string }>;
}
```

### 6.5 Evaluator Interface (Post-Interaction)

```typescript
export interface Evaluator {
  name: string;
  description?: string;
  examples: ActionExample[][];

  validate: (
    runtime: IAgentRuntime,
    message: Memory,
    state?: State
  ) => Promise<boolean>;

  handler: (
    runtime: IAgentRuntime,
    message: Memory,
    state?: State,
    options?: HandlerOptions,
    callback?: HandlerCallback
  ) => Promise<void>;
}
```

**Rust Implication**:
- Define `Plugin` trait with `register(&mut Runtime)` method
- Use trait objects `Box<dyn Action>`, `Box<dyn Service>`, etc.
- Store plugins in `Vec<Box<dyn Plugin>>` on runtime
- Support dynamic loading via `libloading` crate (optional)

---

## 7. Deterministic UUID Swizzling

### 7.1 Cross-Agent Identity

**File**: `packages/core/src/entities.ts` (lines 323-335)

```typescript
export const createUniqueUuid = (
  runtime: IAgentRuntime,
  baseUserId: UUID | string
): UUID => {
  // If the base user ID is the agent ID, return it directly
  if (baseUserId === runtime.agentId) {
    return runtime.agentId;
  }

  // Use a deterministic approach to generate a new UUID based on both IDs
  // This creates a unique ID for each user+agent combination while still being deterministic
  const combinedString = `${baseUserId}:${runtime.agentId}`;

  // Create a namespace UUID (version 5) from the combined string
  return stringToUuid(combinedString);
};
```

**File**: `packages/core/src/utils/index.ts` (stringToUuid implementation)

```typescript
import { v5 as uuidv5 } from 'uuid';

export function stringToUuid(str: string): UUID {
  // UUID v5 with DNS namespace
  const namespace = '6ba7b810-9dad-11d1-80b4-00c04fd430c8';
  return uuidv5(str, namespace) as UUID;
}
```

**Why This Matters**:
- Each agent sees the same user with a **different deterministic UUID**
- Prevents ID collisions in shared memory stores
- Example: `user123` → `agent-A` sees `uuid-A`, `agent-B` sees `uuid-B`

**Rust Implication**:
- Use `uuid::Uuid::new_v5(&namespace, data)` for deterministic UUIDs
- Create `EntityIdSwizzler` utility with `swizzle(base_id: &str, agent_id: &Uuid) -> Uuid`

---

## 8. Multi-Step Decision Making (NOT Voting!)

### 8.1 Iterative Action Execution

**File**: `packages/core/src/services/default-message-service.ts` (lines 1197-1951)

This is what people **mistakenly call "voting"**. It's actually **iterative action execution**:

```typescript
private async runMultiStepCore(
  runtime: IAgentRuntime,
  message: Memory,
  state: State,
  callback: HandlerCallback | undefined,
  opts: ResolvedMessageOptions,
  responseId: UUID
): Promise<StrategyResult> {
  const traceActionResult: MultiStepActionResult[] = [];
  let iterationCount = 0;

  while (iterationCount < opts.maxMultiStepIterations) {
    iterationCount++;

    // 1. Compose state with previous action results
    accumulatedState = await runtime.composeState(message, [
      'RECENT_MESSAGES',
      'ACTION_STATE',
    ]);
    accumulatedState.data.actionResults = traceActionResult;

    // 2. LLM decides next action
    const prompt = composePromptFromState({
      state: accumulatedState,
      template: multiStepDecisionTemplate,
    });

    const stepResultRaw = await runtime.useModel(ModelType.TEXT_LARGE, { prompt });
    const parsedStep = parseKeyValueXml(stepResultRaw);

    const thought = parsedStep.thought;
    const action = parsedStep.action;
    const isFinish = parsedStep.isFinish;

    // 3. Execute providers in parallel
    const providerResults = await Promise.allSettled(
      providers.map(name => executeProvider(name, runtime, message, state))
    );

    // 4. Execute action
    if (action) {
      await runtime.processActions(message, [...], accumulatedState);
      traceActionResult.push({ actionName: action, success, text });
    }

    // 5. Check completion
    if (isFinish === 'true') break;
  }

  // 6. Generate final summary
  const summaryPrompt = composePromptFromState({
    state: accumulatedState,
    template: multiStepSummaryTemplate,
  });

  const finalOutput = await runtime.useModel(ModelType.TEXT_LARGE, {
    prompt: summaryPrompt
  });
}
```

**Key Takeaways**:
- This is **NOT multi-agent consensus**
- It's **one agent** making **multiple sequential decisions**
- Providers run in **parallel** (with timeout), actions run **sequentially**
- Final summary consolidates all action results

---

## 9. State Composition System

### 9.1 State Interface

**File**: `packages/core/src/types/state.ts`

```typescript
export interface State {
  /** Typed values for template substitution */
  values: {
    [key: string]: unknown;
  };

  /** Structured data (action results, provider outputs) */
  data: {
    [key: string]: unknown;
  };

  /** Final composed text for prompts */
  text: string;
}
```

### 9.2 State Composition

**File**: `packages/core/src/runtime.ts` (composeState method)

```typescript
async composeState(
  message: Memory,
  includeList?: string[],
  onlyInclude?: boolean,
  skipCache?: boolean
): Promise<State> {
  // 1. Get recent messages from room
  const recentMessages = await this.getMemories({
    tableName: 'messages',
    roomId: message.roomId,
    count: this.#conversationLength,
  });

  // 2. Get entities in room
  const entities = await this.getEntitiesForRoom(message.roomId, true);

  // 3. Execute providers
  for (const provider of this.providers) {
    if (includeList?.includes(provider.name)) {
      const result = await provider.get(this, message, state);
      state.data[provider.name] = result;
    }
  }

  // 4. Compose final text
  state.text = composePromptFromState({ state, template });

  return state;
}
```

---

## 10. Implications for Rust Swarm Implementation

### 10.1 Core Architecture

```rust
// Core types
pub struct World {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub roles: HashMap<Uuid, Role>,
    pub metadata: HashMap<String, serde_json::Value>,
}

pub struct Room {
    pub id: Uuid,
    pub name: String,
    pub world_id: Uuid,
    pub channel_type: ChannelType,
    pub source: String,
    pub participants: Vec<Uuid>,
}

pub struct Entity {
    pub id: Uuid,
    pub names: Vec<String>,
    pub agent_id: Uuid,
    pub metadata: HashMap<String, serde_json::Value>,
    pub components: Vec<Component>,
}

pub struct Memory {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub agent_id: Uuid,
    pub room_id: Uuid,
    pub content: Content,
    pub embedding: Option<Vec<f32>>,
    pub created_at: i64,
}
```

### 10.2 Event Bus

```rust
use tokio::sync::broadcast;

pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn emit(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    RunStarted { run_id: Uuid, room_id: Uuid },
    RunEnded { run_id: Uuid, duration: Duration },
    MessageReceived { message: Memory },
    ActionExecuted { action: String, result: ActionResult },
}
```

### 10.3 Plugin System

```rust
#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    async fn register(&self, runtime: &mut Runtime);
}

#[async_trait]
pub trait Action: Send + Sync {
    fn name(&self) -> &str;
    async fn validate(&self, runtime: &Runtime, message: &Memory) -> bool;
    async fn handler(&self, runtime: &Runtime, message: &Memory, state: &State)
        -> Result<ActionResult>;
}

#[async_trait]
pub trait Service: Send + Sync {
    async fn initialize(&self, runtime: &Runtime);
    async fn stop(&self);
}

pub struct Runtime {
    pub agent_id: Uuid,
    pub character: Character,
    pub actions: Vec<Arc<dyn Action>>,
    pub services: HashMap<String, Arc<dyn Service>>,
    pub event_bus: EventBus,
    pub memory: Arc<dyn MemoryAdapter>,
}
```

### 10.4 Memory Adapter

```rust
#[async_trait]
pub trait MemoryAdapter: Send + Sync {
    async fn create_memory(&self, memory: &Memory, table: &str) -> Result<Uuid>;
    async fn get_memories(&self, params: GetMemoriesParams) -> Result<Vec<Memory>>;
    async fn search_memories(&self, embedding: &[f32], threshold: f32, limit: usize)
        -> Result<Vec<Memory>>;
    async fn update_memory(&self, id: Uuid, content: Content) -> Result<()>;
    async fn delete_memory(&self, id: Uuid) -> Result<()>;
}

// PostgreSQL + pgvector implementation
pub struct PgVectorAdapter {
    pool: PgPool,
}
```

### 10.5 Room Management

```rust
pub struct RoomManager {
    rooms: Arc<RwLock<HashMap<Uuid, Room>>>,
    participants: Arc<RwLock<HashMap<Uuid, Vec<Uuid>>>>, // room_id -> entity_ids
}

impl RoomManager {
    pub async fn ensure_room(&self, room: Room) -> Result<()> {
        let mut rooms = self.rooms.write().await;
        rooms.entry(room.id).or_insert(room);
        Ok(())
    }

    pub async fn add_participant(&self, room_id: Uuid, entity_id: Uuid) -> Result<()> {
        let mut participants = self.participants.write().await;
        participants.entry(room_id)
            .or_insert_with(Vec::new)
            .push(entity_id);
        Ok(())
    }

    pub async fn get_participants(&self, room_id: Uuid) -> Vec<Uuid> {
        self.participants.read().await
            .get(&room_id)
            .cloned()
            .unwrap_or_default()
    }
}
```

---

## 11. What ElizaOS Does NOT Have (Myths Debunked)

### 11.1 No Voting/Consensus

- **No multi-agent voting** on decisions
- **No consensus protocols** (Raft, Paxos, etc.)
- **No self-consistency sampling** (despite the name appearing in research)
- Multi-step mode is **single-agent iterative**, not multi-agent voting

### 11.2 No Built-In Swarm Coordination

- Agents can **share rooms**, but don't **coordinate decisions**
- No "swarm leader" or "worker agent" patterns
- No task distribution or load balancing
- No agent-to-agent direct communication (all via rooms)

### 11.3 No Hierarchical Agent Trees

- Flat agent structure (all agents are peers)
- No parent-child agent relationships
- Worlds provide **organization**, not **hierarchy**

---

## 12. Key Patterns to Adopt for Rust Swarm

### 12.1 Room-Based Coordination ✅

```rust
// Agents coordinate by sharing rooms
let trading_room = Room::new("BTC-USD-Analysis", ChannelType::Group);
room_manager.add_participant(trading_room.id, analyst_agent.id).await;
room_manager.add_participant(trading_room.id, executor_agent.id).await;

// Analyst posts analysis to room
analyst_agent.send_message(&trading_room, "BTC bullish divergence detected").await;

// Executor receives via room subscription
let messages = memory.get_memories(GetMemoriesParams {
    room_id: trading_room.id,
    count: 10,
}).await;
```

### 12.2 Entity-Component System ✅

```rust
// Rich entity metadata via components
let trader_entity = Entity {
    id: Uuid::new_v4(),
    names: vec!["TraderBot".into()],
    agent_id: agent.id,
    components: vec![
        Component {
            type_: "portfolio".into(),
            data: json!({ "balance_usd": 10000.0, "positions": [] }),
            ...
        },
        Component {
            type_: "risk_params".into(),
            data: json!({ "max_drawdown": 0.05, "max_leverage": 3.0 }),
            ...
        },
    ],
};
```

### 12.3 Plugin Extensibility ✅

```rust
// Define trading plugin
pub struct TradingPlugin;

#[async_trait]
impl Plugin for TradingPlugin {
    fn name(&self) -> &str { "trading" }

    async fn register(&self, runtime: &mut Runtime) {
        runtime.register_action(Arc::new(PlaceOrderAction));
        runtime.register_action(Arc::new(CancelOrderAction));
        runtime.register_service(Arc::new(ExchangeService::new()));
    }
}
```

### 12.4 Deterministic UUID Swizzling ✅

```rust
use uuid::Uuid;

pub fn swizzle_entity_id(base_id: &str, agent_id: Uuid) -> Uuid {
    let namespace = Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();
    let combined = format!("{}:{}", base_id, agent_id);
    Uuid::new_v5(&namespace, combined.as_bytes())
}
```

---

## 13. Summary Table: ElizaOS Features → Rust Swarm

| ElizaOS Feature | Rust Equivalent | Priority | Complexity |
|----------------|-----------------|----------|------------|
| World/Room hierarchy | `World`, `Room` structs + `RoomManager` | HIGH | Medium |
| Entity-Component-System | `Entity` with `Vec<Component>` | HIGH | Low |
| Plugin system | `trait Plugin` + dynamic registration | HIGH | Medium |
| Message memory | `MemoryAdapter` trait + pgvector | HIGH | High |
| Event bus | `tokio::sync::broadcast` | MEDIUM | Low |
| Deterministic UUIDs | `uuid::Uuid::new_v5` | MEDIUM | Low |
| Multi-step execution | Iterative action loop | LOW | Medium |
| State composition | `State` struct + provider system | MEDIUM | Medium |
| Character files | TOML/JSON + `serde` | HIGH | Low |
| BM25 search | `tantivy` crate | LOW | Medium |

---

## 14. Files to Reference

**Core Architecture**:
- `packages/core/src/types/environment.ts` — World/Room/Entity types
- `packages/core/src/types/runtime.ts` — IAgentRuntime interface
- `packages/core/src/runtime.ts` — AgentRuntime implementation
- `packages/core/src/database.ts` — DatabaseAdapter interface

**Message Handling**:
- `packages/core/src/services/default-message-service.ts` — Message processing pipeline
- `packages/core/src/types/memory.ts` — Memory/Content types

**Extensibility**:
- `packages/core/src/types/plugin.ts` — Plugin interface
- `packages/core/src/types/components.ts` — Action/Provider/Evaluator
- `packages/core/src/plugin.ts` — Plugin loading utilities

**Character System**:
- `packages/core/src/types/agent.ts` — Character/Agent types
- `packages/core/src/schemas/character.ts` — Validation schema

**Utilities**:
- `packages/core/src/entities.ts` — UUID swizzling, entity resolution
- `packages/core/src/search.ts` — BM25 implementation
- `packages/core/src/types/events.ts` — Event types

---

## 15. Conclusion

ElizaOS provides a **solid foundation for room-based multi-agent systems**, but **NOT** for voting/consensus swarms. Its strengths lie in:

1. **Shared environment model** (rooms as coordination spaces)
2. **Rich entity metadata** (component-based)
3. **Plugin extensibility** (actions/services/providers)
4. **Persistent memory** (vector + keyword search)
5. **Deterministic identity** (UUID swizzling)

For a **Rust trading swarm**, adopt:
- Room-based agent coordination ✅
- Entity-component architecture ✅
- Plugin system for strategies ✅
- Memory adapter for market data ✅
- Event bus for real-time coordination ✅

**Skip** or redesign:
- Multi-step decision making (too LLM-centric)
- Character files (replace with strategy configs)
- Twitter/Discord integrations (not relevant)

**Add** for swarm coordination:
- Leader election (NOT in elizaOS)
- Task distribution (NOT in elizaOS)
- Consensus voting (NOT in elizaOS)
- Agent-to-agent RPC (NOT in elizaOS)

---

**End of Dissection**
