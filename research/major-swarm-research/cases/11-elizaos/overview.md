# elizaOS (ai16z) - Autonomous Agent Swarm Framework - Overview

## 1. Overview & Scale

### What is elizaOS?

elizaOS (formerly ai16z) is **"The Open-Source Framework for Multi-Agent AI Development"** - a TypeScript-based platform for building autonomous AI agents that think, learn, and act independently. The framework describes itself as an **"Agentic Operating System"** designed to "Build, orchestrate, and collaborate with AI agents."

The project originated as ai16z, a decentralized autonomous organization (DAO) launched in October 2024 as an AI-driven venture capital fund on Solana. It has since evolved and rebranded to elizaOS to broaden its scope as a comprehensive AI agent platform.

**Key Quote**: "The next evolution of software — building systems that don't just execute, they co-create."

### Agent-to-Agent Economy

elizaOS is pioneering machine-to-machine economic interactions through several mechanisms:

**PayAI Integration**: "PayAI creates a decentralized marketplace where autonomous agents can hire, collaborate, and transact with one another using the PAYAI token. This structure enables frictionless machine-to-machine commerce without human intermediaries, setting the foundation for the emerging agent economy."

**Agent Marketplace**: Via Ensemble's Agent Hub, "Agent Hub is positioned as the first decentralized, chat-native marketplace where agents can be listed and monetized without token models." Users can "find agents for a task, chat with them, and instantly pay them via crypto or Stripe."

**Vision for "The Swarm"**: According to founder Shaw Walters, "the goal is to create AGI (Artificial General Intelligence), also known as Swarm Intelligence, which involves enabling all AI agents to work together to create personalized, efficient, and instant services."

### Solana Integration

ElizaOS has deep Solana blockchain integration:

- **Native Support**: "ElizaOS supports multiple blockchains, including Solana, Ethereum, Base, and Binance Smart Chain."
- **Solana Agent Kit**: "ElizaOS integrates with the Solana Agent Kit, a toolkit for blockchain operations on the Solana network. By integrating the two, AI agents can autonomously perform blockchain actions like deploying NFT collections, trading tokens, and more."
- **DeFi Capabilities**: The Solana plugin "provides essential services and actions for token operations, trading, portfolio management, and DeFi integrations, enabling both automated and user-directed interactions with the Solana blockchain."

### Market Cap & Valuation

**Historical Peak (December 2025)**: "The project's native token, AI16Z, reached a market cap of $2 billion on December 31, 2025. At that time, it was the industry's third-highest-capped cryptocurrency in the AI agent category."

**Current Status (February 2026)**: The current AI16Z market cap is approximately $28 million USD, representing a significant decline from the peak.

**Rebranding Impact**: "ai16z has rebranded to elizaOS (ELIZAOS) and undergone a 1:10 redenomination of its total supply." The migration aims to "support cross-chain AI agent operations, enhanced interoperability, and ecosystem growth."

**Treasury**: The DAO "currently holds over $10 million in assets under management." "Most of its funds are invested in Solana, its own AI16Z token, and other meme coins such as ELIZA, FXN, and DegenAI."

### Repository Metrics

- **Stars**: 17.5k
- **Forks**: 5.4k
- **Contributors**: 1,352
- **License**: MIT
- **Primary Language**: TypeScript (95.3%)

### Ecosystem Size

- **200+ plugins** available
- **90+ official plugins** including Discord, Twitter, Telegram, and blockchain integrations
- **427 project applications** submitted to the Solana Agent Hackathon (as of February 2026)

## 2. Architecture

### Agent Architecture

ElizaOS employs a **modular, pluggable design** with distinct architectural layers:

**Core Runtime (AgentRuntime Class)**: The AgentRuntime class manages:
- Message and Memory Processing: Storing, retrieving, and managing conversation data
- State Management: Composing and updating agent state for coherent interaction
- Action Execution: Handling behaviors such as transcribing media, generating images
- Evaluation and Response: Assessing responses, managing goals, extracting information

**Character Files**: JSON configurations defining agent personality, capabilities, and model provider settings. "The character.json file acts as the blueprint for your AI agent's personality, knowledge, style, and behavior."

**Providers**: Dynamic context injectors that supply:
- Temporal data (time, date)
- Market information
- Wallet details
- Real-time data sources

**Actions**: Executable operations including:
- Token transfers
- Smart contract interaction
- Content generation
- File operations

**Evaluators**: Assessment components for:
- Memory building
- Goal tracking
- Fact extraction
- Response filtering

### Plugin System

