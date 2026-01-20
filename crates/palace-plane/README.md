# palace-plane

Plane.so integration for Palace.
Project management, task tracking, and AI-driven task generation.

## Purpose

Connects Palace to Plane.so for issue tracking and project management.
Analyzes codebases with LLM to generate actionable task suggestions,
manages the approval workflow, and syncs with Plane.so.

## Features

- Full integration with the entire Plane.so API
- Manage and reason about tasks
  - Verification end-to-end - if it's marked as completed in Plane, do all the tests pass?
- Cycles as sprints
- Modules as milestones
- Tasks as work items
  - Status - Backlog? To Do? In Progress? Done? Cancelled?
  - Priority - Urgent, High, Medium, Low, None
  - Assignee - assign tasks to people
  - Labels - categorize tasks with labels
  - Start date - when is the task expected to start?
    - When it has started, update it to the current date and time
  - End date - when is the task expected to be completed?
    - When it has been completed, update it to the current date and time
  - Cycle - which cycle is the task part of?
  - Module - which module is the task part of?
- Full relation support
  - Relates to, duplicate of, blocked by, blocking, starts before, starts after, finishes before, finishes after
- Attachments - attach information to tasks
- **Task Generation**: LLM analyzes code, suggests next actions
- **Pending Queue**: Review suggestions before creating issues
- **Project Config**: Per-project workspace/project mapping
- **Storage**: Local task storage in `~/.palace/`

### Plane.so Mapping

| Plane.so | Palace Use |
|----------|------------|
| Cycles | Sprints |
| Modules | Milestones |
| Tasks | Work items |
| Relations | Dependencies (blocked by, blocking, etc.) |

### Task Properties

- **Status**: Backlog, To Do, In Progress, Done, Cancelled
- **Priority**: Urgent, High, Medium, Low, None
- **Assignee**: Assign tasks to people
- **Labels**: Categorize with labels
- **Dates**: Start/end tracking with auto-update
- **Relations**: Relates to, duplicate of, blocked by, blocking, etc.

## Workflow

```
pal next          Generate suggestions from codebase analysis
    │
    ▼
Pending Tasks     Review locally before committing
    │
    ▼
pal approve 1,2   Create Plane.so issues for approved tasks
    │
    ▼
Plane.so Issues   Track in project management system
    │
    ▼
Verification      If marked done, do all tests pass?
```

## Configuration

Stored in `.palace/config.json`:

```json
{
  "workspace": "myworkspace",
  "project_slug": "myproject"
}
```

## API Usage

```rust
use palace_plane::{PlaneClient, ProjectConfig};

let config = ProjectConfig::load(&project_path)?;
let client = PlaneClient::new(&api_key, &config.workspace);

// Create issue
let issue = client.create_issue(&config.project_slug, &task).await?;

// List issues
let issues = client.list_issues(&config.project_slug).await?;
```

## License

AGPL-3.0
