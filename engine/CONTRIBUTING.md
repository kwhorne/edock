# Contributing to eDock

Thanks for helping. Issues and pull requests are welcome.

## Before you start

For anything larger than a small fix, open an issue first so we can agree on
the approach, and wait for that before you write the code. eDock aims to be
small and fast: changes that add a dependency, a background process or
startup time need a good reason.

Bug reports need the eDock version and system, the exact commands you ran and
the output you got, as the issue form asks. A report we can't reproduce, or a
problem found only by reading the code, may be closed until it can be shown
on a real machine.

Keep at most 3 pull requests open at a time. Each one takes a maintainer's
time to review, and a few at a time get reviewed sooner. Pull requests beyond
that, or larger changes without an agreed issue, are closed; you're welcome
to reopen them one at a time later.

## Building and testing

```sh
cargo xtask host      # target/release/edock, signed with the virtualization entitlement
cargo test            # unit tests
tests/e2e.sh          # the Docker compatibility suite, against a running `edock daemon`
```

Use `cargo xtask host`, not plain `cargo build`, for a binary you run as the
daemon: without the virtualization entitlement the VM will not start. The
guest needs `cargo xtask kernel`, `cargo xtask disk` and `cargo xtask guest`
once; see docs/development.md.

`tests/e2e.sh` only removes containers, networks and volumes it created.

## Speed is a feature

eDock is small and fast on purpose, and every pull request is checked for it.
The Speed check builds main and your change, runs both engines side by side
on one machine and compares them: `docker run`, API round trips, the daemon's
memory, the system calls one container costs, and the binaries' size. It
fails when the change is slower than main past the limits in
`scripts/perf.py`; the table is in the check's summary. To run it yourself on
Linux, build both with `cargo xtask linux` and run
`scripts/perf-run.sh <main's build> <your build> <work folder>`.

## Pull requests

- Keep each pull request to one change, with a message that says what it does
  and why.
- Run `cargo fmt`, `cargo clippy` and the tests. After changing Rust
  dependencies or bundled tool versions, run `scripts/ci/notices.sh` and commit
  the updated NOTICE.
By submitting a contribution you agree it is licensed under Apache-2.0.
