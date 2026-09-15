---
title: Build and release flow
date: 2026-09-14
---

Ran the full release matrix. Notes:

1. `cargo fmt --check` first.
2. clippy must be warning-free.
3. Tag `v*` triggers the release workflow.