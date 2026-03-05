# Palace

Power armour for AI agents.
A development environment designed for rapid prototyping AND production-worthy code.

## Vision

Take software to endgame. Software that is:
- **Provably correct** (Lean verification)
- **Self-maintaining** (recursive improvement)
- **Intent-preserving** (LLM translation)
- **Context-aware** (shared observation)

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                              Palace                                  │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                      Human Layer                             │    │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │    │
│  │  │  Gamepad    │  │  Conductor  │  │   palace-render     │  │    │
│  │  │  (input)    │──│ (interviews)│──│   (dual-screen)     │  │    │
│  │  └─────────────┘  └─────────────┘  └─────────────────────┘  │    │
│  └───────────────────────────┬─────────────────────────────────┘    │
│                              │                                       │
│  ┌───────────────────────────┼─────────────────────────────────┐    │
│  │                    Control Layer                             │    │
│  │                           │                                  │    │
│  │  ┌─────────────┐  ┌──────┴──────┐  ┌─────────────────────┐  │    │
│  │  │  Director   │──│   Mountain  │──│   llm-code-sdk      │  │    │
│  │  │ (auto mgmt) │  │  (cascade)  │  │   (LLM API)         │  │    │
│  │  └─────────────┘  └─────────────┘  └─────────────────────┘  │    │
│  └───────────────────────────┬─────────────────────────────────┘    │
│                              │                                       │
│  ┌───────────────────────────┼─────────────────────────────────┐    │
│  │                  Integration Layer                           │    │
│  │                           │                                  │    │
│  │  ┌─────────────┐  ┌──────┴──────┐  ┌─────────────────────┐  │    │
│  │  │ palace-gba  │  │palace-plane │  │    palace-ci        │  │    │
│  │  │ (emulator)  │  │ (Plane.so)  │  │    (Dagger)         │  │    │
│  │  └─────────────┘  └─────────────┘  └─────────────────────┘  │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

## Data Flow

```
Human Intent
     │
     ▼
┌─────────┐    uncertain    ┌───────────┐
│Conductor│◀───────────────▶│  Director │
│(resolve)│                 │(autonomous)│
└────┬────┘                 └─────┬─────┘
     │                            │
     │     resolved intent        │
     └────────────┬───────────────┘
                  │
                  ▼
           ┌──────────┐
           │ Mountain │
           │(cascade) │
           └────┬─────┘
                │
    ┌───────────┼───────────┐
    │           │           │
    ▼           ▼           ▼
┌──────┐   ┌──────┐    ┌──────┐
│  8b  │   │flash │    │ full │  ... model tiers
└──┬───┘   └──┬───┘    └──┬───┘
   │          │           │
   └──────────┼───────────┘
              │
              ▼
      Control Decision
              │
              ▼
    Program (GBA, code, etc.)
```

## Model Cascade

Director selects which model decides vs reviews based on complexity and costs:

| Level | Decision Maker | Feedback From |
|-------|----------------|---------------|
| FAST | nvidia 8b | flash, mini, full |
| FLASH | glm-4.7-flash | mini, full |
| MINI | ministral-14b | full |
| FULL | glm-4.7 | (none) |
| FULL+OC | glm-4.7 | GPT-5.2, Opus, GPT-5.2-Pro |

## Transactional Codegen

Build up arbitrarily large changesets with validation gates:

```
Setup (define preconditions)
  │
  ▼
Build (accumulate shadow edits)
  │
  ▼
Validate (compile, test in worktree)
  │
  ▼
Commit (atomic apply when passing)
```

## Crates

| Crate | Purpose |
|-------|---------|
| `palace` | Main binary, glues everything together |
| `conductor` | Human interface, recursive interviews |
| `director` | Autonomous project management |
| `mountain` | Cascading LLM control |
| `llm-code-sdk` | LLM API with smart code analysis |
| `palace-plane` | Plane.so integration |
| `palace-gba` | GBA emulator wrapper |
| `palace-render` | wgpu dual-screen rendering |
| `palace-ci` | Dagger CI pipelines |

## Prerequisites

### System Dependencies (Ubuntu/Debian)

```bash
# Required for gamepad support and audio
sudo apt-get install -y libudev-dev libasound2-dev
```

### Git Submodules and External Dependencies

Palace uses `llm-code-sdk` as a submodule and requires two additional repos
to be cloned alongside it:

```bash
# Clone Palace and initialize submodules
git clone https://github.com/riffcc/palace.git
cd palace
git submodule update --init --recursive

# Clone required external dependencies (at the same level as palace/)
cd ..
git clone https://github.com/riffcc/minirust-search.git
git clone https://github.com/riffcc/tree-sitter-nim.git tree-sitter-nim-fork
cd palace
```

Your directory structure should look like:
```
parent/
├── palace/
├── minirust-search/
└── tree-sitter-nim-fork/
```

### Rust

Requires Rust 1.75+ (for async trait support).

```bash
# Install via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Quick Start

```bash
# Build
cargo build

# Install
cargo install --path crates/palace

# Initialize a project
pal init --workspace myworkspace --project myproject

# Generate task suggestions
pal next

# List and approve tasks
pal ls
pal approve 1,2,3

# Play GBA with AI
pal play --rom pokemon.gba --bios gba_bios.bin
```

## Development

Palace is its own Palace project—we use it to build itself.

```bash
# Run tests
cargo test --workspace

# Run with verbose logging
RUST_LOG=debug pal next
```

## License

GNU Affero General Public License v3.0

## Documentation

- [Specification](synthesis/SPEC.md) - Full technical specification
- [Paper](paper/) - Academic paper on Palace architecture
