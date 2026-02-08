# Kimi K2.5 Agent Swarm - Overview

## 1. Overview & Scale

### Model Architecture
- **Parameters**: 1.04 trillion total, 32 billion active per token
- **MoE Structure**: 384 experts, 8 selected per token (50% more experts than DeepSeek-V3's 256)
- **Activation Efficiency**: Only 3.2% of parameters activated per inference, reducing computation by 96.8%
- **Context Window**: 256K tokens (~200,000 words)
- **Quantization**: Native INT4 via Quantization-Aware Training (2x speed vs FP16)
- **Model Size**: 595GB (630GB full), requires 4x H200 GPUs or >240GB unified memory
- **Architecture Components**: 61 layers (including 1 dense), 7,168 attention dimension, 64 heads, 160K vocabulary
- **Vision Encoder**: MoonViT (400M parameters) with MLA attention and SwiGLU activation

### Training Scale
- **Pre-training**: 15.5 trillion tokens (K2 base), zero loss spikes
- **Continual Pre-training**: ~15 trillion mixed visual and text tokens (K2.5)
- **Training Innovation**: MuonClip optimizer with QK-clip technique for stability
- **Release Date**: January 27, 2026

### Agent Swarm Scale
> "Kimi K2.5 can self-direct an agent swarm with up to 100 sub-agents, executing parallel workflows across up to 1,500 tool calls."

- **Maximum Sub-Agents**: 100 parallel agents
- **Maximum Tool Calls**: 1,500 per task
- **Active Concurrency**: ~5 agents actively working, others queued
- **Speedup**: 4.5x reduction in execution time vs single-agent
- **Runtime Reduction**: 80% reduction in end-to-end latency (internal evals)
- **Latency on WideSearch**: 3x-4.5x faster as complexity increases

### Key Performance Metrics
> "Kimi K2.5 achieved a 75.0% success rate in autonomous web navigation tasks, significantly outperforming GPT-5.2 and Gemini 3 Pro."

**Tool-Augmented Performance Lift:**
- K2.5: +20.1 percentage points with tools
- GPT-5.2: +11.0 pp
- Claude: +12.4 pp
- Gemini: +8.3 pp

**Benchmark Highlights:**
- AIME 2025: 96.1% (vs 49.5% without extended thinking)
- GPQA-Diamond: 87.6% (vs 75.1% base K2)
- SWE-Bench Verified: 76.8% (vs 65.8% base K2)
- VideoMME: 87.4%
- MMMu-Pro: 78.5%
- OSWorld-Verified: 63.3%
- WebArena: 58.9%

## 2. Architecture

### Decentralized Parallel Intelligence

> "K2.5 transitions from single-agent scaling to a self-directed, coordinated swarm-like execution scheme. It decomposes complex tasks into parallel sub-tasks executed by dynamically instantiated, domain-specific agents."

**Core Components:**
1. **Orchestrator** (trainable): Main agent that decomposes tasks and coordinates swarm
2. **Sub-Agents** (frozen): Dynamically instantiated workers from intermediate policy checkpoints
3. **Tool Layer**: Search, code-interpreter, web-browsing (IPython, GUI, browser automation)

### Orchestrator-SubAgent Model

> "During training, sub-agents are frozen and their execution trajectories are excluded from the optimization objective; only the orchestrator is updated via reinforcement learning."

**Design Rationale:**
- Circumvents credit assignment ambiguity
- Prevents training instability from end-to-end co-optimization
- Enables heterogeneous agent instantiation based on task requirements

### Agent Organization & Roles

**Dynamic Specialization:**
Example from deployment planning task:
- InferenceStackResearcher
- QuantizationHardwareResearcher
- CostControlResearcher
- (Each then fans out to multiple worker personas)

> "Instead of predefining agents and pipelines, K2.5 can self-direct a swarm, deciding when to parallelize, how many agents to spawn, what tools to use, and how to merge results, based on the task itself."

**Agent Types** (visualized as word cloud in technical report):
- AI Researcher
- Physics Researcher
- Fact Checker
- Domain-specific researchers per task

## 3. Communication

### Task Decomposition & Assignment

> "When presented with a complex prompt, the model breaks the goal into smaller, parallelizable sub-tasks."

**Decomposition Strategy:**
- **Wide Search**: Simultaneous exploration across independent information sources
- **Deep Search**: Multiple reasoning branches with delayed aggregation
- Prompts structurally favor parallelization without explicit mandates
- Orchestrator learns whether/when/how to parallelize through environmental feedback

### Coordination Mechanism

**Scheduling:**
> "About five agents worked actively at a time while others queued and resumed as earlier subtasks completed, which suggests an internal scheduling mechanism."

**Result Aggregation:**
> "The primary agent gathers results from the swarm, resolves contradictions, and delivers a final, verified answer."

### Shared State & Context Management

**Proactive Context Management:**
> "Agent Swarm operates as proactive context management through explicit orchestration, contrasting with reactive approaches (Hide-Tool-Result, Summary, Discard-all)."

**Context Sharding:**
> "Long-horizon tasks decompose into semantically isolated subtasks with bounded local contexts. Only task-relevant outputs—rather than full interaction traces—are selectively routed back."

**Context Window Strategy:**
- 256K token limit per agent
- When tool results exceed limit: "simple context management strategy that hides all previous tool outputs is employed"
- No context management applied except when necessary (BrowseComp used discard-all)

### Tool Call Orchestration

**Tool Integration:**
- Each sub-agent can independently use tools (search, code execution, browser)
- Up to 1,500 coordinated tool calls across swarm
- Main agent coordinates tool results and decides next steps

**Benchmark-Specific Configuration:**
- BrowseComp: Main agent 15 steps, sub-agents 100 steps each
- Tools: search, code-interpreter, web-browsing for HLE and agentic benchmarks

## 4. Git & Code Integration

### Visual Coding Mode

> "Kimi K2.5 turns text, images, and video inputs into functional front-end code."

**"Vibe Coding" Capabilities:**
- Screenshot → React component with scroll animations
- Video workflow → executable code
- UI design → complete interactive layouts
- Pixel-level comparison between design and rendered output

**Closed-Loop Visual Debugging:**
> "After generating code, K2.5 will render the result itself and perform pixel-level comparison between the rendered result and the original design, automatically modifying the code if it finds discrepancies."

### Code Generation Performance

**Benchmarks:**
- LiveCodeBench: 53.7% (base K2)
- Terminal Bench: High scores (specific values in technical report)
- SWE-Bench Verified: 76.8%
- SWE-Bench Multilingual: 47.3%

**IDE Integration:**
- Kimi Code CLI (primary agent framework)
- VSCode, Cursor, Zed integrations
- Terminal-based workflows

### Git Operations
NOT DISCLOSED - No specific information about git-aware capabilities or repository operations

## 5. What Worked & What Failed

### What Worked

**Agent Swarm Benchmarks vs Competition:**

**BrowseComp** (web navigation):
- K2.5 Agent Swarm: 78.4%
- GPT-5.2 Pro: 77.9%
- K2.5 Single-Agent: 60.6%
- **Improvement**: +17.8% absolute

**WideSearch** (parallel research):
- K2.5 Agent Swarm: 79.0%
- Claude Opus 4.5: 76.2%
- K2.5 Single-Agent: 72.7%
- **Improvement**: +6.3%

**In-house Swarm Bench:**
- K2.5 Agent Swarm: 58.3%
- K2.5 Single-Agent: 41.6%
- **Improvement**: +16.7%

**Key Success: Tool-Augmented Tasks**
> "K2.5's improvement when given access to web search and code execution tools is +20.1 percentage points, compared to +11.0 for GPT-5.2."

**Parallelism Effectiveness:**
- Effective parallelization learned through PARL training
- Critical path optimization prevents "fake parallelism"
- Dynamic agent instantiation based on task structure

### What Failed / Limitations

**API & Integration Issues:**
> "Kimi K2.5 via API frequently errors out with either 'Bad request' or 'Too many requests' (Error code 429)."

- Tool calls broken in some third-party integrations
- Thinking mode errors: "reasoning_content is missing in assistant tool call message"
- File operations only reliable through first-party tools

**Agent Swarm Limitations:**
> "The viral 'agent swarm' demo is basically a tech demo unless you're using Kimi's native tools, with third-party access severely limited."

**Token Consumption:**
> "Kimi K2 Thinking consumed roughly 2.5x more tokens than DeepSeek-V3.2, with the issue being the model's verbosity."

- 15-35% slower responses with thinking enabled
- 1.2-1.6x token usage on comparable tasks
- Can execute up to 1,500 tool calls per task (cost implications)

**Long-Context Issues:**
> "K2 can summarize long transcripts well, but multi-turn projects where summaries are stacked over several chats start to blur specifics, with entities merging and dates drifting."

**Overthinking Simple Tasks:**
> "K2 may overthink simple tasks, such as producing a mini-outline for a two-sentence meta description that wasn't needed."

**Serial Collapse Challenge:**
Without PARL reward shaping, orchestrator defaults to single-agent execution despite parallel capacity (addressed in training but potential failure mode)

**Spurious Parallelism:**
Without finish reward, orchestrator spawns many agents without meaningful task decomposition (reward-hacking behavior, addressed in training)

**Context Length Failures:**
> "Tasks exceeding the supported context length were directly counted as failed."

**Visual RL Mechanism Unclear:**
> "Visual RL's mechanism for improving text performance remains somewhat speculative: 'likely because joint pretraining already establishes strong vision-text alignment.'"

## 6. Open Source & Artifacts

### Open Source Model

**License**: Modified MIT License (code + weights)
- Commercial use permitted without fees below thresholds
- **Attribution requirement**: Only above 100M MAU or $20M monthly revenue
- Private deployment allowed
- Modification and distribution permitted
- Domain-specific fine-tuning allowed

**Release Status:**
> "Moonshot AI officially released and completely open-sourced Kimi K2.5 on January 26, 2026, including both the code and the model weights."

### Model Weights

**Hugging Face Repositories:**
- `moonshotai/Kimi-K2.5` (main model, 595GB)
- `moonshotai/Kimi-K2-Thinking` (thinking mode variant)
- `moonshotai/Kimi-K2-Instruct` (instruction-tuned base)

**Community Quantizations:**
- `unsloth/Kimi-K2.5-GGUF` (GGUF format)
- `unsloth/Kimi-K2.5` (Unsloth optimized)
- `AesSedai/Kimi-K2.5-GGUF` (alternative GGUF)
- `mlx-community/Kimi-K2.5` (MLX format for Apple Silicon)

### GitHub Repositories

**Official Repos:**
- `MoonshotAI/Kimi-K2.5` - Main model repository with tech report
- `MoonshotAI/Kimi-K2` - Base K2 model series

**Technical Report:**
- `tech_report.pdf` at `MoonshotAI/Kimi-K2.5/blob/master/tech_report.pdf`
- arXiv: 2602.02276 "Kimi K2.5: Visual Agentic Intelligence"

**Community Repos:**
- `dnnyngyen/kimi-k2.5-prompts-tools` - Extracted system prompts and tool schemas
- `The-Swarm-Corporation/PARL` - PARL implementation (community recreation)

### API Access

**Official Platform:**
- https://platform.moonshot.ai
- OpenAI/Anthropic-compatible endpoints
- Supports text, image, video inputs

**Third-Party Providers:**
- NVIDIA NIM: build.nvidia.com/moonshotai/kimi-k2.5
- OpenRouter: openrouter.ai/moonshotai/kimi-k2.5
- Together AI: together.ai/models/kimi-k2-5
- Fireworks AI: Available with full-parameter RFT

**Ollama:**
- `ollama library/kimi-k2.5`

### Deployment Engines

**Supported Inference Engines:**
- vLLM (recommended)
- SGLang (recommended)
- KTransformers (recommended)
- Transformers 4.57.1+ (minimum)

**Deployment Guides:**
- `Kimi-K2.5/docs/deploy_guidance.md`
- Tensor parallelism configurations for multi-GPU setups
- KTransformers guide: `kvcache-ai/ktransformers/doc/en/Kimi-K2.5.md`

### SDK & Code Examples

**Primary Framework:**
- Kimi Code CLI (official agent framework)

**API Modes:**
- K2.5 Instant (temperature 0.6, top_p 0.95)
- K2.5 Thinking (temperature 1.0, top_p 0.95)
- K2.5 Agent (single-agent mode)
- K2.5 Agent Swarm (beta, up to 100 agents)

**Thinking Mode Control:**
```python
# Official API
extra_body={'thinking': {'type': 'disabled'}}

# vLLM/SGLang
extra_body={'chat_template_kwargs': {"thinking": False}}
```

**System Prompts & Tools:**
- 6 documented agent types
- 37 distinct tool schemas
- Runtime environment source code samples
- Available in `dnnyngyen/kimi-k2.5-prompts-tools`

### Research Papers

**arXiv Papers:**
- [2507.20534] "Kimi K2: Open Agentic Intelligence" (base K2 model)
- [2602.02276] "Kimi K2.5: Visual Agentic Intelligence" (K2.5 with Agent Swarm)
- PDF: arxiv.org/pdf/2602.02276

**Official Websites:**
- moonshotai.github.io/Kimi-K2/ (K2 technical site)
- kimi.com/blog/kimi-k2-5.html (official blog)
- kimi.com/ai-models/kimi-k2-5 (model page)

### Infrastructure Support

**Hardware Requirements:**
- Minimum: 240GB unified memory (RAM+VRAM) for 10+ tokens/s
- Production: 4x H200 GPUs
- Model size: 630GB full, 595GB repository

**Optimization:**
- Native INT4 quantization via QAT
- Decoupled Encoder Process (DEP) for multimodal training
- 90% multimodal training efficiency vs text-only

---

## Sources

- [Constellation Research: Moonshot's Kimi K2.5 introduces agent swarm](https://www.constellationr.com/insights/news/moonshots-kimi-k25-introduces-agent-swarm-highlights-open-source-model-momentum)
- [Kimi K2.5 Official Model Page](https://www.kimi.com/ai-models/kimi-k2-5)
- [FinancialContent: The Swarm Emerges - Kimi K2.5 Challenges Western AI](https://markets.financialcontent.com/stocks/article/tokenring-2026-2-5-the-swarm-emerges-moonshot-ais-kimi-k25-challenges-western-ai-hegemony)
- [Hugging Face: moonshotai/Kimi-K2.5](https://huggingface.co/moonshotai/Kimi-K2.5)
- [Codecademy: Kimi K2.5 Complete Guide](https://www.codecademy.com/article/kimi-k-2-5-complete-guide-to-moonshots-ai-model)
- [NVIDIA NIM: kimi-k2.5 Model Card](https://build.nvidia.com/moonshotai/kimi-k2.5/modelcard)
- [DataCamp: Kimi K2.5 and Agent Swarm Guide](https://www.datacamp.com/tutorial/kimi-k2-agent-swarm-guide)
- [WaveSpeedAI: Everything About Kimi K2.5](https://wavespeed.ai/blog/posts/kimi-k2-5-everything-we-know-about-moonshots-visual-agentic-model/)
- [DEV Community: Kimi K2.5 Ultimate Guide](https://dev.to/czmilo/kimi-k25-in-2026-the-ultimate-guide-to-open-source-visual-agentic-intelligence-18od)
- [VentureBeat: Kimi K2.5 is open, 595GB, built for agent swarms](https://venturebeat.com/orchestration/moonshots-kimi-k2-5-is-open-595gb-and-built-for-agent-swarms-reddit-wants-a)
- [Unsloth: Kimi K2.5 Local Running Guide](https://unsloth.ai/docs/models/kimi-k2.5)
- [Simon Willison: Kimi K2.5 Visual Agentic Intelligence](https://simonwillison.net/2026/Jan/27/kimi-k25/)
- [arXiv:2507.20534 - Kimi K2: Open Agentic Intelligence](https://arxiv.org/abs/2507.20534)
- [arXiv:2602.02276 - Kimi K2.5: Visual Agentic Intelligence](https://arxiv.org/html/2602.02276v1)
- [GitHub: MoonshotAI/Kimi-K2.5](https://github.com/MoonshotAI/Kimi-K2.5)
- [Kimi K2.5 Tech Blog](https://www.kimi.com/blog/kimi-k2-5.html)
- [Medium: Four Giants One Winner - K2.5 vs GPT-5.2 vs Opus vs Gemini](https://medium.com/@cognidownunder/four-giants-one-winner-kimi-k2-5-vs-gpt-5-2-vs-claude-opus-4-5-vs-gemini-3-pro-comparison-38124c85d990)
- [Apiyi: Kimi K2.5 Paper Parameters Requirements Guide](https://help.apiyi.com/en/kimi-k2-5-paper-parameters-requirements-guide-en.html)
- [VERTU: Kimi K2.5 1.04T MoE Model & Agent Swarm](https://vertu.com/lifestyle/kimi-k2-5-the-trillion-parameter-open-source-ai-revolutionizing-multimodal-agents/)
- [GitHub: The-Swarm-Corporation/PARL](https://github.com/The-Swarm-Corporation/PARL)
- [GitHub: dnnyngyen/kimi-k2.5-prompts-tools](https://github.com/dnnyngyen/kimi-k2.5-prompts-tools)
- [Kimi Code GitHub Issues](https://github.com/Kilo-Org/kilocode/issues/5719)
- [Blog: What We Learned from a Week of Free Kimi K2.5](https://blog.kilo.ai/p/what-we-learned-from-a-week-of-free)
