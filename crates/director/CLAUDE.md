# Director

See [docs/architecture/DIRECTOR.md](../../docs/architecture/DIRECTOR.md) for full documentation.

## Quick Reference

Director = Palace Session + Director tools

A Director is an autonomous project manager built on llm-code-sdk.

### Key Components

- `ZulipTool` (`src/zulip_tool.rs`) - Zulip messaging
- `ControlServer` (`src/control.rs`) - Unix socket control interface
- `palace-director` binary (`src/bin/palace-director.rs`) - The daemon

### Running

```bash
systemctl --user start palace-director@<name>
```

### Control Commands

```bash
echo '{"cmd": "ping"}' | nc -U /run/user/$UID/palace/director/<name>.sock
echo '{"cmd": "exec", "prompt": "...", "model": "lm:/qwen3-8b"}' | nc -U ...
```

### Adding Director Tools

Director tools go here, not in llm-code-sdk.
These are tools specific to project management, not general coding.

Examples:
- ZulipTool (communication)
- PlaneTool (issue tracking)
- SessionSpawnTool (delegation)
