# Contributing to dbKu

Thanks for helping. A few things keep the project easy to work on.

## Setup

```bash
pnpm install
pnpm tauri dev
```

UI-only work: `pnpm dev`, then open http://localhost:1420 in a browser. A demo backend
(`src/dev/mockBackend.ts`) answers the IPC calls.

## Before opening a pull request

```bash
pnpm check && pnpm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

- **Adding or changing an IPC command:** update `src/lib/api.ts` and `src/lib/types.ts`,
  and add the call to `src-tauri/tests/ipc.rs`. That test is what catches a renamed argument.
- **Touching the read-only guard** (`src-tauri/src/sql/guard.rs`): add the statement you're
  allowing or blocking to its tests. A change that lets a writing statement through will not
  be merged.
- **Driver changes:** run `tests/drivers.rs` against a real server (see the README) and say
  which engines and versions you tested in the PR.
- Keep UI logic that can be pure (no DOM, no IPC) in `src/lib/util/` with a vitest test.

## Style

- Rust: `cargo fmt`, clippy clean, no `unwrap()` outside tests.
- TypeScript/Svelte: Svelte 5 runes, strict TypeScript, design tokens from `src/app.css`
  instead of hard-coded colours.
- Commit messages: `feat: …`, `fix: …`, `docs: …`, `refactor: …`, `test: …`, `chore: …`.
