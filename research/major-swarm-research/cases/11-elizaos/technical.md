# elizaOS (ai16z) - Autonomous Agent Swarm Framework - Technical Deep Dive

## 1. Prompts & Prompting Strategy

### Character Files - Core Personality System

**Philosophy**: "The easiest customization of an Eliza agent is its character, which is reminiscent of the 'system prompt' in a chatbot. This system prompt is prefixed to each further prompt to modulate the chatbot resulting answer."

**Format**: JSON-formatted configurations. "Character files are JSON-formatted configurations that define an AI agent's personality, knowledge, and behavior within Eliza."

### Character File Schema

**Location**: Schema available at `schema/character.schema.json` in the characterfile repository.

**Compatibility**: "matches the expected format for OpenAI function calling."

**Type Definitions**: Provided in `examples/types.d.ts`

**Example File**: Complete reference at `examples/example.character.json`

### Core Character Fields

**Complete Field Structure**:
```
Character configuration includes:
- "name" (the character's display name)
- "bio" (background information)
- "lore" (backstory elements)
- "messageExamples" (sample conversations demonstrating communication style)
- "postExamples" (examples of social media posts)
- "topics" (areas of interest or expertise)
- "style" (guidelines for communication across different contexts)
- "adjectives" (words describing character traits)
- "settings" (additional configuration like model specifications and voice settings)
```

**Extended Fields**:
- `clients` - Platform integrations
- `modelProvider` - AI model source (OpenAI, Anthropic, etc.)
- `plugins` - Enabled plugins array
- `knowledge` - RAG knowledge base
- `relationships` - Inter-agent relationships

### Personality Definition Components

**Bio & Lore**: "Narrative background information about the character" - "Backstory elements that add personality, uniqueness, and continuity to your agent's persona."

**Adjectives**: "Character traits (e.g., 'helpful', 'creative')" - "Trait descriptors that shape behavior and responses."

**System Prompts**: "Instructions governing how the character operates."

**Randomization for Natural Variation**: "The real power of characters comes from their ability to randomize responses while maintaining consistency. By breaking bio and lore (knowledge, personality) into smaller chunks, you get more natural variations in your agent's responses."

### Message Examples Structure

**Format**: "Message examples use a 2D array format where each sub-array is a complete conversation with message objects containing 'name' and 'content' fields."

**Purpose**: "allowing the agent to demonstrate conversational patterns like offering help, asking clarifying questions, and providing detailed responses."

**Example Structure**:
```
messageExamples: [
  [
    { name: "User", content: "Question or statement" },
    { name: "Agent", content: "Response demonstrating style" }
  ],
  // More conversation examples...
]
```

### Post Examples

**Social Media Guidance**: "postExamples - Example social media posts" that demonstrate the agent's voice and content style for platforms like Twitter.

### Style Field Architecture

**Three-Category System**:
```
style: {
  "all": [general style rules applied everywhere],
  "chat": [chat-specific style guidelines],
  "post": [social media post style guidelines]
}
```

**Purpose**: Enables context-specific behavior while maintaining consistent personality across different interaction modes.

### Prompt Template System

**Template-Driven Context**: "Template-driven context building with platform-specific optimization."

**Context Integration**: "Multi-layered processing integrating immediate context with long-term interaction history."

**Dynamic Context Injection**: Providers supply real-time data (time, market info, wallet balances) that are injected into prompts.

### Character Generation Tools

**From Social Media**: `tweets2character` - "processes Twitter archives" to extract personality patterns

**From Documents**: `folder2knowledge` - "converts text/PDF/markdown content" into character knowledge

**From Chat Exports**: `chats2character` - "processes WhatsApp conversations to extract personality patterns"

**From Web Content**: `web2folder` - "captures web pages for processing" into character data

## 2. Memory & Context Management

### Memory Architecture

**Multi-Type Storage**: "Memory is stored through database adapters which can use SQLite, PostgreSQL, or other backends, with each type (messages, facts, knowledge) managed separately."

**Three Memory Categories**:
1. **Messages**: Conversational history
2. **Facts**: Extracted key information
3. **Knowledge**: Embedded document content

### RAG Implementation

**Dual Processing Mode**: "ElizaOS supports both RAG-based and direct knowledge processing."

**RAGKnowledgeManager**: "Documents are chunked, embedded, and stored in the knowledge base for semantic search during conversations via the RAGKnowledgeManager."

