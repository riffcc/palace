# Palace Documentation

## Architecture

- [Overview](architecture/ARCHITECTURE.md) - Core concepts and hierarchy
- [llm-code-sdk](architecture/LLM-CODE-SDK.md) - The autonomous coding agent SDK
- [Director](architecture/DIRECTOR.md) - Goal-directed project management agent

## Tools

- [Tool Reference](tools/TOOLS.md) - All available tools and how to use them

## Guides

Coming soon.

## Quick Reference

### The Hierarchy

```
Palace Session = llm-code-sdk + create_editing_tools()
Director = Palace Session + [PlaneTool, ZulipTool, SessionSpawnTool]
```

### Available Tools

Run `pal call list` to see all tools.

### Key Commands

```bash
# Call a tool
pal call <tool> --input '<json>'

# Start a Director daemon
systemctl --user start palace-director@<name>

# Control a Director
echo '{"cmd": "ping"}' | nc -U /run/user/$UID/palace/director/<name>.sock
```