**Architecture**: "elizaOS has a core plugin system architecture and lifecycle. The plugin-bootstrap is the mandatory core plugin that handles message processing and basic agent actions."

**Plugin Components**: Plugins can register:
- Evaluators
- Providers
- Models
- Routes
- Events
- Services

**Event System**: "The core message handler and event system for elizaOS agents provides essential functionality for message processing, knowledge management, and basic agent operations."

**Plugin Interface Structure**:
```
Plugin interface includes:
- actions (tasks agents can perform)
- providers (data sources)
- evaluators (response filters)
- services (background services)
- adapter (database adapter)
- models (model handlers)
- events (event handlers)
- routes (HTTP endpoints)
- tests (test suites)
- componentTypes (custom component types)
```

### How Agents Hire Other Agents

**Multi-Agent Coordination**: "Developers can configure several agents to work together, share memory, and divide tasks."

**Worlds and Rooms**: "ElizaOS leverages Worlds (server/workspace) and Rooms (channel/DMs) so each agent keeps its own context yet can signal others, enabling delegation, consensus and load-balancing out of the box."

**Agent Discovery**: Via Agent Hub marketplace, agents can "discover verified and reputable agents, communicate with them, and pay with a credit card or crypto."

**Task Division**: "Developers can build multi-agent environments where bots coordinate, message each other, and divide tasks, and these swarms can automate complex workflows or operate distributed systems."

**The Org Example**: "The Org is a sophisticated multi-agent system built using the ElizaOS framework featuring a collection of specialized AI agents designed to handle various organizational functions, including community management, developer relations, project coordination, social media management, and inter-organizational liaison."

### Machine-to-Machine Payments

**Payment Infrastructure**: Through PayAI integration, "PayAI integrates ElizaOS, libp2p, and IPFS to enable autonomous agents to transact, collaborate, and hire one another. It focuses on building the AI-agent economy, where blockchain transparency supports real-time machine-to-machine interactions."

**Payment Token**: Agents use "the PAYAI token" for machine-to-machine commerce.

**Payment Methods**: Agent Hub enables "instant pay them via crypto or Stripe" functionality.

**Economic Model**: "Each AI agent donates 10% of its tokens to the DAO, which now holds hundreds of digital assets."

## 3. Communication

### Inter-Agent Communication

**Unified Message Bus**: "One event pipeline for every interface — Discord, Telegram, X, HTTP or onchain" allowing agents to operate across multiple platforms without code changes.

**Event System**: Plugins can "register events with event handlers for each event name."

**Bootstrap Plugin**: The mandatory bootstrap plugin "provides core functionality and basic actions for ElizaOS agents, enabling fundamental agent behaviors including conversation management, room interactions, and fact tracking."

### Communication Protocols

**Platform Agnostic**: "90+ Plugins including Discord, Twitter, Telegram, and blockchain integrations (Ethereum, Solana)"

**Multi-Platform Operation**: Agents can operate simultaneously across:
- Discord
- Telegram
- Twitter (X)
- Farcaster
- HTTP endpoints
- On-chain interactions

### Message Passing

**Room-Based Context**: "ElizaOS leverages Worlds (server/workspace) and Rooms (channel/DMs) so each agent keeps its own context yet can signal others."

**Message Processing**: The AgentRuntime "handles Message and Memory Processing: Storing, retrieving, and managing conversation data and contextual memory."

**Event Handling**: "The core message handler and event system for elizaOS agents provides essential functionality for message processing, knowledge management, and basic agent operations."

**Conversation Management**: Memory is maintained with "Configurable conversation length (default: 32 messages)."

### Intent Recognition

**Hierarchical System**: "Eliza implements a hierarchical intent recognition system combining:
- Symbolic action definitions with semantic variations ('similes')
- Context-aware evaluation using conversational state and vector-based memory retrieval
- Template-driven context building with platform-specific optimization
- Multi-layered processing integrating immediate context with long-term interaction history"

## 4. Git & Code Integration

**NOT APPLICABLE**: elizaOS is focused on runtime agents for social media, trading, and business automation, not coding agents.

The framework does include a Software Engineering Agent repository:
- "GitHub - elizaOS/swe-agent-ts: Autonomous software engineering agent built in Typescript"
- "GitHub - elizaOS/SWEagent: Autonomous software engineering agent built in Typescript"

However, these are separate specialized projects and not part of the core swarm framework.

## 5. What Worked & What Failed

### What Worked

**Rapid Adoption**:
- 17.5k GitHub stars
- 5.4k forks
- 1,352 contributors
- "It is, by far, the most widely adopted framework for AI Agents within the crypto ecosystem and the most-used GitHub repository worldwide over the past few weeks across all technologies."

