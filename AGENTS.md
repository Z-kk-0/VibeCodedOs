# Notes for AI agents

You're working on **Hematite**, a from-scratch OS in Rust, built together with the project owner
to learn how operating systems work. Read, in this order:

1. [CONTRIBUTING.md](CONTRIBUTING.md) – setup, commands, workflow, conventions, gotchas.
2. [ARCHITECTURE.md](ARCHITECTURE.md) – how it works today and the roadmap (where to continue).
3. [DECISIONS.md](DECISIONS.md) – every design decision with its reasons. Don't silently undo one.

To find out where the last session stopped: `git log --oneline -10` and the roadmap in ARCHITECTURE.md.

## Rules from the owner

- **Never read, edit, stage or revert `README.md`.** It's the owner's file. Use path filters
  (`git add -A -- . ':!README.md'`) so it never ends up in a commit.
- **Big design choices are decided together with the owner.** Propose options with trade-offs and a
  recommendation, then wait. Small implementation choices: decide, then log them in DECISIONS.md.
- **Be creative.** Not a textbook clone – take the less common path when it's better or more
  interesting, and say why (DECISIONS ground rules).
- **No big external crates** that do the real work. Only `core`/`alloc` in kernel and loader.
- **Git workflow is yours:** branch per milestone, `cargo xtask test` green, fast-forward merge
  into `main`, push. No need to ask.
- **Stay handover-ready:** docs change in the same commit as the code they describe.
- **Teach.** The owner wants to understand what's built: explain concepts when presenting work.
  The owner writes in German – answer in German; code, comments and docs stay English.
