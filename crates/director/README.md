# Director

Autonomous AI project manager for Palace.
Orchestrates development by creating issues, prioritizing work,
and managing the full development lifecycle.

## Purpose

Director is the autonomous brain that manages projects
without constant human intervention.
It maintains goals, creates plans, monitors progress,
and escalates to humans (via Conductor) only when needed.

## Features

- **Goal Management**: Define high-level objectives,
  Director breaks them into actionable tasks
- **Plan Execution**: Creates and executes development plans
- **Progress Monitoring**: Tracks task completion and adjusts priorities
- **External Integration**: Plane.so for issues, GitHub for PRs
- **Human Escalation**: Routes uncertainties to Conductor

## Architecture

```
Goals (human-defined)
       │
       ▼
┌─────────────────┐
│    Director     │
│  ┌───────────┐  │
│  │  Planner  │  │──▶ Break goals into tasks
│  └───────────┘  │
│  ┌───────────┐  │
│  │ Executor  │  │──▶ Run tasks via Mountain
│  └───────────┘  │
│  ┌───────────┐  │
│  │  Monitor  │  │──▶ Track progress, adjust
│  └───────────┘  │
└─────────────────┘
       │
       ├──▶ Plane.so (issues)
       ├──▶ GitHub (PRs)
       └──▶ Conductor (escalation)
```

## Integration

Director coordinates with:
- **Mountain**: Execute code changes and program control
- **Conductor**: Human escalation for uncertainties
- **Palace-Plane**: Issue tracking and project state

## License

AGPL-3.0