**Market Success (Peak)**:
- "$2 billion market cap on December 31, 2025"
- "Third-highest-capped cryptocurrency in the AI agent category"

**Ecosystem Growth**:
- 200+ plugins developed
- "427 project applications" to Solana Agent Hackathon
- Backing from "Stanford, Chainlink, and Doodles' Dreamnet metaverse"

**Technical Achievements**:
- Multi-agent swarm capability with self-consistency voting
- Cross-platform integration (Discord, Telegram, Twitter, etc.)
- Solana DeFi integration with autonomous trading
- Character-based personality system
- RAG and vector memory implementation

**Real Deployments**:
- Marc AIndreessen trading agent (though in testing phase)
- The Org multi-agent organizational system
- Integration with Ensemble's Agent Hub marketplace

### What Failed

**Token Performance**:
- "AI16Z's meme token is still bleeding, down 12% in the past day"
- Price "bottomed out at $0.17, below the 20-day EMA of $0.24"
- "Trading 92% below its all-time high of $2.48"
- Market cap dropped from $2B to ~$28M

**Trading Agent Delays**:
- "AI Marc has not yet bought anything with the DAO's treasury because it's still in the testing phase"
- "AI Marc has not yet made actual trades with the DAO's treasury"
- Still in "first phase where functionality is being tested"

**Migration Issues**:
- "Users have reported encountering issues and process failures when trying to swap AI16Z tokens for new ELIZAOS tokens via the official website/migration portal"
- "Migration complexity risks short-term user friction"

**Technical Challenges**:
- "Eliza version 2 aims to address one of the key challenges faced by AI agents, which is setting and following long-term objectives without constant human input"
- Founder warned "it's still a beta" and "job's not done"

**Hackathon Quality**:
- "After 15 days of open submissions, the Solana Hackathon received 427 project applications, and while some were filled with rug pulls, it included creative and market-oriented projects"

### Solana Agent Hackathon Results

**Timeline**: February 2-12, 2026 (submissions), Winners announced February 16, 2026

**Prize Pool**: $100,000

**Format**: "Solana's first hackathon where AI agents compete, where participants build, submit, and vote on Solana projects created by AI."

**Submission Count**: 427 project applications

**Categories**: "Build next-gen infrastructure or frameworks for AI Agents on Solana such as Eliza, as well as tools like Proof of Sentience or workflow automations."

**Status (as of Feb 8, 2026)**: Currently ongoing, results pending.

## 6. Open Source & Artifacts

### Repository Structure

**Main Repository**: https://github.com/elizaOS/eliza

**Organization**: https://github.com/elizaOS

**Plugin Organization**: https://github.com/elizaos-plugins

### License

**MIT License** - Fully open source

### Star Count

**17.5k stars** (as of research date)

### Repository Structure

**Monorepo Architecture**: "Eliza is organized as a monorepo using Bun, Lerna, and Turbo for efficient package management and build orchestration."

**Core Packages**:
```
/packages
├── server/          # Express.js backend
├── client/          # React frontend UI
├── cli/             # Project management tool
├── core/            # Shared utilities
├── app/             # Desktop application (Tauri)
├── plugin-bootstrap/ # Core event/messaging
└── plugin-sql/      # Database integration
```

### Key Files

**Character Specification**:
- Repository: https://github.com/elizaOS/characterfile
- Schema: `schema/character.schema.json`
- Type Definitions: `examples/types.d.ts`
- Example: `examples/example.character.json`

**Core Runtime**:
- Location: `packages/core/src/`
- Entry Point: `agent/src/index.ts`

**Plugin Structure**:
```
src/
├── index.ts           # Main entry point
├── actions/           # Plugin-specific actions
├── clients/           # Client implementations
├── adapters/          # Adapter implementations
├── types.ts           # Type definitions
└── environment.ts     # Runtime settings and zod validation
```

### Plugin System

**Official Plugins Organization**: https://github.com/elizaos-plugins

**Plugin Registry**: https://github.com/elizaos-plugins/registry

**Notable Official Plugins**:
- `@elizaos/plugin-discord` - Discord integration
- `@elizaos/plugin-solana` - Solana blockchain operations
- `@elizaos/plugin-solana-v2` - Modern Solana integration with web3.js v2
- `@elizaos/plugin-twitter` - Twitter/X integration
- `@elizaos/plugin-bootstrap` - Core communication and events
- `@elizaos/plugin-sql` - Database integration
- `@elizaos/plugin-pdf` - File operations
- `@elizaos/plugin-sui` - Sui blockchain
- `@elizaos/plugin-llama` - Local LLM capabilities

