# Project Rules for rusty-psn

## Where this project is

**State:** 4 — Queue checked (2026-10-12): every discovery sign is named by a roadmap item.
**In flight:** RPSN-0162, the fork's 1.0.0 release (prep listed in its roadmap notes; publishing waits for the owner's S10 check, RPSN-0095).

## How work is done here

- **`~/.claude/workflow.md`** — the states, the gates, and what "done" means. Read in place.
- **`~/.claude/standards/`** — how to write code, tests, commits, documents and releases. Read in place.
- **`docs/discovery.md`** — what this project is for, and the signs that it works.
- **`docs/design.md`** — the parts, what each may depend on, and where each sign lives.

The rules below are this project's own, layered on top of those.

## Workflow Rules

These rules apply to ALL work on this project, in every conversation.

### 1. Security First
- Every change MUST be reviewed for security vulnerabilities before being considered complete
- Check for: path traversal, command injection, unchecked user input, unsafe URL handling, OWASP top 10
- TLS certificate bypass (`danger_accept_invalid_certs`) must ONLY be scoped to Sony PSN domains
- All downloaded URLs must be validated against a whitelist of expected Sony domains
- All file paths must be validated to prevent writes outside the configured download directory
- Never store secrets in code without documenting why it's acceptable (e.g., the publicly known HMAC keys)
- Run `cargo audit` mentally against any new dependency before adding it - prefer well-maintained, widely-used crates

### 2. Research Before Acting
- Before implementing any feature or fix, thoroughly research the topic online first
- For PSN/Sony API work: verify URL formats, response structures, and behaviors against known working tools (PKGj, NPS Browser, VitaDB)
- For RPCS3 integration: verify file formats and paths against RPCS3's actual source code and documentation
- For Rust crate usage: check crate documentation, recent issues, and security advisories
- Document research findings in code comments or the STANDARDS.md when they reveal non-obvious domain knowledge

### 3. Plan Before Implementing
- For any non-trivial change (new feature, architectural change, multi-file refactor):
  1. **Research** - Gather all relevant information (see Rule 2)
  2. **Plan** - Create a clear implementation plan with specific files and functions to change
  3. **Review** - Review the plan for completeness, correctness, and security implications before writing code
  4. **Implement** - Execute the plan, writing code only after the plan is reviewed
- Small bug fixes and typo corrections can skip the plan step
- If the plan changes during implementation, update the plan first, then continue

### 4. Code Quality Standards
- All code must pass `rustfmt` (max_width = 130) and `clippy` without warnings
- Follow existing naming conventions documented in STANDARDS.md
- Prefer explicit error handling over `unwrap()` - every `unwrap()` must have a comment justifying why it cannot fail
- New code must include tests where feasible
- Do not introduce new technical debt without documenting it in ROADMAP.md

### 5. Reference Documents
- **STANDARDS.md** - Architecture, coding conventions, domain model, existing behavior
- **ROADMAP.md** - Feature roadmap with priorities and implementation details. Generated from the roadmap store: change it with `roadmap_log`, never by hand. IDs are `RPSN-NNNN`.
- Both documents live in the project root alongside the source code

### 6. No Hardcoded Paths
- All user-specific paths (RPCS3 location, game library, download directory) MUST be configurable via the app's settings, never hardcoded
- The app must auto-detect common paths where possible but always allow manual override
- Preferred download layout: `{Game Title}/{Serial}/{pkg files}` (configurable)
- No personal filesystem paths, usernames, or machine-specific info should appear in source code or committed documentation
