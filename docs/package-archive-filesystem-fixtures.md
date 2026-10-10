# Package archive filesystem fixtures

Issue #1660 includes hostile filesystem inputs as well as malformed archive bytes.
The `package_archive::filesystem_tests` module exercises the public extraction and
verification entry points with real temporary filesystem entries:

| Fixture | Expected result and preservation evidence |
| --- | --- |
| Existing destination file or directory | `extraction_root_create_failed`; existing bytes and directory contents survive; a fresh destination still extracts and verifies |
| Destination symlink (Unix) | `extraction_root_create_failed`; link and external sentinel survive, with no extracted files in the external directory |
| Root symlink (Unix) | `tree_root_invalid`; its legitimate target verifies successfully |
| Intermediate directory symlink (Unix) | `tree_symlink_rejected` even when external contents match the manifest; link and external bytes survive; restoring the real directory verifies |
| Non-UTF-8 filename (Linux) | `tree_path_invalid`; offending bytes survive; removing the entry restores successful verification |
| Unix socket entry | `tree_entry_type_rejected`; removing the socket restores successful verification |

Run this bounded suite with:

```sh
cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib package_archive --locked
```

These fixtures cover static hostile state. They do not prove safety against
concurrent filesystem replacement, symlink ancestors above the supplied root,
crash/power-loss recovery, or every supported platform. Unix-specific fixtures are
compiled only on Unix, except the non-UTF-8 fixture, which is Linux-only because
APFS rejects creating that name. Windows execution is separate evidence. The socket fixture
needs an environment that permits creating a local Unix-domain listener.

This is regression coverage of existing behavior, not a new storage contract or a
claim that all of #1660 is complete. The existing leaf-symlink and no-follow FIFO
reader tests remain in the parent module. No production code or CI policy changes
are needed for these fixtures.