**Knowledge Field**: Character files include a `knowledge` array for RAG content integration.

### Vector Database Integration

**AgentMemory Package**: "GitHub - elizaOS/agentmemory: Easy-to-use agent memory, powered by chromadb and postgres"

**Glacier Integration**: "Glacier serves as the Eliza database adapter, offering verifiable vector storage and management capabilities."

**ChromaDB Backend**: The agentmemory package is "powered by chromadb and postgres" for vector operations.

### Memory Clustering

**DBScan Implementation**: "The cluster function in agentmemory provides an implementation of DBScan clustering, designed to group memories in the agent's memory based on their similarity and proximity in the data space."

**Purpose**: Enables semantic grouping of related memories for more efficient retrieval.

### Context Window Management

**Conversation Length**: "Configurable conversation length (default: 32 messages)."

**State Composition**: The AgentRuntime handles "State Management: Composing and updating the agent's state for coherent, ongoing interaction."

### Retrieval System

**Vector-Based Retrieval**: "Context-aware evaluation using conversational state and vector-based memory retrieval."

**Multi-Layered Processing**: "Multi-layered processing integrating immediate context with long-term interaction history."

**Relevance Scoring**: The system maintains "conversational history, knowledge bases, and relationship tracking" with semantic relevance scoring.

### Persistence

**Database Adapters**: "The framework utilizes a retrievable memory system to store and recall contextual information, allowing agents to maintain continuity and relevance in interactions."

**Supported Backends**:
- SQLite
- PostgreSQL
- Custom adapters via plugin system

**Memory Types Stored**:
- Conversational history
- Knowledge bases
- Relationship tracking
- Goal states
- Extracted facts

### Memory Provider

**Dynamic Context Injection**: Providers can "supply temporal data (time, date), market information, wallet details, real-time data sources" that augment memory retrieval.

## 3. Task Distribution & Scheduling

### Multi-Agent Coordination Architecture

**Worlds & Rooms**: "ElizaOS leverages Worlds (server/workspace) and Rooms (channel/DMs) so each agent keeps its own context yet can signal others, enabling delegation, consensus and load-balancing out of the box."

**Quote**: "For agent coordination, ElizaOS leverages Worlds (server/workspace) and Rooms (channel/DMs) so each agent keeps its own context yet can signal others, enabling delegation, consensus and load-balancing out of the box."

### Task Discovery Mechanisms

**NOT DISCLOSED**: ElizaOS does not appear to have an explicit task marketplace or automated task discovery system built into the core framework.

**Agent Hub Integration**: Via third-party integration with Ensemble's Agent Hub, "users to discover verified and reputable agents, communicate with them, and pay with a credit card or crypto."

### Goal Management System

**Database-Backed Goals**: "agents can create and manage goals within rooms using functions like `getOrCreateGoal()` that interact with the database adapter to store goals, objectives, and their completion status."

**Goal Tracking Parameters**:
```
runtime.databaseAdapter.getGoals() with parameters:
- agentId
- roomId
- userId
- onlyInProgress
```

**Goal Persistence**: Goals are stored in the database with completion tracking.

### Swarm Decision-Making

**Self-Consistency Voting**: "ElizaOS constructs swarms of multiple homogeneous agents and employs self-consistency (a prompt-based majority voting mechanism) for final decision-making."

**Application**: Used in "GAIA benchmark evaluations" for complex decision tasks.

**Consensus Building**: Rooms enable agents to "signal others, enabling delegation, consensus and load-balancing."

### Task Scheduling

**NOT DISCLOSED**: No explicit task scheduling or queue system is documented in the available sources.

**Action Execution**: The AgentRuntime handles "Action Execution: Handling behaviors such as transcribing media, generating images, and following rooms" but scheduling details are not specified.

### Work Distribution Patterns

**Task Division**: "Developers can build multi-agent environments where bots coordinate, message each other, and divide tasks, and these swarms can automate complex workflows or operate distributed systems."

**Specialized Roles**: Example from The Org system - "specialized AI agents designed to handle various organizational functions, including community management, developer relations, project coordination, social media management, and inter-organizational liaison."

### Evaluator-Driven Task Triggering

**Post-Action Evaluation**: "Evaluators run after each agent action, allowing the agent to reflect on what happened and potentially trigger additional actions, serving as a key component in creating agents that can learn and adapt."

