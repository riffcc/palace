# Palace Repository Map

## Overview

Palace is an AI-assisted software development framework with gamepad control,
designed for rapid prototyping and production-worthy code.
It integrates LLM-based project management, GBA emulation, and dual-screen UI rendering.

**License:** GNU Affero General Public License v3.0
**Rust Edition:** 2024
**Workspace Version:** 0.1.0

---

## Workspace Structure

```
palace-2026-v2/
├── Cargo.toml              # Workspace root with shared dependencies
├── CLAUDE.md               # Project principles and standards
├── README.md               # Project overview
├── .palace/                # Local project config (this directory)
│   └── REPOMAP.md          # This file
├── synthesis/              # Design documents and handoffs
│   ├── SPEC.md             # Architecture specification
│   └── HANDOFF.md          # Current state and next steps
└── crates/                 # All Rust crates
    ├── palace/             # Main CLI binary
    ├── llm-code-sdk/       # LLM API SDK with tool use
    ├── mountain/           # Time-delayed cascading LLM control
    ├── conductor/          # Recursive interview UI with gamepad
    ├── director/           # Autonomous project management
    ├── palace-plane/       # Plane.so integration
    ├── palace-render/      # Dual-screen wgpu rendering
    ├── palace-gba/         # GBA emulation wrapper
    ├── palace-ci/          # Dagger-powered CI pipelines
    └── pal/                # Legacy task CLI (consolidation pending)
```

---

## Crate Reference

### palace (Main Binary)

**Purpose:** Central application orchestrator tying together all Palace subsystems

**Location:** `crates/palace/`

**CLI Commands:**
- `palace play --rom --bios [--turbo]` - Play GBA ROM with AI assistance
- `palace project --path [--name]` - AI-assisted project development
- `palace next --path` - Generate task suggestions via LLM
- `palace ls [--active] --path` - List pending/active tasks
- `palace gamepads` - Show connected gamepads
- `palace test-llm [prompt]` - Test LM Studio connection

**Key Files:**
| File | Purpose |
|------|---------|
| `src/main.rs` | CLI entry point with clap subcommands |
| `src/app.rs` | Main application logic and event loop |
| `src/gamepad.rs` | Gamepad input handling (gilrs) |
| `src/inference.rs` | LM Studio client integration |
| `src/gba_controllable.rs` | GBA emulator control interface |
| `src/config.rs` | Configuration management |

---

### llm-code-sdk

**Purpose:** Rust SDK for LLM APIs with agentic tool use capabilities

**Location:** `crates/llm-code-sdk/`

**Key Modules:**
| Module | Purpose |
|--------|---------|
| `src/client/` | OpenAI-compatible API client |
| `src/types/` | Message, content block, tool definitions |
| `src/streaming/` | SSE stream handling |
| `src/tools/` | Tool trait and execution loop |
| `src/e2e/` | Playwright-based E2E testing (optional feature) |

**Features:**
- `e2e` - Optional end-to-end testing support with Playwright

---

### mountain

**Purpose:** Enables realtime AI control by compensating for LLM response latency through cascading models

**Location:** `crates/mountain/`

**Architecture:**
Runs multiple models in parallel with time-delayed execution (2-3 seconds behind realtime).

```
REALTIME STATE ──────────────────────────────────────────────►
     │
     ├──► Tiny Local (100ms) ────┐
     ├──► Medium Local (500ms) ──┼──► CASCADE MERGE
     ├──► Fast Cloud (800ms) ────┤         │
     └──► Big Cloud (frozen) ────┴─────────┘
                                            │
                                            ▼
                            WASM EXECUTION (delayed 2-3 seconds)
```

**Model Tiers:**
| Tier | Model | Latency |
|------|-------|---------|
| FAST | nvidia_orchestrator-8b@q6_k_l | 100ms |
| FLASH | glm-4.7-flash | 800ms |
| MINI | ministral-3-14b-reasoning | 500ms |
| FULL | glm-4.7 | 2000ms |

**Key Files:**
| File | Purpose |
|------|---------|
| `src/cascade.rs` | Model cascading orchestration |
| `src/delay.rs` | Time-delay buffering |
| `src/confidence.rs` | Confidence slider UI state |
| `src/controller.rs` | Program controller and Controllable trait |
| `src/model.rs` | Model tier definitions |
| `src/state.rs` | Program state tracking |
| `src/stream.rs` | LLM output streaming |

**Features:**
- `wasm-isolation` - Optional WASM runtime for delayed execution

---

### conductor

**Purpose:** Interactive decision resolution system for human-AI collaboration with gamepad interface

**Location:** `crates/conductor/`

**Gamepad Controls:**
| Input | Action |
|-------|--------|
| RB/LB | Toggle between visible AI agents |
| L2/R2 | Control output granularity (context depth) |
| X + Left Stick | Topic/Focus radial menu |
| X + Right Stick | Intent/Strategy radial menu (L3 toggles) |
| Touchpad | Dynamic selection/flinging/combining |
| A/B | Confirm/Back |

