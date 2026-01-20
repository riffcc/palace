# Mountain

Universal program scripting with cascading LLM control.
Compensates for model latency through intelligent orchestration.

## Purpose

Mountain enables AI to control arbitrary programs in real-time
by orchestrating multiple model tiers.
Decisions cascade through the tree—Director selects which model
decides vs reviews based on complexity and costs.

## Modes

1. **Blind Mode**: Input injection (keyboard, mouse, gamepad)
   to any program without modification
2. **Integrated Mode**: Rust SDK for rich state observation
   (compiled out in release builds)

## Architecture

```
Program State
     │
     ▼
┌─────────────────────────────────────────┐
│              Mountain                    │
│                                          │
│  ┌────────┐ ┌────────┐ ┌────────┐       │
│  │  8b    │ │ flash  │ │  full  │ ...   │
│  │ (fast) │ │(medium)│ │(smart) │       │
│  └────┬───┘ └────┬───┘ └────┬───┘       │
│       │          │          │            │
│       ▼          ▼          ▼            │
│  ┌──────────────────────────────────┐   │
│  │         Cascade Controller        │   │
│  │  (Director adjusts based on      │   │
│  │   complexity and costs)          │   │
│  └──────────────────────────────────┘   │
│                  │                       │
│                  ▼                       │
│           Control Decision               │
└─────────────────────────────────────────┘
     │
     ▼
Program Input (buttons, keys, etc.)
```

## AssuranceLevel

Controls which model tier makes decisions:

| Level | Decision Maker | Feedback From |
|-------|----------------|---------------|
| FAST | 8b | flash, mini, full |
| FLASH | flash | mini, full |
| MINI | mini | full |
| FULL | glm-4.7 | (none) |
| FULL+OC | glm-4.7 | GPT-5.2, Opus, Pro |

## Cascade Behavior

- Higher models can freeze input and wait for responses
- Models can respond while still processing
- Delays compensated by lower tiers updating decisions
- Director autonomously adjusts based on complexity and costs

## License

AGPL-3.0