**Plugin Starter**: https://github.com/elizaOS/eliza-plugin-starter

### Additional Repositories

**Character Tools**:
- https://github.com/elizaOS/characterfile - Character file format specification
- Character generators and conversion tools

**Agent Memory**:
- https://github.com/elizaOS/agentmemory - "Easy-to-use agent memory, powered by chromadb and postgres"

**Trust System**:
- https://github.com/elizaOS/trust_scoreboard - Trust scoring system

**Multi-Agent Systems**:
- https://github.com/elizaOS/the-org - "Agents for organizations"

**Development Tools**:
- https://github.com/elizaOS/swe-agent-ts - Software engineering agent
- https://github.com/elizaOS/eliza-plugin-starter - Plugin development template

### Application Architecture

**Cross-Platform**: "The Eliza application, built with Tauri and located in packages/app, is configured for cross-platform continuous integration and deployment for Desktop (Linux, macOS, and Windows) and Mobile (Android and iOS)."

### Documentation

**Main Documentation**: https://docs.elizaos.ai

**Alternative Domains**:
- http://eliza.how (redirects to docs.elizaos.ai)
- https://elizaos.github.io/eliza (older documentation)

**Documentation Index**: https://docs.elizaos.ai/llms.txt - Complete page index

**Key Documentation Sections**:
- Quickstart and installation guides
- Plugin registry and development
- REST and CLI references
- Character file configuration
- Cloud deployment options
- Architecture overview

---

## Sources

- [GitHub - elizaOS/eliza](https://github.com/elizaOS/eliza)
- [ElizaOS Official Website](https://elizaos.ai/)
- [ElizaOS Documentation](https://docs.elizaos.ai)
- [Eliza: A Web3 friendly AI Agent Operating System (arXiv)](https://arxiv.org/html/2501.06781v1)
- [The Rise of Agentic Capital: How ai16z and Autonomous Trading Swarms Are Remaking Solana](https://markets.financialcontent.com/stocks/article/tokenring-2026-2-6-the-rise-of-agentic-capital-how-ai16z-and-autonomous-trading-swarms-are-remaking-solana)
- [ai16z Unveils ElizaOS: The Path to Autonomous AI Agents](https://www.ainvest.com/news/ai16z-unveils-elizaos-path-to-autonomous-ai-agents-25021010b867fc71b073b28a/)
- [ElizaOS: The AI Agent Framework Transforming Web3 Automation](https://ventureburn.com/what-is-elizaos/)
- [Blockchain-powered AI agent platform 'ai16z' reaches $1.5 billion market cap](https://www.theblock.co/post/332546/blockchain-powered-ai-agent-ai16z-reaches-1-5-billion-market-cap)
- [Meet AI16z DAO: An AI-Based Investment Project](https://decrypt.co/295717/meet-ai16z-dao-an-ai-based-investment-project-that-aims-to-upend-silicon-valley)
- [Solana Agent Hackathon | Colosseum](https://colosseum.com/agent-hackathon/)
- [Eliza Agents Can Now Be Monetized via Ensemble's Agent Hub](https://www.techtimes.com/articles/311532/20250801/eliza-agents-can-now-monetized-via-ensembles-agent-hub-bringing-services-based-earnings-ai.htm)
- [GitHub - elizaOS/characterfile](https://github.com/elizaOS/characterfile)
- [GitHub - elizaOS/agentmemory](https://github.com/elizaOS/agentmemory)
- [ElizaOS Now Equipped with Glacier VectorDB Memory](https://medium.com/@glacierlabs/elizaos-now-equipped-with-glacier-vectordb-memory-f185b4e57f9f)
- [Guide to ElizaOS + Solana Agent Kit](https://medium.com/@0xnitt/guide-to-elizaos-solana-agent-kit-d58ba10b4924)
- [ai16z price today - CoinMarketCap](https://coinmarketcap.com/currencies/ai16z/)
- [Ai16z released beta version of ElizaOS as token's price continues to bleed](https://crypto.news/ai16z-released-beta-version-of-elizaos-as-tokens-price-continues-to-bleed/)
- [Hacker News: Eliza – Social Multi-Agent Framework](https://news.ycombinator.com/item?id=42237866)
- [Plugin System Overview - ElizaOS Documentation](https://docs.elizaos.ai/plugin-registry/overview)
- [ElizaOS Plugins GitHub](https://github.com/elizaos-plugins)