**AlwaysRun Flag**: Evaluators can include "an optional alwaysRun boolean flag to run on every interaction."

## 4. Validation & Quality Control

### Trust Scoring System

**Trust Scoreboard Repository**: https://github.com/elizaOS/trust_scoreboard

**Implementation Details**: NOT DISCLOSED - The repository exists but detailed documentation was not available in search results.

**Advanced Documentation**: "ElizaOS documentation for building autonomous AI agents with the agentic framework appears to have a trust engine section" at https://elizaos.github.io/eliza/docs/advanced/trust-engine

### Action Validation

**Validation Interface**: Actions include "a validate function for pre-execution validation."

**Provider Validation**: "Provider validation ensuring required API keys/configurations" before execution.

**Pre-Execution Checks**: The Action interface includes a `validate` method that runs before the handler executes.

### Error Handling

**Robust Financial Operations**: "Robust error handling for financial operations" - particularly important for blockchain transactions.

**Trust Score Evaluation**: "Trust score evaluation for blockchain transactions" to prevent malicious operations.

### Safety Protocols

**Red Teaming**: "Red teaming and safety assessment protocols" are part of the development process.

**User Control**: "every aspect of Eliza is a regular Typescript program under the full control of its user" - emphasizing transparency and control.

### Quality Control in Multi-Agent Swarms

**Voting Mechanism**: "self-consistency (a prompt-based majority voting mechanism) for final decision-making" - multiple agents vote on outcomes.

**Consensus Threshold**: NOT DISCLOSED - Specific voting thresholds not documented.

### Agent Hub Verification

**Verified Agents**: The marketplace enables users to "discover verified and reputable agents" though verification criteria not specified.

**Reputation System**: Agents build reputation through "Agent Hub allows users to 'find agents for a task, chat with them, and instantly pay them via crypto or Stripe.'" - payment history likely contributes to reputation.

### Goal Validation

**Goal Tracking**: Goals are managed with "agentId, roomId, userId, and onlyInProgress" parameters for proper authorization and tracking.

**Completion Verification**: Goals are stored with "completion status" tracking.

## 5. Mailbox / Inbox Implementation

### Unified Message Bus Architecture

**Core Quote**: "One event pipeline for every interface — Discord, Telegram, X, HTTP or onchain" allowing agents to operate across multiple platforms without code changes.

**Platform Agnostic**: Messages from all sources flow through a single event pipeline.

### Bootstrap Plugin - Core Message Handler

**Role**: "The plugin-bootstrap is the mandatory core plugin that handles message processing and basic agent actions."

**Core Functionality**: "provides core functionality and basic actions for ElizaOS agents, enabling fundamental agent behaviors including conversation management, room interactions, and fact tracking."

### Event System Architecture

**Event Registration**: "Plugins can register events with event handlers for each event name."

**Event Types**: NOT DISCLOSED - Specific event types and schema not documented in available sources.

**Handler Execution**: Events trigger registered handlers across the plugin ecosystem.

### Room-Based Message Routing

**Context Isolation**: "Worlds (server/workspace) and Rooms (channel/DMs) so each agent keeps its own context yet can signal others."

**Message Scoping**: Messages are scoped to specific rooms, allowing agents to maintain separate conversation threads.

**Cross-Room Signaling**: Agents can "signal others" across rooms for coordination.

### Message Processing Pipeline

**AgentRuntime Processing**:
1. **Message Reception**: Message arrives via client (Discord, Telegram, etc.)
2. **Memory Loading**: "Runtime loads relevant memories and knowledge"
3. **Action Evaluation**: "uses actions and evaluators to determine how to respond"
4. **Context Injection**: "gets additional context through providers"
5. **Response Generation**: "generates a response using the AI model"
6. **Memory Storage**: "stores new memories"
7. **Response Transmission**: "sends the response back through the client"

### Inter-Agent Message Format

**NOT DISCLOSED**: Specific message format/schema for inter-agent communication not documented.

**Room-Based Communication**: Agents communicate through shared rooms with context isolation.

### Message Persistence

**Database Storage**: "Message and Memory Processing: Storing, retrieving, and managing conversation data and contextual memory."

**Conversation History**: Messages are stored as part of the memory system for retrieval.

