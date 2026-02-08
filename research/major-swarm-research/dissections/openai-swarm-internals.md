# OpenAI Swarm Internals: Complete Dissection

**Source**: OpenAI's Educational Swarm Framework (20K+ stars)
**Repository**: https://github.com/openai/swarm
**Note**: Now superseded by OpenAI Agents SDK, but remains foundational for understanding multi-agent patterns

---

## 1. Core Swarm Loop

### Location
`swarm/core.py` - `Swarm.run()` method (lines 231-292)

### The Main Execution Loop

```python
def run(
    self,
    agent: Agent,
    messages: List,
    context_variables: dict = {},
    model_override: str = None,
    stream: bool = False,
    debug: bool = False,
    max_turns: int = float("inf"),
    execute_tools: bool = True,
) -> Response:
    active_agent = agent
    context_variables = copy.deepcopy(context_variables)
    history = copy.deepcopy(messages)
    init_len = len(messages)

    while len(history) - init_len < max_turns and active_agent:
        # 1. Get completion with current history, agent
        completion = self.get_chat_completion(
            agent=active_agent,
            history=history,
            context_variables=context_variables,
            model_override=model_override,
            stream=stream,
            debug=debug,
        )
        message = completion.choices[0].message
        message.sender = active_agent.name
        history.append(json.loads(message.model_dump_json()))

        # 2. Check if we're done (no tool calls)
        if not message.tool_calls or not execute_tools:
            break

        # 3. Handle function calls, updating context_variables, and switching agents
        partial_response = self.handle_tool_calls(
            message.tool_calls,
            active_agent.functions,
            context_variables,
            debug
        )
        history.extend(partial_response.messages)
        context_variables.update(partial_response.context_variables)

        # 4. AGENT SWITCH happens here
        if partial_response.agent:
            active_agent = partial_response.agent

    return Response(
        messages=history[init_len:],
        agent=active_agent,
        context_variables=context_variables,
    )
```

### Key Pattern

**The loop is stateless between `run()` calls**:
1. Get completion from current agent
2. Execute tool calls and append results
3. **Switch agent if a function returned an agent**
4. Update context variables if necessary
5. If no new function calls, return

**Critical**: Each agent's instructions become the system prompt. When switching agents, the system prompt changes but chat history persists.

### Rust Mapping

```rust
pub struct SwarmRuntime {
    client: Arc<dyn LLMClient>,
}

impl SwarmRuntime {
    pub async fn run(
        &self,
        mut active_agent: Arc<Agent>,
        mut messages: Vec<Message>,
        mut context_variables: HashMap<String, Value>,
        max_turns: usize,
    ) -> Result<Response> {
        let init_len = messages.len();
        let mut turn_count = 0;

        while turn_count < max_turns {
            // 1. Get completion
            let completion = self.get_chat_completion(
                &active_agent,
                &messages,
                &context_variables,
            ).await?;

            // 2. Add to history
            let mut message = completion.message;
            message.sender = Some(active_agent.name.clone());
            messages.push(message.clone());

            // 3. Check for tool calls
            if message.tool_calls.is_none() {
                break;
            }

            // 4. Execute tools and handle agent switches
            let partial = self.handle_tool_calls(
                &message.tool_calls.unwrap(),
                &active_agent.functions,
                &mut context_variables,
            ).await?;

            messages.extend(partial.messages);
            context_variables.extend(partial.context_variables);

            // 5. AGENT HANDOFF
            if let Some(new_agent) = partial.agent {
                active_agent = new_agent;
            }

            turn_count += 1;
        }

        Ok(Response {
            messages: messages[init_len..].to_vec(),
            agent: active_agent,
            context_variables,
        })
    }
}
```

---

## 2. Agent Definition

### Location
`swarm/types.py` - `Agent` class (lines 14-21)

### Agent Structure

```python
from pydantic import BaseModel
from typing import List, Callable, Union

AgentFunction = Callable[[], Union[str, "Agent", dict]]

class Agent(BaseModel):
    name: str = "Agent"
    model: str = "gpt-4o"
    instructions: Union[str, Callable[[], str]] = "You are a helpful agent."
    functions: List[AgentFunction] = []
    tool_choice: str = None
    parallel_tool_calls: bool = True
```