**Key Files:**
| File | Purpose |
|------|---------|
| `src/interview.rs` | Recursive interview tree |
| `src/question.rs` | Question/answer system with caching |
| `src/gamepad.rs` | PS5 controller with haptic feedback |
| `src/radial.rs` | Radial menu UI (dual thumbstick) |
| `src/touchpad.rs` | Touchpad gesture recognition |
| `src/output.rs` | Agent output streaming |
| `src/remote.rs` | Multi-device coordination |

---

### director

**Purpose:** AI project manager orchestrating development according to goals

**Location:** `crates/director/`

**Architecture:**
Goals → Planner → Executor → Monitor,
with Plane.so/GitHub/Human Review integration.

**Key Files:**
| File | Purpose |
|------|---------|
| `src/project.rs` | Project management |
| `src/goals.rs` | Goal definition with priority/status |
| `src/issues.rs` | Issue tracking and classification |
| `src/planner.rs` | Task planning orchestrator |
| `src/state.rs` | Director state and metrics |

---

### palace-plane

**Purpose:** Plane.so API client and task management

**Location:** `crates/palace-plane/`

**Plane.so Mapping:**
| Plane.so Concept | Palace Usage |
|------------------|--------------|
| Cycles | Sprints |
| Modules | Milestones |
| Tasks | Work items |
| Relations | blocks, blocked_by, relates_to |

**Storage Layout:**
```
~/.palace/
  config.yml                    # PLANE_API_KEY, defaults
  projects/<safe-path>/
    PENDING-{id}.json          # Pending task suggestions
    APPROVED-{id}.json         # Approved tasks

.palace/
  project.yml                   # workspace, project_slug
```

**Key Files:**
| File | Purpose |
|------|---------|
| `src/api.rs` | Plane.so REST API client |
| `src/config.rs` | Global and project configuration |
| `src/storage.rs` | Local task storage (redb) |
| `src/task.rs` | Task management workflow |

---

### palace-render

**Purpose:** wgpu-based GPU-accelerated rendering for dual-screen UI

**Location:** `crates/palace-render/`

**Display Layout:**
- Left screen: Game/program output
- Right screen: Orchestrator (AI output/UI controls)
- Bottom-right: Confidence slider widget

**Key Files:**
| File | Purpose |
|------|---------|
| `src/renderer.rs` | Main wgpu renderer |
| `src/screen.rs` | Dual-screen management |
| `src/ui.rs` | UI state and elements |
| `src/slider.rs` | Confidence slider widget |
| `src/text.rs` | Text rendering (glyphon) |
| `src/display.rs` | Display profiles |

**Example:** `examples/demo.rs` - Rendering demonstration

---

### palace-gba

**Purpose:** Game Boy Advance emulation via rustboyadvance-ng

**Location:** `crates/palace-gba/`

**Features:**
- Real-time audio output (tinyaudio)
- Screen capture for LLM vision (base64 PNG)
- Save state support for Mountain time-delay sync
- Input handling for gamepad control

**Screen Dimensions:** 240x160 pixels

**Key Files:**
| File | Purpose |
|------|---------|
| `src/emulator.rs` | GBA emulator wrapper |
| `src/audio.rs` | Audio player |

---

### palace-ci

**Purpose:** Configurable build/test/run pipelines powered by Dagger

**Location:** `crates/palace-ci/`

**CI Levels:**
| Level | Description |
|-------|-------------|
| Simple | Check compilation |
| Lint | Zero warnings |
| Basic | Run tests |
| BasicLong | ALL tests (including ignored) |
| Run | Run binary (debug) |
| RunProd | Run binary (release) |

**Key Files:**
| File | Purpose |
|------|---------|
| `src/pipeline.rs` | Pipeline orchestrator |
| `src/config.rs` | Project type configuration |
| `src/levels.rs` | CI granularity levels |
| `src/rust.rs` | Rust-specific pipeline |
| `src/scenarios.rs` | Test scenario runner |

---

### pal (Legacy)

**Purpose:** Legacy task management binary

**Location:** `crates/pal/`

**Status:** May need consolidation with palace-plane

**Storage:** ReDB embedded database

---

## Key Dependencies

| Category | Crate | Version |
|----------|-------|---------|
| Async | tokio | full features |
| HTTP | reqwest | json, stream |
| Serialization | serde, serde_json, serde_yaml | - |
| GPU | wgpu | 28 |
| Windowing | winit | 0.30 |
| Text | glyphon | 0.10 |
| Database | redb | 2 |
| CLI | clap | 4 (derive) |
| Gamepad | gilrs | 0.11 |
| GBA | rustboyadvance-core | - |
| Audio | tinyaudio | - |
| CI | dagger-sdk | - |

---

## Environment

| Variable | Purpose |
|----------|---------|
| `ZAI_API_KEY` | LLM API key |
| `PLANE_API_KEY` | Plane.so API key |

LM Studio runs on `localhost:1234`

---

*Generated for Palace 2026 v2*
