# Dependency holds

> **Purpose — so a dependency kept below its latest release is a recorded,
> retestable decision, never an accident.**

Every dependency tracks its latest release (`~/.claude/standards/dependencies.md`
§ 1), and § 1 says when one may be held back. Each hold gets one row here, with the
version that broke, so a later release can be retested and the hold lifted
(§ 3).

Floors ("needs Rust 1.95 or newer") are not holds and do not go here (§ 4).

| What | Held at | Broke at | What breaks | What would release it | Decided / last retested |
|---|---|---|---|---|---|
| — none — | | | | | |