### Key Features

1. **Instructions can be dynamic**: Either a string OR a function that returns a string
2. **Functions can return agents**: This is how handoffs work
3. **Minimal overhead**: No complex state management
4. **Pydantic validation**: Type safety and serialization built-in

### Dynamic Instructions Example

```python
def instructions(context_variables):
    name = context_variables.get("name", "User")
    return f"You are a helpful agent. Greet the user by name ({name})."

agent = Agent(
    name="Agent",
    instructions=instructions,  # Function, not string
    functions=[print_account_details],
)
```

### Rust Mapping

```rust
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type AgentFunction = Arc<dyn Fn(FunctionArgs, &Context) -> BoxFuture<'static, Result<FunctionResult>> + Send + Sync>;

pub enum Instructions {
    Static(String),
    Dynamic(Arc<dyn Fn(&HashMap<String, Value>) -> String + Send + Sync>),
}

#[derive(Clone)]
pub struct Agent {
    pub name: String,
    pub model: String,
    pub instructions: Instructions,
    pub functions: Vec<(String, AgentFunction)>,  // (name, function)
    pub tool_choice: Option<String>,
    pub parallel_tool_calls: bool,
}

impl Agent {
    pub fn get_instructions(&self, context: &HashMap<String, Value>) -> String {
        match &self.instructions {
            Instructions::Static(s) => s.clone(),
            Instructions::Dynamic(f) => f(context),
        }
    }
}
```

---

## 3. Handoff Mechanism (The Key Innovation)

### Location
`swarm/core.py` - `handle_function_result()` (lines 71-87) and `handle_tool_calls()` (lines 89-137)

### How Handoffs Work

**Step 1**: Function returns an Agent object

```python
spanish_agent = Agent(
    name="Spanish Agent",
    instructions="You only speak Spanish.",
)

def transfer_to_spanish_agent():
    """Transfer spanish speaking users immediately."""
    return spanish_agent  # Returns an Agent!

english_agent = Agent(
    name="English Agent",
    instructions="You only speak English.",
    functions=[transfer_to_spanish_agent],
)
```

**Step 2**: `handle_function_result()` detects the Agent return

```python
def handle_function_result(self, result, debug) -> Result:
    match result:
        case Result() as result:
            return result
        case Agent() as agent:
            # Handoff detected!
            return Result(
                value=json.dumps({"assistant": agent.name}),
                agent=agent,  # This signals the switch
            )
        case _:
            return Result(value=str(result))
```

**Step 3**: Main loop switches `active_agent`

```python
# In handle_tool_calls()
for tool_call in tool_calls:
    raw_result = function_map[name](**args)
    result: Result = self.handle_function_result(raw_result, debug)

    # Accumulate agent switches
    if result.agent:
        partial_response.agent = result.agent  # Last one wins

# In run() loop
if partial_response.agent:
    active_agent = partial_response.agent  # AGENT SWITCH HAPPENS HERE
```

### The Result Object

```python
class Result(BaseModel):
    """Encapsulates the possible return values for an agent function."""
    value: str = ""
    agent: Optional[Agent] = None
    context_variables: dict = {}
```

Functions can return:
- **String**: Normal tool result
- **Agent**: Agent handoff
- **Result**: Complex result with value + agent switch + context update

### Airline Example: Multi-Level Routing

From `examples/airline/configs/agents.py`:

```python
def transfer_to_flight_modification():
    return flight_modification  # First handoff

def transfer_to_flight_cancel():
    return flight_cancel  # Second level handoff

triage_agent = Agent(
    name="Triage Agent",
    functions=[transfer_to_flight_modification, transfer_to_lost_baggage],
)

flight_modification = Agent(
    name="Flight Modification Agent",
    functions=[transfer_to_flight_cancel, transfer_to_flight_change],
)

flight_cancel = Agent(
    name="Flight cancel traversal",
    instructions=STARTER_PROMPT + FLIGHT_CANCELLATION_POLICY,
    functions=[escalate_to_agent, initiate_refund, transfer_to_triage],
)
```

**Flow**: Triage → Flight Modification → Flight Cancel → Triage (can return!)

### Rust Mapping

