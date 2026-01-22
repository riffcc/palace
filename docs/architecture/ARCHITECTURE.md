# Palace Architecture

## Core Concept

**llm-code-sdk is a full autonomous coding agent.**
It's not just a "tool runner" - it's an agent that can:
- Explore codebases autonomously
- Make decisions based on context
- Take actions (read, write, edit, search, execute)
- Run agentic loops until tasks complete
- Handle multi-turn conversations with tool use

**Palace Session = llm-code-sdk + standard tools**
A full autonomous coding session that can work on code independently.

**Director = Palace Session + Director tools (Plane, Zulip, session spawning)**
A goal-directed agent that manages projects, delegates to sub-sessions, and coordinates work.

## Tool System

### Where Tools Live

```
llm-code-sdk/src/tools/     # Standard tools (DO NOT DUPLICATE)
├── standard.rs             # BashTool, ReadFileTool, WriteFileTool, EditFileTool,
│                           # GlobTool, GrepTool, ListDirectoryTool
├── smart/                  # SmartReadTool, SmartWriteTool (when feature="smart")
├── search.rs               # SearchTool (when feature="search")
├── registry.rs             # ToolRegistry for central registration
├── runner.rs               # ToolRunner - the agentic loop
└── traits.rs               # Tool trait definition

director/src/               # Director-specific functionality
├── zulip_tool.rs           # ZulipTool - send messages, polls, get messages
└── (other director modules)

palace-plane/src/           # Plane.so integration
├── api.rs                  # PlaneClient - raw API access
└── exploration.rs          # Exploration tools (read, list, glob, grep)
```

### Available Tools via `pal call`

Run `pal call list` to see available tools:

| Tool | Description | Input Format |
|------|-------------|--------------|
| `smart_read` | Token-efficient code reading with 5-layer analysis | `{"path": "...", "layer": "ast"}` |
| `smart_write` | Structure-aware code editing | `{"path": "...", "operation": "...", "content": "..."}` |
| `search` | MRS-based semantic code search | `{"query": "...", "limit": 10}` |
| `plane` | Unified Plane.so API | `{"verb": "list", "project": "PAL"}` |

### Tool Trait

All tools implement the `Tool` trait from `llm-code-sdk`:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn to_param(&self) -> ToolParam;
    async fn call(&self, input: HashMap<String, serde_json::Value>) -> ToolResult;
}
```

## Creating an Agent

### Basic Palace Session (Claude Code equivalent)

```rust
use llm_code_sdk::{Client, MessageCreateParams};
use llm_code_sdk::tools::{ToolRunner, create_editing_tools};

let client = Client::new("api-key")?;
let tools = create_editing_tools(&project_path);
let runner = ToolRunner::new(client, tools);

// Run agentic loop
let result = runner.run(params).await?;
```

### Director (Palace Session + Director tools)

```rust
use llm_code_sdk::tools::{ToolRunner, create_editing_tools};

// Start with standard tools
let mut tools = create_editing_tools(&project_path);

// Add Director tools
tools.push(Arc::new(PlaneTool::new()));      // Plane.so integration
tools.push(Arc::new(ZulipTool::from_env()?)); // Zulip communication
tools.push(Arc::new(SessionSpawnTool::new())); // Spawn sub-sessions

let runner = ToolRunner::new(client, tools);
```

## Plane.so Tool

The `plane` tool provides full Plane.so API access:

```bash
# List issues in a project
pal call plane --input '{"verb": "list", "project": "PAL"}'

# Create an issue
pal call plane --input '{"verb": "create", "project": "PAL", "name": "Fix bug", "priority": "high"}'

# Update an issue
pal call plane --input '{"verb": "update", "project": "PAL", "id": "PAL-42", "state": "done"}'

# List cycles
pal call plane --input '{"verb": "list", "type": "cycles", "project": "PAL"}'

# Raw API access
pal call plane --input '{"verb": "raw", "method": "GET", "path": "/workspaces/wings/projects/"}'
```

## Zulip Tool

The `ZulipTool` in director crate:

```rust
use director::ZulipTool;

let zulip = ZulipTool::from_env()?;

// Send a message
zulip.send("stream", "topic", "content").await?;

// Send a poll
zulip.send_poll("stream", "topic", "Question?", &["Option 1", "Option 2"]).await?;

// Get messages
let messages = zulip.get_messages("stream", "topic", Some(10)).await?;
```

## Director Daemon

The Director daemon (`palace-director`) runs as a systemd service:

```bash
# Start
systemctl --user start palace-director@tealc

# Control via socket
echo '{"cmd": "ping"}' | nc -U /run/user/1000/palace/director/tealc.sock

# Execute a task
echo '{"cmd": "exec", "prompt": "List the files", "model": "lm:/qwen3-8b"}' | nc -U ...
```

Control commands use internally tagged serde: `{"cmd": "command_name", ...params}`

## Model Routing

Models can be prefixed to route to different backends:

| Prefix | Backend |
|--------|---------|
| `lm:/` | LM Studio local |
| `z:/` | ZhipuAI |
| `or:/` | OpenRouter |
| `ms:/` | Microsoft |
| (none) | Default (usually Anthropic) |

Example: `lm:/qwen3-8b@q6_k_l`

## FOOM - Fully Out of Order Management

The goal architecture:

```
Claude Code (you)
    │
    ▼
Director (goal-directed Palace Session)
    │
    ├── Plane tool (read issues, update status)
    ├── Zulip tool (communicate, get human input)
    └── Session spawn tool (delegate work)
            │
            ▼
        Palace Sessions (work on specific issues)
```

Each layer is just ToolRunner + appropriate tools.
