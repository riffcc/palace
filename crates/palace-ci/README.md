# palace-ci

Dagger-powered CI pipelines with configurable granularity.
Build, test, and deploy with varying intensity levels.

## Purpose

Provides flexible CI that adapts to the development stage—fast
checks during TDD, thorough tests before merge, exhaustive
validation before release.

## Granularity Levels

| Level | Description | Use Case |
|-------|-------------|----------|
| `simple` | Compilation only | Instant feedback |
| `lint` | Zero warnings, zero linter errors | Code quality gate |
| `basic` | Run tests | Standard validation |
| `basic-long` | Run ALL tests including ignored | Thorough check |
| `run` | Basic + run binary | Integration test |
| `run-prod` | Release mode execution | Performance validation |
| `scenario` | Full scriptable E2E | Complete verification |

## Testing Tiers

Maps to development workflow:

```
draft    (<5s)   - TDD velocity, changed sections only
dev      (~10m)  - Coffee break, almost all issues
release  (hours) - No expense spared, certification
```

## Usage

```rust
use palace_ci::{Pipeline, Granularity};

let pipeline = Pipeline::rust()
    .granularity(Granularity::Basic)
    .build();

pipeline.run().await?;
```

## Dagger Integration

Runs in containers for reproducibility:
- Consistent environments across machines
- Parallel execution where possible
- Caching for incremental builds

## License

AGPL-3.0