```rust
pub enum FunctionResult {
    Value(String),
    AgentHandoff(Arc<Agent>),
    Full(Result),
}

pub struct Result {
    pub value: String,
    pub agent: Option<Arc<Agent>>,
    pub context_variables: HashMap<String, Value>,
}

impl SwarmRuntime {
    async fn handle_function_result(&self, result: Box<dyn Any>) -> Result<Result> {
        // Type checking to detect agent handoffs
        if let Some(agent) = result.downcast_ref::<Arc<Agent>>() {
            return Ok(Result {
                value: serde_json::to_string(&json!({
                    "assistant": agent.name
                }))?,
                agent: Some(agent.clone()),
                context_variables: HashMap::new(),
            });
        }

        if let Some(result) = result.downcast_ref::<Result>() {
            return Ok(result.clone());
        }

        // Default: treat as string
        Ok(Result {
            value: format!("{:?}", result),
            agent: None,
            context_variables: HashMap::new(),
        })
    }
}
```

**Challenge in Rust**: Python's duck typing vs Rust's static typing. Solutions:
1. Use `enum FunctionResult` instead of `Any`
2. Use trait objects: `dyn AgentFunction`
3. Use type erasure with `Box<dyn Any>`

---

## 4. Context Variables (Shared State)

### Location
`swarm/core.py` - lines 23, 42-46, 119-121

### How Context Variables Work

**Definition**: A simple `dict` passed through all function calls and agent instructions

```python
__CTX_VARS_NAME__ = "context_variables"

# In get_chat_completion()
context_variables = defaultdict(str, context_variables)
instructions = (
    agent.instructions(context_variables)  # Passed to callable instructions
    if callable(agent.instructions)
    else agent.instructions
)
```

### Auto-Injection into Functions

**Key Pattern**: Functions that have a `context_variables` parameter automatically receive it

```python
# In handle_tool_calls()
func = function_map[name]
# Check function signature
if __CTX_VARS_NAME__ in func.__code__.co_varnames:
    args[__CTX_VARS_NAME__] = context_variables  # Auto-inject!
raw_result = func(**args)
```

### Hidden from Model

```python
# In get_chat_completion()
tools = [function_to_json(f) for f in agent.functions]

# Hide context_variables from model
for tool in tools:
    params = tool["function"]["parameters"]
    params["properties"].pop(__CTX_VARS_NAME__, None)
    if __CTX_VARS_NAME__ in params["required"]:
        params["required"].remove(__CTX_VARS_NAME__)
```

**Why?** Context variables are for internal state, not exposed to the LLM.

### Example: User Context

From `examples/airline/main.py`:

```python
context_variables = {
    "customer_context": """Here is what you know about the customer's details:
1. CUSTOMER_ID: customer_12345
2. NAME: John Doe
3. PHONE_NUMBER: (123) 456-7890
4. EMAIL: johndoe@example.com
5. STATUS: Premium
6. ACCOUNT_STATUS: Active
7. BALANCE: $0.00
8. LOCATION: 1234 Main St, San Francisco, CA 94123, USA
""",
    "flight_context": """The customer has an upcoming flight from LGA to LAX.
The flight # is 1919. The flight departure date is 3pm ET, 5/21/2024.""",
}

def triage_instructions(context_variables):
    customer_context = context_variables.get("customer_context", None)
    flight_context = context_variables.get("flight_context", None)
    return f"""You are to triage a users request...
    The customer context is here: {customer_context},
    and flight context is here: {flight_context}"""
```

### Context Updates

```python
def print_account_details(context_variables: dict):
    user_id = context_variables.get("user_id", None)
    name = context_variables.get("name", None)
    print(f"Account Details: {name} {user_id}")
    return "Success"

# Or update context:
def talk_to_sales():
    return Result(
        value="Done",
        agent=sales_agent,
        context_variables={"department": "sales"}  # Merge update
    )
```

### Rust Mapping

