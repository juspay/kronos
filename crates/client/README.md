# kronos_sdk (generated)

**DO NOT EDIT FILES IN THIS DIRECTORY.**

This crate is generated from `smithy/model/*.smithy` by `smithy-rs`. Every
file here is overwritten on each regeneration — any hand edits will be lost
silently.

## Updating the SDK

After changing anything under `smithy/model/`:

```bash
just smithy-build       # validates model, regenerates, syncs crates/client/
git diff -- crates/client   # review the resulting diff
git add smithy/ crates/client/
git commit              # commit model + generated SDK in the same PR
```

> **Note:** there is no CI guard for drift right now. smithy-rs codegen
> emits some `pub use` blocks in JVM HashMap iteration order (non-stable
> across processes), so a naive `git diff --exit-code` check is too noisy
> to enforce. A canonicalization step + drift check will be added in a
> follow-up. Until then, please regenerate before committing model changes.

## Why is this committed?

Downstream Rust consumers (e.g. aarokya) depend on this crate via a Cargo
`git` dep pinned to a kronos commit/tag. Committing the generated output
means those consumers don't need the Smithy CLI, JVM, or Juspay's Maven
mirror to build.

This crate is excluded from the kronos workspace (`Cargo.toml` →
`[workspace] exclude`) because it targets a different MSRV (1.82) and
pulls a heavy AWS smithy runtime stack that the server crates don't need.
