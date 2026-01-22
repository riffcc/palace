# Palace

Palace is a development environment with a difference.

Designed for rapid prototyping *and* production-worthy code,
and designed explicitly to be understandable, inspectable and moddable,
Palace allows you to build sustainable software.

## Documentation

**READ THESE FIRST:**
- [docs/README.md](docs/README.md) - Documentation index
- [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md) - Core architecture
- [docs/tools/TOOLS.md](docs/tools/TOOLS.md) - Available tools

## Core Principles

### DRY - DON'T REPEAT YOURSELF (CRITICAL)

**BEFORE WRITING ANY CODE, SEARCH THE CODEBASE FOR EXISTING IMPLEMENTATIONS.**

1. **SEARCH FIRST, CODE NEVER**
   - Before implementing ANY functionality, grep/glob for existing implementations
   - If something similar exists, USE IT or EXTEND IT
   - NEVER create parallel implementations of the same concept

2. **One Tool, One Place**
   - Tools live in ONE crate, ONE registry
   - `llm-code-sdk/src/tools/` for standard tools
   - `director/src/` for Director-specific tools
   - `palace-zulip/` for ALL Zulip functionality
   - NEVER duplicate tool implementations across crates

3. **Existing Infrastructure Checklist**
   Before writing new code, check:
   - [ ] Does `llm-code-sdk` already have this tool?
   - [ ] Does `director` already expose this via control commands?
   - [ ] Does `palace-zulip` already handle this messaging?
   - [ ] Does `palace-plane` already provide this Plane.so integration?
   - [ ] Is there a `pal call <tool>` that already does this?

4. **Red Flags That You're Violating DRY**
   - Writing `curl` commands instead of using existing HTTP clients
   - Creating new structs that mirror existing ones
   - Adding match arms in main.rs instead of extending the tool system
   - Implementing API calls that another crate already handles
   - Using `println!` instead of `tracing`

5. **The Right Way**
   - EXTEND existing tools with new functionality
   - ADD methods to existing structs
   - IMPLEMENT traits on existing types
   - REUSE existing clients, not create new ones

6. **Emergent Behaviour**
   - EMERGENT BEHAVIOUR driven by enabling models to have new capabilities
   - Like a crow that can make its own tools as needed
   - Every change tracked in Plane in realtime

**EXAMPLE VIOLATIONS (DO NOT DO THESE):**
- Creating ZulipTool in palace/main.rs when palace-zulip exists
- Writing curl commands to Zulip API when ZulipTool.send() exists
- Adding Plane API calls when palace-plane already has PlaneClient
- Duplicating Tool trait implementations across crates

### Test driven development

Everything in Palace is designed for test-first development.
TDD allows us to ensure that code retains a base level of quality.

### The Ratchet

Refactors and breaking changes should be intentional.
* All tests should pass and light green.
* Technical debt should be minimized, not carried forwards.
* Users can and should expect everything to work.

### The user is there to develop and play, not to test

* If it is at all possible to automate the testing of something,
  the user should not have to do it. Ever, period.
* If it possible to automate but we lack the framework to do so,
  that is a bug. Fix that.

### If your tools do not work, break your tools

This means don't complain that something doesn't support what you're trying to do -
if the primitive makes sense, *add it* TO THE EXISTING TOOL, not create a new one.

### Structure determines action

* The UI and structure we afford to the users will shape their experience,
  their capabilities and how happy they are.
* If we give them a narrow, focused setup,
  they will feel constrained but possibly have a good golden path experience.
* If we give them a sandbox, they might enjoy that or feel decision paralysis.
* It is our job to strike a fair balance.

## Documentation standards
* Semantic Line Breaks (SemBr) should be used everywhere
  that it makes sense to use them, including READMEs,
  all Markdown documentation, and anything that needs to remain readable.

## Model Architecture (AssuranceLevel)

Palace uses a multi-model cascade where fast models DECIDE and slower models provide FEEDBACK.
The `AssuranceLevel` enum controls which tier makes decisions:

### Model Endpoints

| Model | Endpoint | Use |
|-------|----------|-----|
| nvidia_orchestrator-8b | LM Studio (localhost:1234) | FAST - immediate decisions |
| glm-4.7-flash | LM Studio (localhost:1234) | FLASH - fast local |
| ministral-3-14b-reasoning | LM Studio (localhost:1234) | MINI - local reasoning |
| glm-4.7 | Z.ai API | FULL - smartest, but remote |
| Devstral 2 | Mistral API (via llm-code-sdk) | Fast codegen |
| GPT-5.2 | OpenRouter | LOW OC feedback (cheaper) |
| Claude Opus 4.5 | OpenRouter | MED OC feedback |
| GPT-5.2-Pro | OpenRouter | HIGH OC feedback (selective) |

