# Conductor

Human interface layer for Palace.
Resolves uncertainties through recursive interviews with branching Q&A.

## Purpose

Conductor is the bridge between human intent and AI execution.
When the AI is uncertain, it doesn't guess—it asks,
using a gamepad-driven interview system that branches and refines.

## Features

- **Recursive Interviews**: Questions branch based on answers,
  drilling down until intent is clear
- **Radial Menus**: Dual thumbstick selection
  - Left stick: Topic/Focus/Goal
  - Right stick: Intent/Method/Strategy
- **Touchpad Gestures**: PS5 touchpad for direct manipulation
- **Multi-Agent Streams**: View output from multiple LLMs simultaneously
- **Gamepad Controls**: Full PS5/Xbox controller support

## Architecture

```
Human Input (gamepad)
       │
       ▼
┌─────────────────┐
│   Conductor     │
│  ┌───────────┐  │
│  │ Interview │  │──▶ Questions branch recursively
│  │  System   │  │
│  └───────────┘  │
│  ┌───────────┐  │
│  │  Radial   │  │──▶ Contextual options regenerate
│  │  Menus    │  │
│  └───────────┘  │
│  ┌───────────┐  │
│  │  Stream   │  │──▶ Multiple LLM outputs visible
│  │  Viewer   │  │
│  └───────────┘  │
└─────────────────┘
       │
       ▼
  Resolved Intent → Director/Mountain
```

## Usage

Conductor is used internally by the `palace` binary.
It's activated when the user holds X to open radial menus
or when the AI needs clarification.

## License

AGPL-3.0