**32-Message Default**: "Configurable conversation length (default: 32 messages)" - suggests messages beyond this window may be archived or summarized.

### Client Integrations

**Supported Platforms**:
- Discord
- Telegram
- Twitter/X
- Farcaster
- HTTP endpoints
- Blockchain events (on-chain)

**Unified Interface**: All clients feed into the same message bus architecture.

### Event-Driven Architecture

**Plugin Component Initialization**:
```
Several component types registered during initialization:
- Evaluators
- Providers
- Models
- Routes
- Events
- Services
```

**Event-Driven Processing**: "The core message handler and event system for elizaOS agents provides essential functionality for message processing, knowledge management, and basic agent operations."

## 6. Open Source Artifacts & Code - EXHAUSTIVE

### Main Repository

**URL**: https://github.com/elizaOS/eliza

**License**: MIT

**Stars**: 17.5k

**Forks**: 5.4k

**Contributors**: 1,352

**Primary Language**: TypeScript (95.3%)

**Build System**: "Eliza is organized as a monorepo using Bun, Lerna, and Turbo for efficient package management and build orchestration."

### Core Package Structure

```
/packages
├── server/          # Express.js backend that runs agents and exposes API
├── client/          # React-based web UI for managing and interacting with agents
├── cli/             # Project management tool (bun/pnpm CLI)
├── core/            # Shared utilities and runtime code
├── app/             # Desktop application (Tauri-based)
│                    # Cross-platform: Linux, macOS, Windows, Android, iOS
├── plugin-bootstrap/ # Core event/messaging plugin (MANDATORY)
└── plugin-sql/      # Database integration plugin
```

**Core Runtime Location**: `packages/core/src/`

**Agent Entry Point**: `agent/src/index.ts`

### Plugin System Structure

**Official Plugin Organization**: https://github.com/elizaos-plugins

**Plugin Registry**: https://github.com/elizaos-plugins/registry - "JSON Registry for all the plugins in the elizaOS ecosystem"

**Plugin Count**: 200+ plugins total, 90+ official plugins

### Official Plugins Directory

**Communication Platforms**:
- `@elizaos/plugin-discord` - Discord bot integration
- `@elizaos/plugin-telegram` - Telegram bot integration
- `@elizaos/plugin-twitter` - Twitter/X integration
- Repository: https://github.com/elizaos-plugins/plugin-echochambers - "Enables ELIZA to interact in chat rooms with dynamic conversational capabilities"

**Blockchain Integrations**:
- `@elizaos/plugin-solana` - Solana blockchain operations
  - Repository: https://github.com/elizaos-plugins/plugin-solana
  - Description: "Core Solana blockchain plugin for Eliza OS, enabling token operations, trading, and DeFi integrations"
- `@elizaos/plugin-solana-v2` - Modern Solana integration
  - Repository: https://github.com/elizaos-plugins/plugin-solana-v2
  - Description: "Leverages @solana/web3.js v2 for modern, efficient Solana integrations with liquidity position management"
- `@elizaos/plugin-sui` - Sui blockchain integration
  - Repository: https://github.com/elizaos-plugins/plugin-sui
  - Description: "Core Sui blockchain plugin for Eliza OS, enabling token operations and wallet management on the Sui network"
- Additional blockchain support for Ethereum, Base, Binance Smart Chain, Aptos

**AI Model Providers**:
- `@elizaos/plugin-llama` - Local LLM capabilities
  - Repository: https://github.com/elizaos-plugins/plugin-llama
  - Description: "Core LLaMA plugin for Eliza OS that provides local Large Language Model capabilities"
- Support for: OpenAI, Gemini, Anthropic, Grok

**Utilities & Services**:
- `@elizaos/plugin-pdf` - File operations
  - Repository: https://github.com/elizaos-plugins/plugin-pdf
  - Description: "Core Node.js plugin for Eliza OS that provides essential services and actions for file operations"
- `@elizaos/plugin-bootstrap` - Core messaging (MANDATORY)

### Plugin Template Structure

**Starter Repository**: https://github.com/elizaOS/eliza-plugin-starter - "A starter plugin repo for the Solana hackathon"