```rust
use std::collections::HashMap;
use serde_json::Value;

pub type Context = HashMap<String, Value>;

impl SwarmRuntime {
    async fn inject_context_if_needed(
        &self,
        func: &AgentFunction,
        mut args: FunctionArgs,
        context: &Context,
    ) -> FunctionArgs {
        // Check if function signature expects context
        // (In Rust, this would be explicit via function signature)
        if func.expects_context() {
            args.context = Some(context.clone());
        }
        args
    }

    fn hide_context_from_schema(&self, mut schema: ToolSchema) -> ToolSchema {
        // Remove context_variables from OpenAI function schema
        schema.parameters.properties.remove("context_variables");
        schema.parameters.required.retain(|p| p != "context_variables");
        schema
    }
}

// Function signature pattern
pub struct FunctionArgs {
    pub params: HashMap<String, Value>,
    pub context: Option<Context>,  // Auto-injected if function needs it
}

// Example function
async fn print_account_details(args: FunctionArgs) -> Result<FunctionResult> {
    let ctx = args.context.as_ref().ok_or("Missing context")?;
    let user_id = ctx.get("user_id");
    let name = ctx.get("name");
    println!("Account Details: {:?} {:?}", name, user_id);
    Ok(FunctionResult::Value("Success".to_string()))
}
```

---

## 5. Function Calling (Tools)

### Location
`swarm/util.py` - `function_to_json()` (lines 31-87)

### Auto-Schema Generation

**Swarm converts Python functions to OpenAI function schemas automatically**:

```python
def function_to_json(func) -> dict:
    """
    Converts a Python function into a JSON-serializable dictionary
    that describes the function's signature, including its name,
    description, and parameters.
    """
    type_map = {
        str: "string",
        int: "integer",
        float: "number",
        bool: "boolean",
        list: "array",
        dict: "object",
        type(None): "null",
    }

    signature = inspect.signature(func)
    parameters = {}

    for param in signature.parameters.values():
        param_type = type_map.get(param.annotation, "string")
        parameters[param.name] = {"type": param_type}

    required = [
        param.name
        for param in signature.parameters.values()
        if param.default == inspect._empty
    ]

    return {
        "type": "function",
        "function": {
            "name": func.__name__,
            "description": func.__doc__ or "",
            "parameters": {
                "type": "object",
                "properties": parameters,
                "required": required,
            },
        },
    }
```

### Example Transformation

**Python function**:

```python
def greet(name, age: int, location: str = "New York"):
    """Greets the user. Make sure to get their name and age before calling.

    Args:
        name: Name of the user.
        age: Age of the user.
        location: Best place on earth.
    """
    print(f"Hello {name}, glad you are {age} in {location}!")
```

**Generated schema**:

```json
{
    "type": "function",
    "function": {
        "name": "greet",
        "description": "Greets the user. Make sure to get their name and age before calling.\n\nArgs:\n   name: Name of the user.\n   age: Age of the user.\n   location: Best place on earth.",
        "parameters": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"},
                "location": {"type": "string"}
            },
            "required": ["name", "age"]
        }
    }
}
```

### Function Execution

From `swarm/core.py` - `handle_tool_calls()`:

```python
def handle_tool_calls(
    self,
    tool_calls: List[ChatCompletionMessageToolCall],
    functions: List[AgentFunction],
    context_variables: dict,
    debug: bool,
) -> Response:
    function_map = {f.__name__: f for f in functions}
    partial_response = Response(messages=[], agent=None, context_variables={})

    for tool_call in tool_calls:
        name = tool_call.function.name

        # Handle missing tool
        if name not in function_map:
            partial_response.messages.append({
                "role": "tool",
                "tool_call_id": tool_call.id,
                "content": f"Error: Tool {name} not found.",
            })
            continue

        # Parse arguments
        args = json.loads(tool_call.function.arguments)

        # Auto-inject context if function expects it
        func = function_map[name]
        if __CTX_VARS_NAME__ in func.__code__.co_varnames:
            args[__CTX_VARS_NAME__] = context_variables

        # Execute
        raw_result = func(**args)
        result: Result = self.handle_function_result(raw_result, debug)

        # Add result to message history
        partial_response.messages.append({
            "role": "tool",
            "tool_call_id": tool_call.id,
            "tool_name": name,
            "content": result.value,
        })

        # Merge context updates
        partial_response.context_variables.update(result.context_variables)

        # Track agent switches (last one wins)
        if result.agent:
            partial_response.agent = result.agent

    return partial_response
```

### Rust Mapping

