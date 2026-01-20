# llm-code-sdk

A Rust SDK for LLM APIs with agentic tool use capabilities.

Inspired by and derives patterns from
[anthropic-sdk-python](https://github.com/anthropics/anthropic-sdk-python) (MIT Licensed).

## What It Provides

llm-code-sdk is a **full autonomous coding agent** that can:

- **Messages API**: Create messages with text, images, and documents
- **Streaming**: Real-time streaming responses via SSE
- **Tool Use**: Define and execute tools in agentic loops
- **Extended Thinking**: Enable model reasoning processes
- **Token Counting**: Count tokens before sending requests
- **Agentic Loops**: Run autonomous multi-turn conversations until task completion

## Core Components

### Client

Multi-backend LLM client supporting:
- Anthropic API
- ZhipuAI (Z.ai)
- OpenRouter
- LM Studio (local)
- OpenAI-compatible endpoints

```rust
use llm_code_sdk::{Client, ClientBuilder, ApiFormat};

// Default (Anthropic)
let client = Client::new("api-key")?;

// ZhipuAI
let client = Client::zai("api-key")?;

// LM Studio (local)
let client = ClientBuilder::new()
    .base_url("http://localhost:1234/v1")
    .api_format(ApiFormat::OpenAI)
    .build()?;
```

### ToolRunner

The agentic loop executor. Sends messages, executes tool calls, continues until completion.

```rust
use llm_code_sdk::{Client, MessageCreateParams, MessageParam};
use llm_code_sdk::tools::{ToolRunner, create_editing_tools};

let client = Client::new("api-key")?;
let tools = create_editing_tools(&project_path);
let runner = ToolRunner::new(client, tools);

let result = runner.run(MessageCreateParams {
    model: "model-name".into(),
    max_tokens: 4096,
    messages: vec![MessageParam::user("Fix the bug in main.rs")],
    ..Default::default()
}).await?;
```

### Standard Tools

Pre-built tools for code operations:

| Tool | Description |
|------|-------------|
| `ReadFileTool` | Read file contents |
| `WriteFileTool` | Write/create files |
| `EditFileTool` | Edit existing files |
| `GlobTool` | Find files by pattern |
| `GrepTool` | Search file contents |
| `ListDirectoryTool` | List directory contents |
| `BashTool` | Execute shell commands |

Tool sets:
```rust
// Read-only exploration
let tools = create_exploration_tools(&project_path);

// Full editing capabilities
let tools = create_editing_tools(&project_path);
```

### Tool Trait

Implement custom tools:

```rust
use llm_code_sdk::tools::{Tool, ToolResult};
use llm_code_sdk::types::{ToolParam, InputSchema};
use async_trait::async_trait;

pub struct MyTool;

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }

    fn to_param(&self) -> ToolParam {
        ToolParam::new("my_tool", InputSchema::object()
            .required_string("input", "The input"))
            .with_description("Does something useful")
    }

    async fn call(&self, input: HashMap<String, serde_json::Value>) -> ToolResult {
        // Implementation
        ToolResult::success("result")
    }
}
```

### Streaming

Real-time response streaming:

```rust
use tokio_stream::StreamExt;
use llm_code_sdk::streaming::StreamEvent;

let mut stream = client.messages().stream(params).await?;

while let Some(event) = stream.next().await {
    match event {
        StreamEvent::Text { text, .. } => print!("{}", text),
        StreamEvent::ToolUse { name, input, .. } => { /* handle tool */ },
        StreamEvent::Done { message } => break,
        _ => {}
    }
}
```

### FunctionTool

Quick tool creation from closures:

```rust
use llm_code_sdk::tools::FunctionTool;
use llm_code_sdk::types::InputSchema;

let tool = FunctionTool::new(
    "greet",
    "Greet someone",
    InputSchema::object().required_string("name", "Name to greet"),
    |input| {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("World");
        Ok(format!("Hello, {}!", name))
    },
);
```

## Features

Enable optional features in Cargo.toml:

```toml
[dependencies]
llm-code-sdk = { path = "../llm-code-sdk", features = ["smart", "search"] }
```

| Feature | Description |
|---------|-------------|
| `smart` | SmartReadTool, SmartWriteTool with AST analysis |
| `search` | SearchTool with semantic code search |
| `e2e` | End-to-end testing utilities |

## Model Routing

Use prefixes to route to different backends:

| Prefix | Backend | Example |
|--------|---------|---------|
| `lm:/` | LM Studio | `lm:/qwen3-8b@q6_k_l` |
| `z:/` | ZhipuAI | `z:/glm-4-plus` |
| `or:/` | OpenRouter | `or:/anthropic/claude-3-opus` |
| (none) | Default | `glm-4-plus` |

## Usage in Palace

Palace Session = llm-code-sdk + `create_editing_tools()`

Director = Palace Session + Director tools (Plane, Zulip, session spawning)

The SDK provides everything needed to build autonomous coding agents.