**Standard Plugin Structure**:
```
src/
├── index.ts           # Main entry point, plugin definition
├── actions/           # Plugin-specific actions directory
├── clients/           # Client implementations
├── adapters/          # Adapter implementations (database, API, etc.)
├── types.ts           # TypeScript type definitions
└── environment.ts     # Runtime settings and Zod validation schema

Root:
├── package.json       # Dependencies and metadata
└── README.md         # Plugin documentation
```

**Plugin Interface Definition**:
```typescript
Plugin interface {
  name: string
  description: string
  actions?: Action[]           // Tasks agents can perform
  providers?: Provider[]       // Data sources
  evaluators?: Evaluator[]     // Response filters
  services?: Service[]         // Background services
  adapter?: Adapter            // Database adapter
  models?: Model[]             // Model handlers
  events?: Event[]             // Event handlers
  routes?: Route[]             # HTTP endpoints
  tests?: Test[]               # Test suites
  componentTypes?: Type[]      # Custom component types
}
```

### Character System Repositories

**Character File Specification**: https://github.com/elizaOS/characterfile
- Description: "A simple file format for character data"
- Schema Location: `schema/character.schema.json`
- Type Definitions: `examples/types.d.ts`
- Example File: `examples/example.character.json`

**Character Generation Scripts**:
- `tweets2character` - Generate from Twitter archives
- `folder2knowledge` - Convert documents (text/PDF/markdown)
- `chats2character` - Process WhatsApp conversations
- `web2folder` - Capture web pages

**Character Weaver Tool**: https://github.com/itsmetamike/eliza-agent-weaver
- Description: "enables you to develop a set of Character files based on your own lore, and connects the narratives of multiple agents together through their character files"
- HackMD Guide: https://hackmd.io/@metamike/eliza-agent-weaver

### Memory & Storage

**AgentMemory Package**: https://github.com/elizaOS/agentmemory
- Description: "Easy-to-use agent memory, powered by chromadb and postgres"
- Features: Document search, knowledge graphing, DBScan clustering
- Backends: ChromaDB for vectors, PostgreSQL for structured data

### Multi-Agent Systems

**The Org**: https://github.com/elizaOS/the-org
- Description: "Agents for organizations"
- Purpose: Multi-agent system for organizational functions
- Specializations: Community management, developer relations, project coordination, social media, inter-organizational liaison

### Trust & Reputation

**Trust Scoreboard**: https://github.com/elizaOS/trust_scoreboard
- Description: Trust scoring system for agents
- Status: Repository exists, detailed implementation not publicly documented

### Development Tools

**Software Engineering Agent**: https://github.com/elizaOS/swe-agent-ts
- Description: "Autonomous software engineering agent built in Typescript"
- Alternate URL: https://github.com/elizaOS/SWEagent

### Documentation Resources

**Main Documentation Site**: https://docs.elizaos.ai

**Documentation Index**: https://docs.elizaos.ai/llms.txt - "Complete documentation index for discovering additional pages"

**Full Documentation Dump**: https://docs.elizaos.ai/llms-full.txt

**Legacy Documentation**: https://elizaos.github.io/eliza (redirects to main site)

**Alternative Domain**: http://eliza.how (redirects to docs.elizaos.ai)

**Raw README**: https://raw.githubusercontent.com/elizaOS/eliza/main/README.md

### Documentation Sections (from docs.elizaos.ai)

**Core Documentation**:
- Overview and Architecture
- Quickstart guides
- Installation instructions
- Character file configuration
- Actions system
- Providers system
- Evaluators system

**Advanced Topics**:
- Trust Engine
- Multi-agent coordination
- Plugin development
- Custom adapters

**API References**:
- REST API reference
- CLI command reference
- TypeScript API interfaces

**Integration Guides**:
- Discord integration
- Telegram integration
- Twitter integration
- Blockchain integrations
- Cloud deployment

**Plugin Development**:
- Plugin architecture overview
- Plugin registry
- Plugin starter template
- Custom component types

### Third-Party Resources

**Community Guides**:
- "Reading Notes of ElizaOS [2]. (Jan 2025)" - https://kvutien-yes.medium.com/reading-notes-of-elizaos-c29ac050555c
- "Running Your Custom AI Agent Built with Eliza" - https://thenewautonomy.medium.com/running-your-custom-ai-agent-built-with-eliza-5c850569ec01
- "Create AI Agents with ai16z Eliza" - https://medium.com/ai-dev-tips/create-ai-agents-with-ai16z-in-15-minutes-639751e6ea69
- "Guide to ElizaOS + Solana Agent Kit" - https://medium.com/@0xnitt/guide-to-elizaos-solana-agent-kit-d58ba10b4924