```rust
use std::any::TypeId;

// Function registry with schema generation
pub struct FunctionRegistry {
    functions: HashMap<String, RegisteredFunction>,
}

pub struct RegisteredFunction {
    pub name: String,
    pub schema: ToolSchema,
    pub executor: AgentFunction,
}

impl FunctionRegistry {
    pub fn register<F, Args, Ret>(&mut self, name: &str, func: F)
    where
        F: Fn(Args, Option<&Context>) -> BoxFuture<'static, Result<Ret>> + Send + Sync + 'static,
        Args: DeserializeOwned,
        Ret: Serialize,
    {
        let schema = ToolSchema {
            r#type: "function".to_string(),
            function: FunctionSchema {
                name: name.to_string(),
                description: String::new(),  // Would parse from doc comments
                parameters: self.generate_params_schema::<Args>(),
            },
        };

        self.functions.insert(name.to_string(), RegisteredFunction {
            name: name.to_string(),
            schema,
            executor: Arc::new(move |args, ctx| {
                let func = func.clone();
                Box::pin(async move {
                    let typed_args: Args = serde_json::from_value(args.params)?;
                    let result = func(typed_args, ctx).await?;
                    Ok(FunctionResult::Value(serde_json::to_string(&result)?))
                })
            }),
        });
    }

    fn generate_params_schema<T: DeserializeOwned>(&self) -> ParametersSchema {
        // Use serde introspection or proc macros
        // Similar to schemars crate
        todo!("Generate JSON schema from Rust type")
    }
}

// Usage
let mut registry = FunctionRegistry::new();
registry.register("greet", |args: GreetArgs, _ctx| async move {
    println!("Hello {}, glad you are {} in {}!", args.name, args.age, args.location);
    Ok("Greeted")
});

#[derive(Deserialize)]
struct GreetArgs {
    name: String,
    age: i32,
    #[serde(default = "default_location")]
    location: String,
}

fn default_location() -> String {
    "New York".to_string()
}
```

**Better approach**: Use proc macros like `#[swarm_function]` to auto-generate schemas.

---

## 6. Response Handling

### Location
`swarm/types.py` - `Response` class (lines 23-26)

### Response Structure

```python
class Response(BaseModel):
    messages: List = []
    agent: Optional[Agent] = None
    context_variables: dict = {}
```

**Minimal state returned**:
1. **messages**: New messages added during this run
2. **agent**: The final active agent (after any handoffs)
3. **context_variables**: Updated context

### Stateless Continuation Pattern

```python
# From examples/basic/simple_loop_no_helpers.py
messages = []
agent = my_agent

while True:
    user_input = input("> ")
    messages.append({"role": "user", "content": user_input})

    response = client.run(agent=agent, messages=messages)

    # Update state for next iteration
    messages = response.messages  # Full history
    agent = response.agent        # May have switched

    pretty_print_messages(messages)
```

**Key Pattern**: The application maintains state by feeding `response.messages` and `response.agent` back into the next `run()` call.

### Streaming Responses

From `swarm/core.py` - `run_and_stream()`:

```python
def run_and_stream(self, agent, messages, ...):
    active_agent = agent
    history = copy.deepcopy(messages)

    while len(history) - init_len < max_turns:
        # Stream each chunk
        completion = self.get_chat_completion(..., stream=True)

        yield {"delim": "start"}  # Agent turn start
        for chunk in completion:
            delta = json.loads(chunk.choices[0].delta.json())
            if delta["role"] == "assistant":
                delta["sender"] = active_agent.name
            yield delta
        yield {"delim": "end"}  # Agent turn end

        # ... handle tool calls ...

    # Final response
    yield {
        "response": Response(
            messages=history[init_len:],
            agent=active_agent,
            context_variables=context_variables,
        )
    }
```

**Two new event types**:
- `{"delim": "start"}` and `{"delim": "end"}`: Signal agent turn boundaries
- `{"response": Response}`: Final aggregated response at end of stream

### Rust Mapping

