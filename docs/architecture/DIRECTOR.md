# Director

The Director is a goal-directed autonomous agent that manages projects.

## Concept

**Director = Palace Session + Director tools**

A Director is a Palace Session (llm-code-sdk + standard tools) with additional tools:
- **Plane tool**: Read issues, create tasks, update status
- **Zulip tool**: Communicate, send polls, get human input
- **Session spawn tool**: Delegate work to sub-sessions

## Scaling Model

```
┌─────────────────────────────────────────────────────────────────┐
│                    Director Daemon (Host A)                      │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │  Director 1  │  │  Director 2  │  │  Director N  │          │
│  │   (tealc)    │  │   (daniel)   │  │     ...      │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
│         │                │                 │                    │
│         └────────────────┼─────────────────┘                    │
│                          │ Telepathy (internal)                 │
└──────────────────────────┼──────────────────────────────────────┘
                           │
              Zulip DMs/Chats (external coordination)
                           │
┌──────────────────────────┼──────────────────────────────────────┐
│                    Director Daemon (Host B)                      │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐                             │
│  │  Director 3  │  │  Director 4  │                             │
│  │   (samantha) │  │   (jarvis)   │                             │
│  └──────────────┘  └──────────────┘                             │
└─────────────────────────────────────────────────────────────────┘
```

- **One daemon** can host **N Directors** on a single machine
- **Multiple daemons** across hosts for horizontal scaling
- **Telepathy**: Internal coordination between Directors on same daemon
- **Zulip DMs/Chats**: External coordination between Directors across hosts
- Directors hand work to each other and to Palace sessions

## Running a Director

### As a systemd service

```bash
# Install the service (one time)
cp deploy/palace-director@.service ~/.config/systemd/user/
systemctl --user daemon-reload

# Create environment file
mkdir -p ~/.config/palace/env
cat > ~/.config/palace/env/tealc << 'EOF'
PALACE_PROJECT=/home/wings/projects/palace-2026-v2
ZULIP_EMAIL=bot@example.com
ZULIP_API_KEY=your-key
ZULIP_SITE=https://chat.example.com
EOF

# Start
systemctl --user start palace-director@tealc
systemctl --user status palace-director@tealc

# View logs
journalctl --user -u palace-director@tealc -f
```

### Control socket

Directors listen on a Unix socket at `/run/user/$UID/palace/director/<name>.sock`

```bash
# Ping
echo '{"cmd": "ping"}' | nc -U /run/user/1000/palace/director/tealc.sock

# Execute a task
echo '{"cmd": "exec", "prompt": "List all TODO comments", "model": "lm:/qwen3-8b"}' | nc -U ...

# Get status
echo '{"cmd": "status"}' | nc -U ...
```

Control commands use internally tagged serde: `{"cmd": "command_name", ...params}`

## Control Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `ping` | Health check | none |
| `status` | Get current status | none |
| `exec` | Execute a task | `prompt`, `model` |
| `session` | Spawn a sub-session | `target`, `strategy` |
| `reload` | Reload configuration | none |
| `shutdown` | Graceful shutdown | none |

## Zulip Integration

Directors post to Zulip streams:
- Topic: `director/<daemon-name>` (e.g., `director/tealc`)
- Message #1: Current status (updated in place)
- Message #2: Todo list (updated in place)
- Subsequent messages: Activity log

```rust
use director::ZulipTool;

let zulip = ZulipTool::from_env()?;
zulip.send("palace", "director/tealc", "Status: Working on PAL-42").await?;
```

## Plane Integration

Directors use the `plane` tool to manage issues:

```bash
# List current work
pal call plane --input '{"verb": "list", "project": "PAL", "state": "in_progress"}'

# Pick up an issue
pal call plane --input '{"verb": "update", "project": "PAL", "id": "PAL-42", "state": "in_progress"}'

# Mark complete
pal call plane --input '{"verb": "update", "project": "PAL", "id": "PAL-42", "state": "done"}'
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      DIRECTOR                            │
│                                                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │              Palace Session                       │   │
│  │  (llm-code-sdk + create_editing_tools())         │   │
│  └──────────────────────────────────────────────────┘   │
│                         +                                │
│  ┌──────────────────────────────────────────────────┐   │
│  │              Director Tools                       │   │
│  │  - PlaneTool (issue management)                  │   │
│  │  - ZulipTool (communication)                     │   │
│  │  - SessionSpawnTool (delegation)                 │   │
│  └──────────────────────────────────────────────────┘   │
│                         │                                │
└─────────────────────────┼────────────────────────────────┘
                          │
          ┌───────────────┼───────────────┐
          ▼               ▼               ▼
     ┌─────────┐    ┌─────────┐    ┌─────────┐
     │ Plane   │    │  Zulip  │    │  Sub-   │
     │   .so   │    │         │    │Sessions │
     └─────────┘    └─────────┘    └─────────┘
```

## FOOM - Fully Out of Order Management

The end goal:

```
Human (via Zulip/CLI)
    │
    ▼
Director (autonomous project manager)
    │
    ├── Reads Plane issues
    ├── Prioritizes work
    ├── Spawns Palace Sessions for each task
    ├── Monitors progress
    ├── Updates Plane status
    └── Escalates to human when needed
            │
            ▼
        Palace Sessions (work on specific issues)
            │
            └── Full autonomous coding capability
```

Each level is just: `llm-code-sdk` + appropriate tools.