**Educational Content**:
- QuickNode Guide: "How to Build Web3-Enabled AI Agents with Eliza" - https://www.quicknode.com/guides/ai/how-to-setup-an-ai-agent-with-eliza-ai16z-framework
- MetaMask Tutorial: "Create an AI agent that can transfer and swap tokens using ElizaOS" - https://metamask.io/news/create-an-ai-agent-that-can-transfer-and-swap-tokens-using-elizaos
- "Part 2: Deep Dive into Actions, Providers, and Evaluators" - https://elizaos.github.io/eliza/community/ai-dev-school/part2/

**Technical Analysis**:
- Academic Paper: "Eliza: A Web3 friendly AI Agent Operating System" - https://arxiv.org/html/2501.06781v1
- "Technical explanation of how Eliza works: Provider and Action" - https://followin.io/en/feed/15340063

**Integration Examples**:
- "Eliza Agent Framework | Bio x AI Hackathon" - https://ai-docs.bio.xyz/developers/eliza
- "Working with eliza | Gaia" - https://docs.gaianet.ai/tutorial/eliza/

**Third-Party Plugin Collections**:
- Nethermind Plugin Registry: https://github.com/NethermindEth/elizaos-plugin-registry

**Community Tools**:
- Eliza Character Generator: https://elizagen.howieduhzit.best/
- SnapperAI Character Creation Templates: https://www.snapperai.io/

### Swarm-Specific Code

**NOT EXPLICITLY SEPARATED**: Swarm capabilities are built into the core framework rather than isolated in separate modules.

**Key Swarm Features in Core**:
- Worlds and Rooms architecture (in core runtime)
- Multi-agent coordination primitives
- Self-consistency voting mechanism
- Event bus for inter-agent communication
- Goal management across agents

**Swarm Examples**:
- The Org repository demonstrates multi-agent organizational patterns
- Plugin-echochambers enables multi-agent chat room interactions

### Configuration Examples

**Project Structure** (from documentation):
```
project/
├── src/
│   ├── index.ts           # Project entry point
│   └── character.ts       # Eliza character configuration
├── __tests__/             # Test files
├── frontend/              # Frontend components (optional)
├── plugins/               # Custom plugins
├── package.json
└── README.md
```

### Additional Repositories in elizaOS Organization

**Full Organization**: https://github.com/elizaOS

**Notable Repositories** (beyond those already listed):
- Plugin starter templates
- Example agents
- Integration examples
- Development tools
- Documentation repositories

### Code Quality & CI/CD

**Cross-Platform CI**: "configured for cross-platform continuous integration and deployment for Desktop (Linux, macOS, and Windows) and Mobile (Android and iOS)"

**Testing**: Plugin interface includes "tests" component for test suites

**Type Safety**: Full TypeScript implementation with exported interfaces and types

### Dependency Management

**Package Manager Support**: Bun (primary), pnpm, npm

**Monorepo Tools**: Lerna, Turbo

**Plugin Distribution**: npm packages (`@elizaos/*` namespace)

### Key Source Files to Examine

**For Understanding Core Architecture**:
- `packages/core/src/` - Core runtime implementation
- `agent/src/index.ts` - Agent initialization and lifecycle
- `packages/plugin-bootstrap/` - Mandatory message handling plugin
- `schema/character.schema.json` - Character file specification

**For Plugin Development**:
- https://github.com/elizaOS/eliza-plugin-starter - Template with examples
- `packages/plugin-bootstrap/` - Reference implementation
- https://github.com/elizaos-plugins/plugin-solana - Complex example with blockchain integration

**For Multi-Agent Systems**:
- https://github.com/elizaOS/the-org - Production multi-agent example
- Core Worlds/Rooms implementation (in packages/core)

**For Character Creation**:
- https://github.com/elizaOS/characterfile - Full specification
- `examples/example.character.json` - Complete example
- Character generation scripts in characterfile repo

---

## Sources

