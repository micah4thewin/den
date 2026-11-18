# RAR fixtures

Nothing free can *write* a RAR, so these four small archives are copied from
the test data of the [`unrar`](https://crates.io/crates/unrar) crate (MIT or
Apache-2.0), which is what Play reads RAR with:

| File | What it exercises |
| --- | --- |
| `version.rar` | a plain RAR |
| `solid.rar` | a solid RAR |
| `crypted.rar` | an encrypted RAR; the password is `unrar` |
| `archive.part1.rar` | the first volume of a set, with the rest missing |

Every other format the intake tests use is built by the tests themselves.
