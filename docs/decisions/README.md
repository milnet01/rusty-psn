# Decisions

One file per decision: `ADR-NNNN-<topic>.md`, numbered in order, **never
edited after it is accepted**. A decision that turns out wrong gets a new
ADR that supersedes the old one, and the old one stays as written —
that is the whole value, since the point is to record what was believed
at the time.

`documentation.md` owns what an ADR is. `workflow.md` § 4 owns when one
is written: a close call in design, meaning two defensible options with
different consequences. Not every choice — only the ones somebody would
otherwise re-argue in six months.

## The shape

```markdown
# ADR-0001: <the decision, as a statement>

- **Status:** Accepted | Superseded by ADR-NNNN
- **Date:** YYYY-MM-DD

## Context

What was true that forced a choice. The constraints, not the options.

## Decision

What was chosen, stated so someone can act on it.

## Consequences

What this closes off, what it costs, and what now has to be true.
Including the bad parts — an ADR that only lists benefits is a
advertisement, and nobody trusts it later.
```

**Deliberately no ADR-0001 shipped here.** A scaffolded project would
begin with a decision nobody made, about a practice it has not started.
The first ADR is the first real close call.