```rust
use futures::stream::Stream;

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub messages: Vec<Message>,
    pub agent: Arc<Agent>,
    pub context_variables: Context,
}

// Non-streaming
impl SwarmRuntime {
    pub async fn run(
        &self,
        agent: Arc<Agent>,
        messages: Vec<Message>,
        context: Context,
    ) -> Result<Response> {
        // ... execution loop ...

        Ok(Response {
            messages: history[init_len..].to_vec(),
            agent: active_agent,
            context_variables,
        })
    }
}

// Streaming
pub enum StreamEvent {
    DelimStart,
    Delta(Delta),
    DelimEnd,
    FinalResponse(Response),
}

impl SwarmRuntime {
    pub async fn run_stream(
        &self,
        agent: Arc<Agent>,
        messages: Vec<Message>,
        context: Context,
    ) -> impl Stream<Item = Result<StreamEvent>> {
        // Return async stream
        async_stream::try_stream! {
            let mut active_agent = agent;
            let mut history = messages;

            loop {
                yield StreamEvent::DelimStart;

                let mut completion_stream = self.get_completion_stream(&active_agent, &history).await?;
                while let Some(chunk) = completion_stream.next().await {
                    yield StreamEvent::Delta(chunk?);
                }

                yield StreamEvent::DelimEnd;

                // Handle tool calls...
                if no_tool_calls {
                    break;
                }
            }

            yield StreamEvent::FinalResponse(Response {
                messages: history,
                agent: active_agent,
                context_variables,
            });
        }
    }
}

// Usage
let mut stream = runtime.run_stream(agent, messages, context).await;
while let Some(event) = stream.next().await {
    match event? {
        StreamEvent::DelimStart => println!("[Agent turn start]"),
        StreamEvent::Delta(delta) => print!("{}", delta.content),
        StreamEvent::DelimEnd => println!("[Agent turn end]"),
        StreamEvent::FinalResponse(resp) => {
            println!("Final agent: {}", resp.agent.name);
        }
    }
}
```

---

## 7. Real-World Example Patterns

### Pattern 1: Triage Agent (Router)

**From `examples/airline/configs/agents.py`**

```python
def triage_instructions(context_variables):
    customer_context = context_variables.get("customer_context", None)
    flight_context = context_variables.get("flight_context", None)
    return f"""You are to triage a users request, and call a tool to transfer to the right intent.
    Once you are ready to transfer to the right intent, call the tool to transfer to the right intent.
    You dont need to know specifics, just the topic of the request.
    Customer: {customer_context}, Flight: {flight_context}"""

triage_agent = Agent(
    name="Triage Agent",
    instructions=triage_instructions,
    functions=[transfer_to_flight_modification, transfer_to_lost_baggage],
)
```

**Pattern**: Entry point that routes to specialists based on intent

### Pattern 2: Specialist Agents

**Flight Modification Agent** (sub-router):

```python
flight_modification = Agent(
    name="Flight Modification Agent",
    instructions="""You are a Flight Modification Agent...
First, look at message history and see if you can determine if the user wants to
cancel or change their flight. Ask clarifying questions until you know, then call
the appropriate transfer function.""",
    functions=[transfer_to_flight_cancel, transfer_to_flight_change],
    parallel_tool_calls=False,  # Sequential execution
)
```

**Flight Cancel Agent** (executor):

```python
flight_cancel = Agent(
    name="Flight cancel traversal",
    instructions=STARTER_PROMPT + FLIGHT_CANCELLATION_POLICY,
    functions=[
        escalate_to_agent,      # Tool
        initiate_refund,        # Tool
        initiate_flight_credits,# Tool
        transfer_to_triage,     # Handoff back
        case_resolved,          # Tool
    ],
)
```

**Pattern**: Specialized agents with domain tools + escape hatch to return to triage

### Pattern 3: Personal Shopper (Database + Triage)

**From `examples/personal_shopper/main.py`**

```python
refunds_agent = Agent(
    name="Refunds Agent",
    description="You handle refunds after returns are processed.",
    functions=[refund_item, notify_customer],
)

sales_agent = Agent(
    name="Sales Agent",
    description="You handle placing orders to purchase items.",
    functions=[order_item, notify_customer],
)

# Auto-generate triage agent (hypothetical helper)
triage_agent = create_triage_agent(
    name="Triage Agent",
    instructions="You triage requests and transfer to the right agent.",
    agents=[sales_agent, refunds_agent],
    add_backlinks=True,  # Auto-add transfer_to_triage to specialists
)
```