### AssuranceLevel Progression

```
FAST → FLASH → MINI → FULL → FULL+LOW_OC → FULL+MED_OC → FULL+HIGH_OC
 │       │       │      │         │            │             │
 8b    flash   mini   glm-4.7   +GPT-5.2     +Opus     +GPT-5.2-Pro
(local)(local)(local) (Z.ai)  (OpenRouter) (OpenRouter)  (OpenRouter)
```

* **FAST**: 8b decides immediately, all other models provide non-blocking feedback
* **FLASH**: flash decides, mini/full provide feedback
* **MINI**: ministral decides, full provides feedback
* **FULL**: glm-4.7 decides (smartest cheap tier, feedback from mini)
* **FULL+OC**: glm-4.7 decides, premium cloud models provide feedback

### SmartWrite Model Selection

For code generation tasks, select model tier based on edit complexity:
* Trivial (rename, comment) → FAST or Devstral 2
* Simple (add function) → FLASH
* Medium (refactor) → MINI
* Complex (multi-file, architectural) → FULL
* Critical (breaking changes) → FULL+OC

## Director Operating Procedures

When operating as Director on this codebase, follow these procedures:

### Tools (USE THESE, NOT curl/raw API)

```bash
# Issue management
pal call plane --input '{"verb": "list", "type": "issue", "project": "PAL"}'
pal call plane --input '{"verb": "create", "type": "issue", "project": "PAL", "name": "...", "description": "..."}'
pal call plane --input '{"verb": "update", "type": "issue", "project": "PAL", "id": "...", "state": "..."}'

# Cycles and Modules
pal call plane --input '{"verb": "list", "type": "cycle", "project": "PAL"}'
pal call plane --input '{"verb": "create", "type": "module", "project": "PAL", "name": "..."}'

# Code analysis
pal call smart_read --input '{"path": "crates/director/src/lib.rs", "layer": "ast"}'
pal call smart_write --input '{"path": "...", "operation": "replace_function", "target": "...", "content": "..."}'

# Zulip communication
pal call zulip --input '{"verb": "send", "stream": "palace", "topic": "director/tealc", "content": "..."}'
pal call zulip --input '{"verb": "messages", "stream": "palace", "topic": "...", "limit": 10}'
```

### Zulip Channel Structure

- **Stream**: `palace` (the project)
- **Topics**:
  - `director/<machine>` - Director daemon on that machine (e.g., `director/tealc`)
  - `palace/<machine>` - Palace daemon on that machine (e.g., `palace/tealc`)
  - `palace/PAL-<n>` - Work on specific issue (e.g., `palace/PAL-54`)

**Two separate bots:**
1. Palace Bot - runs Sessions, executes code tasks
2. Director Bot - manages project, coordinates work via Plane

### Machine Identity

When operating, identify by machine name, not "claude-code":
- This machine is `tealc`
- Topics: `director/tealc`, `palace/tealc`

### Communication Flow

1. Report status to Zulip topic `director/<machine>`
2. When working on an issue, use topic `palace/PAL-<n>`
3. Human can steer by tagging @palace in relevant topic
4. Surface questions via recursive survey system

### Before Acting

1. Check Plane for existing issues: `pal call plane --input '{"verb": "list", ...}'`
2. Don't create duplicate issues
3. Use JECJIT - minimal but valuable context
4. Document decisions in Zulip

## BANNED PATTERNS

**HEREDOCS ARE BANNED.** Do not use `<< EOF` or similar constructs to work around problems. Fix the actual issue.

**Working around broken tools is BANNED.** If a tool doesn't work, fix the tool. Don't find clever workarounds that hide the problem.

### EAT YOUR OWN DOGFOOD

**Use `pal call` extensively for CODE operations:**

- `pal call smart_read` for reading code files (AST analysis, symbols)
- `pal call smart_write` for editing code files (function replacement, etc.)
- `pal call plane` for all Plane.so operations
- `pal call zulip` for all Zulip operations

If you are an AI agent working on this codebase,
you MUST use the Palace tools for code, not bypass them.
This validates the tools work and surfaces bugs.

Note: Use normal Read/Edit for non-code files like markdown, config, etc.

## License
Palace is licensed under the GNU Affero General Public License v3.0.
