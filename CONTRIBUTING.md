# Contributing to memswap

Thanks for contributing to a portable, provenance-verified agent memory
interchange format. This project aims to become a real standard, so governance
matters as much as code.

## DCO sign-off (required)

Every commit **MUST** be signed off with the Developer Certificate of Origin.
This keeps a clean copyright trail so the project can be re-licensed or donated
to a foundation later without tracking down every contributor.

```sh
git commit -s -m "feat: ..."
```

By signing off you certify you have the right to contribute the change under
Apache-2.0 (see the DCO at https://developercertificate.org/).

## Small PRs welcome; spec changes go through RFC

- **Code** (CLI, adapters, core store): small PRs welcome. Keep them focused;
  add tests.
- **The spec** (`spec/SPEC.md`): the crown jewel. Any change to a MUST/SHOULD/MAY
  rule goes through an RFC.

## RFC process

1. Copy `rfcs/RFC-0000-template.md` to `rfcs/RFC-0001-<slug>.md`.
2. Fill in the problem, the proposed normative change, and a **backwards
   compatibility** section.
3. Open a PR with status `Draft`. The minimum review window is **7 calendar
   days** on GitHub Discussions before acceptance.
4. The maintainer has final call (benevolent-dictator model). Status lifecycle:
   `Draft → Proposed → Accepted → Final` (+ `Superseded`).

## Development

```sh
cargo build
cargo test          # golden + round-trip + tamper tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Ground rules

- **Never fabricate metadata on import.** If a source lacks a field, leave it
  absent — do not invent it.
- **Losslessness is a hard requirement.** Export → import → export must be
  byte-identical. A change that breaks round-trip is a regression, not a feature.
- **Secrets are forbidden** in memory bodies. The importer's redaction pass
  strips credentials; `mem scan` flags them.
- **Document decisions** in `adr/` so contributors see *why*.

## Security

memswap is a **security-sensitive spec** (provenance/verification). Report
vulnerabilities privately to `security@` before opening an issue. See
`SECURITY.md`.
