# Palace

Main binary that integrates all Palace components.
The glue that ties Mountain, Conductor, Director, and subsystems together.

## Purpose

Palace is the entry point—the unified CLI that orchestrates
AI-assisted software development, GBA gameplay, and everything between.

## Commands

```bash
# Play a GBA ROM with AI assistance
pal play --rom pokemon.gba --bios gba_bios.bin

# AI-assisted project development
pal project --path /my/project

# Generate task suggestions
pal next

# List pending/active tasks
pal ls
pal ls --active

# Approve tasks → create Plane.so issues
pal approve 1,2,3

# Initialize Palace for a project
pal init --workspace myworkspace --project myproject

# Test LLM connection
pal test-llm "Hello, what model are you?"

# List connected gamepads
pal gamepads
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                      Palace                          │
│                                                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│  │  Conductor  │  │  Director   │  │   Mountain  │  │
│  │  (human UI) │  │ (auto mgmt) │  │  (control)  │  │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  │
│         │                │                │          │
│         └────────────────┼────────────────┘          │
│                          │                           │
│  ┌─────────────┐  ┌──────┴──────┐  ┌─────────────┐  │
│  │ palace-gba  │  │palace-plane │  │palace-render│  │
│  │ (emulator)  │  │  (Plane.so) │  │   (wgpu)    │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  │
│                                                      │
│  ┌─────────────┐  ┌─────────────┐                   │
│  │ palace-ci   │  │llm-code-sdk │                   │
│  │  (Dagger)   │  │  (LLM API)  │                   │
│  └─────────────┘  └─────────────┘                   │
└─────────────────────────────────────────────────────┘
```

## Configuration

Environment variables (via `.env`):
- `ZAI_API_KEY`: Z.ai API key for glm-4.7
- `PLANE_API_KEY`: Plane.so API key
- `OPENROUTER_API_KEY`: OpenRouter for OC models

## License

AGPL-3.0
