# eDock engine

The container engine behind eDock: a small, fast Docker-compatible runtime
for macOS (Apple silicon and Intel), written in Rust. It runs containers in
one small Linux VM (Apple's Virtualization.framework, a custom kernel and a
tiny PID 1 agent) and serves the Docker Engine API on a unix socket, so the
`docker` CLI, Compose and buildx work unchanged. On Linux it runs containers
straight on the host's kernel.

The eDock desktop app (`../desktop`) is the user interface. It talks to the
engine only through the Docker Engine API and runs the `edock` binary's
`daemon` and `setup` subcommands.

```sh
cargo xtask kernel && cargo xtask disk && cargo xtask guest && cargo xtask host
./target/release/edock daemon &        # ~/.edock/run/docker.sock
docker --host unix://$HOME/.edock/run/docker.sock run --rm alpine echo hello
```

- [Building and architecture](docs/development.md)
- [What works](docs/features.md)
- [Where this comes from](UPSTREAM.md)

## License

Apache-2.0 (see [LICENSE](LICENSE)). eDock engine is a fork of
[Kiri](https://github.com/libracoder/kiri); see [NOTICE](NOTICE) for
attribution and third-party notices. The guest Linux kernel is a separate
GPL-2.0-only work ([source offer](guest/kernel/SOURCE_OFFER.md)).
