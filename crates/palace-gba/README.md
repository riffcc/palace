# palace-gba

GBA emulator integration for Palace.
Wraps rustboyadvance-ng with Palace-specific features.

## Purpose

Provides Game Boy Advance emulation with hooks for AI control,
save state support for Mountain synchronization,
and screen capture for LLM vision analysis.

## Features

- **Audio Output**: Real-time audio via cpal
- **Screen Capture**: Frame buffer access for LLM vision
- **Save States**: Instant state save/load for Mountain
- **Input Handling**: Button injection for AI control
- **Turbo Mode**: Uncapped framerate for fast-forward

## Constants

```rust
pub const GBA_WIDTH: u32 = 240;
pub const GBA_HEIGHT: u32 = 160;
```

## Button Mapping

```rust
pub enum GbaButton {
    A, B, L, R,
    Start, Select,
    Up, Down, Left, Right,
}
```

## Integration with Mountain

Save states enable Mountain's time-delay synchronization:

```
t=0: State saved
t=1: Fast model suggests action
t=2: Action executed
t=3: Slow model disagrees
t=3: Reload state from t=0, replay with correction
```

## Usage

```rust
use palace_gba::GbaEmulator;

let mut emu = GbaEmulator::new(rom_path, bios_path)?;

// Run frame and get screen
let frame = emu.run_frame();

// Inject input
emu.press_button(GbaButton::A);

// Save/load state
let state = emu.save_state();
emu.load_state(&state);
```

## License

AGPL-3.0
