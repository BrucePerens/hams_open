# Data structures of the radioctl test support

### `TestDir` [@ANCHOR: radioctl:radioctl_testdir_test_dir]

**Purpose and location.** The one way a test in the radioctl workspace makes a scratch directory: a fresh, empty directory under the system temporary directory that is removed with everything in it when the value is dropped, also while a failing test unwinds. Defined in `daemons/radioctl/testdir.rs`, which each crate includes with `#[path = "../../../testdir.rs"]` (so the type is compiled into the test code of several crates; it is not a public API of any crate). `tools/check_test_temp_dirs.py` fails a new hand-built `temp_dir().join(` in test code, which is why tests use this type.

**Definition.** `pub struct TestDir(PathBuf)`, a tuple struct holding the directory path.

| Part | Meaning |
|---|---|
| field `.0` | The absolute path of the directory: `<temp dir>/radioctl_<tag>_<process id>_<n>`, where `tag` names the test and `n` is a per-process counter (an atomic), so two tests in one process, or two test processes, never share a directory. |

**Behaviour.** `TestDir::new(tag)` removes any leftover of that name, creates the directory (panicking with a clear message if it cannot) and returns the guard. `Deref<Target = Path>` and `AsRef<Path>` let it be used wherever a path is wanted. `Drop` removes the tree and ignores errors, so a cleanup problem never hides a test failure.

**Concurrency and safety.** The counter is a process-wide `AtomicUsize` (relaxed ordering is enough: it only has to be unique). The directory is created with the default permissions under the temporary directory, so tests must not put secrets in it that other local users could read. Not serialized; no versioning.

**Functions that touch it.** `TestDir::new` and the trait implementations above; used by the record, self-test, update and C-program tests.

**See also:** the `radioctl-station` record tests in `crates/radioctl-station/src/record.rs`, which build their working directory with it.
