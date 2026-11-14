# Guest kernel source offer

The eDock guest kernel is Linux under GPL-2.0-only. Whoever distributes an
eDock build must make the complete corresponding source for the bundled
kernel available alongside it (for example on the same download site as the
release, as `eDock-VERSION-kernel-source.tar.xz`).

That archive contains the exact upstream Linux release (`KVER` in
`guest/kernel/build.sh`, from kernel.org), all eDock patches
(`guest/kernel/patches`) already applied, the kernel configuration
(`guest/kernel/edock.config`) and the build inputs. It is reproducible from
this checkout with:

```sh
scripts/ci/kernel-source.sh target/eDock-VERSION-kernel-source.tar.xz
```

The guest kernel is a separate work from the Apache-2.0 eDock application.
