# kronos_sdk (generated)

**DO NOT EDIT FILES IN THIS DIRECTORY.**

This crate is generated from `smithy/model/*.smithy` by `smithy-rs`. Every
file here is overwritten on each regeneration — any hand edits will be lost
silently.

## Updating the SDK

After changing anything under `smithy/model/`:

```bash
just smithy-build       # validates model, regenerates, syncs sdks/rust/
git diff -- sdks/rust   # review the resulting diff
git add smithy/ sdks/rust/
git commit              # commit model + generated SDK in the same PR
```

CI runs `just smithy-check` to fail builds where the committed SDK has
drifted from the model.

## Why is this committed?

Downstream Rust consumers (e.g. aarokya) depend on this crate via a Cargo
`git` dep pinned to a kronos commit/tag. Committing the generated output
means those consumers don't need the Smithy CLI, JVM, or Juspay's Maven
mirror to build.

This crate is excluded from the kronos workspace (`Cargo.toml` →
`[workspace] exclude`) because it targets a different MSRV (1.82) and
pulls a heavy AWS smithy runtime stack that the server crates don't need.