**Pattern**: Hub-and-spoke with automatic backlinks

### Pattern 4: Support Bot (RAG + Tools)

**From `examples/support_bot/customer_service.py`**

```python
def query_docs(query):
    """Search knowledge base with embeddings"""
    results = query_qdrant(query, collection_name="help_center")
    # Return most relevant article
    return {"response": f"Title: {title}\nContent: {content}"}

help_center_agent = Agent(
    name="Help Center Agent",
    instructions="You handle questions about OpenAI products.",
    functions=[query_docs, submit_ticket, send_email],
)

def transfer_to_help_center():
    return help_center_agent

user_interface_agent = Agent(
    name="User Interface Agent",
    instructions="You handle general interactions. Transfer when appropriate.",
    functions=[query_docs, submit_ticket, send_email, transfer_to_help_center],
)
```

**Pattern**: Shared tools across agents, selective handoff for specialization

---

## 8. Key Insights for Rust Implementation

### 1. **Minimal Abstraction**
- Swarm is ~300 lines of core code
- No complex state machines, just a loop
- Agents are data, not objects with methods

### 2. **Function Return Types Drive Behavior**
- Returning `Agent` = handoff
- Returning `Result` = complex update (value + agent + context)
- Returning `str` = simple tool result

**Rust Challenge**: Need `enum` or trait objects instead of duck typing.

### 3. **Context Variables Are Magic**
- Auto-injected based on function signature inspection
- Hidden from LLM schema generation
- Passed through all layers

**Rust Solution**: Use explicit `Option<&Context>` parameter or proc macros.

### 4. **Stateless Between Calls**
- No session management
- Application layer maintains state by feeding `Response` back in
- Easy to pause/resume/fork conversations

**Rust Advantage**: Natural fit for Rust's ownership model.

### 5. **Streaming as First-Class**
- Delimiters mark agent turn boundaries
- Final `Response` sent at end of stream
- Enables real-time multi-agent visualization

**Rust Solution**: `async_stream` + `Stream` trait.

### 6. **Agent Network Patterns**

Common topologies observed:

1. **Hub-and-Spoke** (Triage): One router, many specialists
2. **Chain** (Airline): Triage → Sub-router → Executor
3. **Bidirectional** (with `transfer_to_triage`): Specialists can return to router
4. **Shared Tools** (Support bot): Multiple agents with overlapping functions

### 7. **Error Handling is Graceful**
- Missing function → error message to agent
- Wrong arguments → error message to agent
- Agent can recover and retry

**Rust Approach**: Return `Result` and convert errors to tool error messages.

---

## 9. Complete Rust Implementation Sketch

