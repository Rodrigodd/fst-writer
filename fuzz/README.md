Crate for fuzzing `fst-writer` using [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz/).

## Usage

See [`cargo-fuzz` Usage](https://github.com/rust-fuzz/cargo-fuzz?tab=readme-ov-file#usage) for details, but in short run the following command in this directory:

```sh
cargo +nightly fuzz run fstapi_diff
```

Useful flags — everything after `--` goes to libFuzzer:

```sh
# stop after a minute, which is what we use as a smoke test
cargo +nightly fuzz run fstapi_diff -- -max_total_time=60

# allow larger inputs, i.e. longer histories and more variables
cargo +nightly fuzz run fstapi_diff -- -max_len=4096
```

## The target

`fstapi_diff` writes the same waveform with `fst-writer` and with the reference writer (GTKWave's
`fstapi`, through the [`fstapi`](https://crates.io/crates/fstapi) crate), then parses both files
with [`fst-reader`](https://crates.io/crates/fst-reader) and compares the time table, the hierarchy
and every value change. The input is a structured `Waveform` — a list of hierarchy items followed by
a list of operations — so it covers scopes, aliases, bit vectors of any width, reals, values written
before the first time change, repeated time stamps and `flush` calls.

Because the input is structured, a saved artifact is best read with `cargo fuzz fmt`, which prints
the deserialized `Waveform` instead of raw bytes:

```sh
cargo +nightly fuzz fmt fstapi_diff fuzz/artifacts/fstapi_diff/crash-<hash>
```

Two things the target deliberately does not generate, because they are known to fail for reasons
that would drown out everything else. Both are documented in `docs/session-handoff.md`:

- **Variable length signals** (`FstSignalType::bit_vec(0)`), which `fst-writer` cannot encode.
- **Flushes with nothing pending, or before the section holds more than one time step**, where the
  reference corrupts its own output or ignores the flush.

## Running many instances in parallel

libFuzzer runs several processes against one shared corpus directory, and each find propagates to
the others through it, so more instances genuinely help.

```sh
# one worker per core, on the corpus in fuzz/corpus/fstapi_diff
cargo +nightly fuzz run fstapi_diff -- -jobs=$(nproc) -workers=$(nproc) -max_total_time=3600
```

- `-workers=N` is how many processes run at once.
- `-jobs=N` is how many runs to start in total; once a worker exits (a crash, or `-max_total_time`),
  the next job starts. Setting both to the core count gives one long-lived worker per core.
- Each worker writes `fuzz-<n>.log` in the directory you invoked from, so watch progress with
  `tail -f fuzz-*.log`. Only the merged corpus and `fuzz/artifacts/` matter afterwards.

`-fork=N` is the more robust alternative for long runs: the driver supervises N child processes, so
a single out-of-memory or timeout kills only its child instead of the whole session.

```sh
cargo +nightly fuzz run fstapi_diff -- -fork=$(nproc) -ignore_crashes=1 -max_total_time=3600
```

With `-ignore_crashes=1` the run keeps going after a find; collect everything from
`fuzz/artifacts/fstapi_diff/` at the end. Drop that flag if you would rather stop at the first one.

Note `cargo fuzz`'s own `-j/--jobs` is unrelated — it sets the number of parallel *build* jobs, not
fuzzing processes.

After a long run, shrink the corpus before committing it, so later runs start faster:

```sh
cargo +nightly fuzz cmin fstapi_diff
```

And minimize an artifact before reporting it:

```sh
cargo +nightly fuzz tmin fstapi_diff fuzz/artifacts/fstapi_diff/crash-<hash>
```