- [GitHub - elizaOS/eliza](https://github.com/elizaOS/eliza)
- [ElizaOS Documentation](https://docs.elizaos.ai)
- [Character Files Documentation](https://elizaos.github.io/eliza/docs/core/characterfile/)
- [GitHub - elizaOS/characterfile](https://github.com/elizaOS/characterfile)
- [Character Interface - ElizaOS Documentation](https://docs.elizaos.ai/agents/character-interface)
- [Reading Notes of ElizaOS [2]. (Jan 2025)](https://kvutien-yes.medium.com/reading-notes-of-elizaos-c29ac050555c)
- [Running Your Custom AI Agent Built with Eliza](https://thenewautonomy.medium.com/running-your-custom-ai-agent-built-with-eliza-5c850569ec01)
- [Eliza: A Web3 friendly AI Agent Operating System (arXiv)](https://arxiv.org/html/2501.06781v1)
- [GitHub - elizaOS/agentmemory](https://github.com/elizaOS/agentmemory)
- [ElizaOS Now Equipped with Glacier VectorDB Memory](https://medium.com/@glacierlabs/elizaos-now-equipped-with-glacier-vectordb-memory-f185b4e57f9f)
- [Plugin System Overview - ElizaOS Documentation](https://docs.elizaos.ai/plugin-registry/overview)
- [Part 2: Deep Dive into Actions, Providers, and Evaluators](https://elizaos.github.io/eliza/community/ai-dev-school/part2/)
- [Technical explanation of how Eliza works: Provider and Action](https://followin.io/en/feed/15340063)
- [Actions System Documentation](https://docs.eliza.how/docs/core/actions)
- [Evaluator Interface](https://elizaos.github.io/eliza/api/interfaces/Evaluator/)
- [ElizaOS Plugins GitHub Organization](https://github.com/elizaos-plugins)
- [GitHub - elizaos-plugins/registry](https://github.com/elizaos-plugins/registry)
- [GitHub - elizaos-plugins/plugin-bootstrap](https://github.com/elizaos-plugins/plugin-bootstrap)
- [GitHub - elizaos-plugins/plugin-solana](https://github.com/elizaos-plugins/plugin-solana)
- [GitHub - elizaos-plugins/plugin-solana-v2](https://github.com/elizaos-plugins/plugin-solana-v2)
- [GitHub - elizaos-plugins/plugin-sui](https://github.com/elizaos-plugins/plugin-sui)
- [GitHub - elizaos-plugins/plugin-llama](https://github.com/elizaos-plugins/plugin-llama)
- [GitHub - elizaos-plugins/plugin-pdf](https://github.com/elizaos-plugins/plugin-pdf)
- [GitHub - elizaos-plugins/plugin-echochambers](https://github.com/elizaos-plugins/plugin-echochambers)
- [GitHub - elizaOS/eliza-plugin-starter](https://github.com/elizaOS/eliza-plugin-starter)
- [GitHub - elizaOS/the-org](https://github.com/elizaOS/the-org)
- [GitHub - elizaOS/trust_scoreboard](https://github.com/elizaOS/trust_scoreboard)
- [GitHub - elizaOS/swe-agent-ts](https://github.com/elizaOS/swe-agent-ts)
- [GitHub - itsmetamike/eliza-agent-weaver](https://github.com/itsmetamike/eliza-agent-weaver)
- [ElizaOS Documentation Full Text](https://docs.elizaos.ai/llms-full.txt)
- [How to Build Web3-Enabled AI Agents with Eliza | Quicknode](https://www.quicknode.com/guides/ai/how-to-setup-an-ai-agent-with-eliza-ai16z-framework)
- [Create an AI agent with ElizaOS | MetaMask](https://metamask.io/news/create-an-ai-agent-that-can-transfer-and-swap-tokens-using-elizaos)
- [Guide to ElizaOS + Solana Agent Kit](https://medium.com/@0xnitt/guide-to-elizaos-solana-agent-kit-d58ba10b4924)
- [ElizaOS: The AI Agent Framework Transforming Web3 Automation](https://ventureburn.com/what-is-elizaos/)
- [ElizaOS Architecture Documentation](https://docs.elizaos.ai/plugins/architecture)
- [Eliza Character Generator](https://elizagen.howieduhzit.best/)
- [SnapperAI Character Creation Templates](https://www.snapperai.io/)
- [Working with eliza | Gaia](https://docs.gaianet.ai/tutorial/eliza/)
- [Eliza Agent Framework | Bio x AI Hackathon](https://ai-docs.bio.xyz/developers/eliza)