```rust
// Core types
pub struct Agent {
    pub name: String,
    pub model: String,
    pub instructions: Instructions,
    pub functions: Vec<(String, AgentFunction)>,
    pub tool_choice: Option<String>,
    pub parallel_tool_calls: bool,
}

pub enum Instructions {
    Static(String),
    Dynamic(Arc<dyn Fn(&Context) -> String + Send + Sync>),
}

pub enum FunctionResult {
    Value(String),
    AgentHandoff(Arc<Agent>),
    Full(Result),
}

pub struct Result {
    pub value: String,
    pub agent: Option<Arc<Agent>>,
    pub context_variables: Context,
}

pub struct Response {
    pub messages: Vec<Message>,
    pub agent: Arc<Agent>,
    pub context_variables: Context,
}

// Runtime
pub struct SwarmRuntime {
    client: Arc<dyn LLMClient>,
}

impl SwarmRuntime {
    pub async fn run(
        &self,
        mut active_agent: Arc<Agent>,
        mut messages: Vec<Message>,
        mut context: Context,
        max_turns: usize,
    ) -> Result<Response> {
        let init_len = messages.len();
        let mut turns = 0;

        while turns < max_turns {
            // 1. Get completion
            let completion = self.get_completion(
                &active_agent,
                &messages,
                &context,
            ).await?;

            let mut message = completion.message;
            message.sender = Some(active_agent.name.clone());
            messages.push(message.clone());

            // 2. Check for tool calls
            let Some(tool_calls) = message.tool_calls else {
                break;
            };

            // 3. Execute tools
            let partial = self.handle_tool_calls(
                &tool_calls,
                &active_agent.functions,
                &mut context,
            ).await?;

            messages.extend(partial.messages);
            context.extend(partial.context_variables);

            // 4. Handle agent switch
            if let Some(new_agent) = partial.agent {
                active_agent = new_agent;
            }

            turns += 1;
        }

        Ok(Response {
            messages: messages[init_len..].to_vec(),
            agent: active_agent,
            context_variables: context,
        })
    }

    async fn handle_tool_calls(
        &self,
        tool_calls: &[ToolCall],
        functions: &[(String, AgentFunction)],
        context: &mut Context,
    ) -> Result<PartialResponse> {
        let function_map: HashMap<_, _> = functions.iter()
            .map(|(name, func)| (name.as_str(), func))
            .collect();

        let mut partial = PartialResponse::default();

        for tool_call in tool_calls {
            let Some(func) = function_map.get(tool_call.name.as_str()) else {
                partial.messages.push(Message {
                    role: "tool".to_string(),
                    tool_call_id: Some(tool_call.id.clone()),
                    content: format!("Error: Tool {} not found", tool_call.name),
                    ..Default::default()
                });
                continue;
            };

            let args = serde_json::from_str(&tool_call.arguments)?;
            let result = func(args, context).await?;

            let result = self.handle_function_result(result)?;

            partial.messages.push(Message {
                role: "tool".to_string(),
                tool_call_id: Some(tool_call.id.clone()),
                content: result.value.clone(),
                ..Default::default()
            });

            partial.context_variables.extend(result.context_variables);

            if let Some(agent) = result.agent {
                partial.agent = Some(agent);  // Last one wins
            }
        }

        Ok(partial)
    }

    fn handle_function_result(&self, result: FunctionResult) -> Result<Result> {
        match result {
            FunctionResult::Value(v) => Ok(Result {
                value: v,
                agent: None,
                context_variables: HashMap::new(),
            }),
            FunctionResult::AgentHandoff(agent) => Ok(Result {
                value: serde_json::to_string(&json!({
                    "assistant": agent.name
                }))?,
                agent: Some(agent),
                context_variables: HashMap::new(),
            }),
            FunctionResult::Full(r) => Ok(r),
        }
    }
}
```

---

## 10. Summary: The Swarm Pattern

**Core Innovation**: Agent handoffs via function return values

**Minimal Primitives**:
1. `Agent` (instructions + functions)
2. `Result` (value + optional agent + optional context)
3. `run()` loop (until no tool calls)

**Key Patterns**:
- Triage → Specialist routing
- Shared context variables
- Stateless execution
- Graceful error recovery
- Streaming with delimiters

**Why It Works**:
- Extremely simple (~300 LOC)
- Leverages OpenAI function calling
- No complex state management
- Easy to test and debug
- Natural composition of agents

**Rust Implementation Strategy**:
1. Use `enum FunctionResult` instead of duck typing
2. Auto-generate schemas with proc macros
3. Use `Arc<Agent>` for cheap cloning
4. Stream with `futures::Stream`
5. Context injection via explicit parameter or macro

---

## Files Analyzed

### Core Implementation
- `swarm/types.py` - Agent, Response, Result definitions
- `swarm/core.py` - Swarm class, run loop, tool handling
- `swarm/util.py` - Function to JSON schema conversion
- `swarm/__init__.py` - Public API exports

### Examples
- `examples/basic/agent_handoff.py` - Simple agent transfer
- `examples/basic/context_variables.py` - Context usage
- `examples/basic/function_calling.py` - Tool execution
- `examples/basic/simple_loop_no_helpers.py` - Manual REPL
- `examples/airline/configs/agents.py` - Multi-level routing
- `examples/airline/main.py` - Triage with context
- `examples/personal_shopper/main.py` - Hub-and-spoke with database
- `examples/support_bot/customer_service.py` - RAG integration
- `examples/weather_agent/agents.py` - Simple function calling

---

**Conclusion**: OpenAI Swarm proves that powerful multi-agent orchestration doesn't require complex frameworks. A simple loop + function returns = composable agent networks. Rust can match this elegance with enums and traits.
