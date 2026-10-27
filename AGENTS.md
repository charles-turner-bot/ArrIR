# ArrIR development guide for agents

## Setup and checks

ArrIR is a Rust crate. Run these before proposing a change:

```bash
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

## Design

The design contract lives in `plans/design.md`, with the reasoning behind each decision
in the dated discussion notes beside it. Until it is merged, it is in PR #18. Read it
before changing an operation, a schema or a crate boundary, and follow its purity rules.
If a change contradicts the contract, say so and update `plans/` in the same PR.

Check the open PRs before starting design work. Several proposals may be in flight at
once, and a design written against `main` alone can contradict one that hasn't merged
yet.

## Code style

- Prefer named structs, explicit enums, ordinary `impl` blocks and readable tests.
- Avoid macro-generated type families, generic frameworks and clever lifetimes unless a
  concrete case needs them. The maintainer is not a Rust expert, and the code should be
  easy to read and extend.
- Each code PR explains the Rust concepts it uses, its one semantic decision, how to run
  it, and what is still missing.

## Writing

Keep prose in docs, PR descriptions and comments clear and concise. When writing starts
to get waffly, use the antislop skill from
[drunkrhin0/antislop](https://github.com/drunkrhin0/antislop) to tighten it. Install it
with `npx skills@latest add drunkrhin0/antislop`. Don't apply it to code, commit messages
or quoted text.

## GitHub interaction

- **Never impersonate anyone.** Don't write as the user, as a collaborator, or as any
  other person. No first person on their behalf ("I wrote", "I'm happy with"). Write in
  a neutral voice, and put questions for someone under a plain label such as
  "Question for Mike:".
- End every PR description and comment with
  `🤖 Generated with [Claude Code](https://claude.com/claude-code)`, or the equivalent
  line for the tool you are. The user adds their own header by hand.
- Don't post progress reports, status updates or explanatory comments on issues or PRs
  unless asked.
- Don't close, merge or reopen PRs, or push to other people's branches, unless asked.
- Commits include a co-authorship trailer for the tool, for example
  `Co-Authored-By: Claude <noreply@anthropic.com>`.
