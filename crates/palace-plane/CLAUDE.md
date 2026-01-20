# palace-plane

Plane.so integration for Palace.

## Quick Reference

### PlaneClient

Raw API client in `src/api.rs`:

```rust
use palace_plane::PlaneClient;

let client = PlaneClient::new()?;
let issues = client.list_active_issues(&config).await?;
```

### Using via pal call

The `plane` tool is available via CLI:

```bash
# List issues
pal call plane --input '{"verb": "list", "project": "PAL"}'

# Create issue
pal call plane --input '{"verb": "create", "project": "PAL", "name": "Fix bug"}'

# Update issue
pal call plane --input '{"verb": "update", "project": "PAL", "id": "PAL-42", "state": "done"}'
```

See [docs/tools/TOOLS.md](../../docs/tools/TOOLS.md) for full plane tool reference.

### DO NOT

- Create new Plane API clients elsewhere
- Duplicate issue management logic
- Add Plane tools to other crates

All Plane.so functionality belongs here.
