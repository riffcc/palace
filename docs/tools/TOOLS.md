# Palace Tool System

## Quick Reference

**Run `pal call list` to see all available tools.**

## Available Tools

### smart_read
Token-efficient code reading with 5-layer analysis.

```bash
# Read with AST layer
pal call smart_read --input '{"path": "src/main.rs", "layer": "ast"}'

# Read specific symbol
pal call smart_read --input '{"path": "src/lib.rs", "layer": "ast", "symbol": "MyStruct"}'

# Batch read multiple files
pal call smart_read --input '{"reads": [{"path": "a.rs", "layer": "raw"}, {"path": "b.rs", "layer": "ast"}]}'
```

Layers: `raw`, `ast`, `call_graph`, `cfg`, `dfg`, `pdg`

### smart_write
Structure-aware code editing.

```bash
# Replace a function
pal call smart_write --input '{"path": "src/lib.rs", "operation": "replace_function", "target": "my_fn", "content": "fn my_fn() { ... }"}'

# Replace a symbol
pal call smart_write --input '{"path": "src/lib.rs", "operation": "replace_symbol", "target": "MyStruct", "content": "struct MyStruct { ... }"}'

# Insert after a symbol
pal call smart_write --input '{"path": "src/lib.rs", "operation": "insert_after", "target": "use_statements", "content": "use new_crate::Thing;"}'

# Delete a symbol
pal call smart_write --input '{"path": "src/lib.rs", "operation": "delete", "target": "deprecated_fn"}'

# Replace specific lines
pal call smart_write --input '{"path": "src/lib.rs", "operation": "replace_lines", "start": 10, "end": 20, "content": "new content"}'
```

Operations: `replace_function`, `replace_symbol`, `insert_after`, `delete`, `replace_lines`

### search
MRS-based semantic code search. Indexes codebase on demand.

```bash
# Search for code
pal call search --input '{"query": "error handling in HTTP requests", "limit": 10}'
```

### plane
Unified Plane.so API with systemd-style verbs.

```bash
# List issues in a project
pal call plane --input '{"verb": "list", "project": "PAL"}'

# Get specific issue
pal call plane --input '{"verb": "get", "project": "PAL", "id": "PAL-42"}'

# Create issue
pal call plane --input '{"verb": "create", "project": "PAL", "name": "Fix bug", "description": "...", "priority": "high"}'

# Update issue
pal call plane --input '{"verb": "update", "project": "PAL", "id": "PAL-42", "state": "in_progress"}'

# Delete issue
pal call plane --input '{"verb": "delete", "project": "PAL", "id": "PAL-42"}'

# List cycles
pal call plane --input '{"verb": "list", "type": "cycles", "project": "PAL"}'

# List project members
pal call plane --input '{"verb": "list", "type": "members", "project": "PAL"}'

# Raw API access
pal call plane --input '{"verb": "raw", "method": "GET", "path": "/workspaces/wings/projects/"}'
```

Verbs: `list`, `get`, `create`, `update`, `delete`, `raw`
Types: `project`, `issue`, `cycle`, `module`, `label`, `state`, `member`

## Tool Implementation Location

| Tool | Crate | File |
|------|-------|------|
| BashTool | llm-code-sdk | `src/tools/standard.rs` |
| ReadFileTool | llm-code-sdk | `src/tools/standard.rs` |
| WriteFileTool | llm-code-sdk | `src/tools/standard.rs` |
| EditFileTool | llm-code-sdk | `src/tools/standard.rs` |
| GlobTool | llm-code-sdk | `src/tools/standard.rs` |
| GrepTool | llm-code-sdk | `src/tools/standard.rs` |
| ListDirectoryTool | llm-code-sdk | `src/tools/standard.rs` |
| SmartReadTool | llm-code-sdk | `src/tools/smart/smart_read.rs` |
| SmartWriteTool | llm-code-sdk | `src/tools/smart/smart_write.rs` |
| SearchTool | llm-code-sdk | `src/tools/search.rs` |
| ZulipTool | director | `src/zulip_tool.rs` |

## Adding New Tools

1. **Check if it already exists** - run `pal call list` and search the codebase
2. **Choose the right crate:**
   - General-purpose tools → `llm-code-sdk/src/tools/`
   - Director-specific tools → `director/src/`
   - Plane.so tools → `palace-plane/src/`
3. **Implement the Tool trait:**

```rust
use llm_code_sdk::tools::{Tool, ToolResult};
use llm_code_sdk::types::{ToolParam, InputSchema};
use async_trait::async_trait;

pub struct MyTool;

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str {
        "my_tool"
    }

    fn to_param(&self) -> ToolParam {
        ToolParam::new("my_tool", InputSchema::object()
            .required_string("input", "Description"))
            .with_description("What this tool does")
    }

    async fn call(&self, input: HashMap<String, serde_json::Value>) -> ToolResult {
        let value = input.get("input").and_then(|v| v.as_str()).unwrap_or("");
        // Do work...
        ToolResult::success("result")
    }
}
```

4. **Register in pal call** - add to `crates/palace/src/main.rs` in the `call` command handler
5. **Update docs** - add to this file

## Creating Tool Sets

```rust
use llm_code_sdk::tools::{create_editing_tools, create_exploration_tools};

// Exploration tools (read-only): ReadFile, Glob, Grep, ListDirectory
let explore_tools = create_exploration_tools(&project_path);

// Editing tools (full access): all exploration + Write, Edit, Bash
let edit_tools = create_editing_tools(&project_path);
```
