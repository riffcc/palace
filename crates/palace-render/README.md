# palace-render

GPU-accelerated rendering for Palace's dual-screen UI.
Built on wgpu for cross-platform support.

## Purpose

Renders the split-screen interface: program output on one side,
orchestration controls on the other.
Handles frame buffers, syntax highlighting, LLM streams,
and the confidence slider widget.

## Features

- **Dual Screen Layout**: Game/program + orchestration side-by-side
- **Frame Buffers**: Efficient GPU texture updates
- **Syntax Highlighting**: Real-time code display
- **LLM Streams**: Multiple model outputs with visual separation
- **Confidence Slider**: Bottom-right widget showing AssuranceLevel
- **View Modes**: Split, GameOnly, ConductorOnly

## Architecture

```
┌─────────────────────────────────────────────┐
│              palace-render                   │
│                                              │
│  ┌───────────────────┬───────────────────┐  │
│  │                   │                   │  │
│  │   Program View    │  Conductor View   │  │
│  │   (GBA/app)       │  (LLM streams)    │  │
│  │                   │                   │  │
│  │                   │                   │  │
│  │                   │                   │  │
│  └───────────────────┴───────────────────┘  │
│                              ┌────────────┐ │
│                              │ FLASH ▸    │ │
│                              └────────────┘ │
└─────────────────────────────────────────────┘
```

## View Modes

- **Split**: Both views visible (default)
- **GameOnly**: Full-screen program output
- **ConductorOnly**: Full-screen orchestration

Toggle with Guide button (single press: toggle conductor,
double press: cycle split/fullscreen).

## Usage

```rust
use palace_render::{Renderer, ViewMode};

let mut renderer = Renderer::new(&window)?;

// Update game frame
renderer.update_game_frame(&pixels);

// Update conductor content
renderer.update_conductor(&llm_output);

// Set view mode
renderer.set_view_mode(ViewMode::Split);

// Render
renderer.render()?;
```

## License

AGPL-3.0
