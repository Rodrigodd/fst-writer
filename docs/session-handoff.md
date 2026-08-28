# Session handoff: matching the reference FST writer

Notes for picking this work up on another machine. Covers how we have been working, what has been
fixed, and where things stand.

## Setup on the new machine

- This repo, branch `fix-bugs` (tracks `fork/fix-bugs`). All work described here
  is in the working tree.
- The reference implementation must be cloned next to this repo, at `../libfstwriter/`. The C
  source we consult is `../libfstwriter/integration_test/verilator_share/gtkwave/fstapi.c`
  (checked against commit `980036d`). Every `fstapi.c:NNN` reference in this repo's docs, tests and
  comments points at that file. It is read-only for us — we never modify it.
- `cargo test` builds the `fstapi` crate (a dev-dependency of this crate since this work), which
  compiles bundled C, so a C compiler is required.
- `tests/fstapi_read.rs::fstapi_diff_first_time_change` writes a file with the reference *writer*,
  which creates scratch files via `tmpfile()` (`fstapi.c:230`), i.e. it needs write access to
  `/tmp`. Without it the test fails with `Err(ContextCreate)`. The reference *reader* has no such
  requirement.

### Reference points used so far

| What | Where in `fstapi.c` |
| --- | --- |
| Scratch files via `tmpfile()` | 230 |
| Header start/end time fixup on close (writes `firsttime`) | 967, 2099 |
| Section header, begin time `is_initial_time ? firsttime : curtime` | 1163, 1188 |
| Time table: compress only when strictly smaller | 1669 |
| `fstWriterFlushContext` is lazy (only sets `flush_context_pending`, and only if `tchn_idx > 1`) | 1835 |
| `fstWriterClose`: truncate the empty section header | 1847, 1862 |
| `fstWriterClose`: mock up time-zero changes when time never advanced | 1870-1883 |
| `fstWriterGetFlushContextPending` | 2503, 2508 |
| `fstWriterEmitTimeChange`, `firsttime = vc_emitted ? 0 : tim` | 3107, 3124 |

## Methodology

The rules this work has followed. They are what keeps the changes defensible, so keep to them:

1. **Reproduce first.** Start from a failing `cargo test`, never from a hypothesis.
2. **Read the reader, not the guess.** The consumer is `wellen` 0.13.12 on top of `fst-reader`
   0.10.2 (sources in `~/.cargo/registry/src/*/`). Find the exact invariant that is violated and
   the line that violates it before touching the writer. Findings are collected in
   `docs/wellen-fst-reader-quirks.md`.
3. **`fstapi.c` is normative.** Before changing writer behaviour, locate the corresponding code in
   the reference and follow it. Cite function name and line in the code comment or doc, so the
   claim can be re-checked. When the reference and intuition disagree, the reference wins.
4. **Failing test first.** Write the test, run it, record the actual failure output, and only then
   implement. Two tests in the repo were added this way and their failure output is quoted in the
   commit discussion.
5. **Never bend an expectation to match our output.** If a differential against the reference
   fails, report the difference rather than adjusting the assertion.
6. **Use both readers.** `wellen` for time tables and signal values; `fstapi::Reader` for
   everything wellen ignores (notably the header start time). If only one reader can see a bug, say
   so in the test.
7. **Freeze artifacts.** A file that crashes a reader goes into `repro/` with a README, and stays
   byte-identical afterwards; the writer fix must not silently invalidate the artifact.
8. **Verify every change** with `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.

## Issues fixed

### 1. A value change section with an empty time table crashed readers

`finish()` always wrote a value change section, even with no time step, producing a time table with
`number_of_items == 0`. `fst-reader` indexes `time_chain[0]` unguarded (`reader.rs:388`) and
panics. The reference never emits such a section.

Fix (`src/writer.rs`): `flush()` is a no-op when nothing is buffered, and `finish()` writes no
section when everything was already flushed — the counterpart of the truncate branch in
`fstWriterClose` (`fstapi.c:1862`). Frozen artifact: `repro/empty-time-table.fst`, described in
`repro/README.md`; it still panics `fst-reader` and is the basis for an upstream report.

### 2. Values written without any time change were dropped

With no `time_change` call, the frame kept its initial all-`x` content while the real values sat in
`values`. The reference mocks up a time-zero step instead (`fstapi.c:1870-1883`: "simulation time
never advanced so mock up the changes as time zero ones", then clones every handle's value).

Fix (`src/buffer.rs`): `mock_initial_time_step`, called from `finish()` when the time never
advanced. It is now the literal reference sequence — one `time_change(0)` plus `clone_all_values()`
(`fstapi.c:1875-1882`) — and clones *every* handle, `x` values included, because that is what the
reference emits (see the `FST_REMOVE_DUPLICATE_VC` note below). Pinned by
`write_read_no_time_change`. Note this applies **only** when the time never advanced; a value
written before an explicit `time_change(0)` is not rescued, see issue #6.

### 3. `finish()` right after `flush()` wrote a redundant section

The first version of the mock triggered on "buffer empty", which also covers the
already-flushed case, and would have written a second section repeating the last timestamp. The
reference distinguishes the two by `is_initial_time`. Fix: gate the mock on
`SignalBuffer::is_initial_time()`. Pinned by `write_read_flush_then_finish`.

### 4. The first time change did not decide the start time

The main change of the last session. We treated `time_change(0)` as a no-op and always started the
first section (and the file header) at 0. The reference always records the first time change — even
at time 0, because `curtime` is forced to 0 and the varint is written outside the `skip` guard —
and starts the section at `firsttime = vc_emitted ? 0 : tim` (`fstapi.c:3107`, `3124`), writing
that same value into the header on close (`fstapi.c:2099`).

Symptom: for a first time change at t > 0 with no earlier value, `fst-reader` re-created a sample
from the section start time, so our files carried a phantom all-`x` sample at time 0 that a
reference-written file does not have.

Fix (`src/buffer.rs`, `src/io.rs`, `src/writer.rs`):
- new `vc_emitted` and `first_time` fields, mirroring the reference's own state;
- the first time change gets its own path in `time_change`, setting `start_time` when no value was
  written yet, and both first-time-step paths share the new `start_time_step` helper;
- `HeaderFinishInfo` carries `start_time` and `update_header` writes it.

Accepted consequence, decided explicitly: when the first time step lands on the section start time
the frame is skipped by readers, so a signal that was never assigned has no samples at all — byte
for byte what the reference produces. **Superseded by issue #6**: explicitly written values are not
rescued either.

### 6. Matching the reference around the first time step, in every case

Decision from the following session: the writer must produce what the reference produces even where
the reference throws data away. The previous behaviour cloned every signal's value out as real value
changes when a value had been written before a first `time_change(0)`, so that the skipped frame
would not swallow it. That deviation is gone.

`fstapi.c` reference points for the rules now implemented:

| Rule | Where |
| --- | --- |
| `signal_change` before the first time change only updates the frame | 2932-2935 |
| `firsttime = vc_emitted ? 0 : tim`, no cloning | 3124, 3146 |
| A section with no value change is never finalized (`vchg_siz <= 1`); its header stays `FST_BL_SKIP`, which readers treat like EOF | 1259, 1181, 4917 |
| Time never advanced: mock a step at 0 and clone every handle | 1870-1883 |
| Frame is only read for the first section, and only when `beg_tim != time_table[0]` | 5065-5066 |

**`FST_REMOVE_DUPLICATE_VC` is not defined in any build here** — not by the `fstapi` crate
(`build.rs` defines only `FST_WRITER_PARALLEL`), not by `libfstwriter`. So the dedup block at
`fstapi.c:2868-2924` is compiled out and `fstWriterEmitValueChange` never filters. Worth knowing
before reasoning about it: were the macro defined, an *unchanged* value at `curtime == 0` would be
emitted only when it is all `x` (2883-2896, 2912-2919) — the opposite of what one might guess.

Behaviour with signals `a` and `b` where only `a` is ever written, as seen by
`fstReaderIterBlocks2`/wellen. Our writer now agrees with the reference in every row:

| | sequence | result |
| --- | --- | --- |
| A | `a=1`; finish | `[0]`, a=1@0, b=x@0 (the mock) |
| B | `a=1`; `t=0`; finish | no value change section at all |
| B′ | `a=1`; `t=0`; `b=1`; finish | `[0]`, b=1@0 — a's value is lost with the frame |
| C | `t=0`; `a=1`; finish | `[0]`, a=1@0 |
| D | `a=1`; `t=5`; `b=1`; finish | `[0,5]`, a=1@0 and b=x@0 from the frame, b=1@5 |

Changes: `time_change`'s initial-time branch collapsed to the reference's one line and no longer
clones; `mock_initial_time_step` does its own cloning and no longer forces `signal_change_emitted`;
`SignalBuffer::is_empty` was replaced by `has_value_changes` (backed by a new
`SingleVecLists::is_empty`), and both `Writer::flush` and `Writer::finish` skip the section when it
holds no value change.

Cost, decided explicitly: rows B and "time changes only" produce a file with **zero** value change
sections, and neither reader copes with that shape — `fstReaderOpen` rejects it (the
`vc_section_count` check; newer GTKWave relaxed it) and `wellen` panics in `load_signals` on the
empty time table. The reference's own file for that history is worse formed (it leaves a zero-length
`FST_BL_SKIP` block). Frozen as `repro/no-value-changes.fst`; pinned as a differential by
`tests/fstapi_read.rs::fstapi_diff_no_value_changes`.

### 5. Refactor that came with it

`signal_change` and `mock_initial_time_step` shared ~12 lines of value-change encoding. That is now
one private helper, `append_value_change`, which reads the current value out of `values`.
`mock_initial_time_step` became the literal reference sequence: one `time_change` plus a clone loop.
Note that it cannot call `signal_change` for the clones — that path returns early when the value
is unchanged, which is exactly the case here.

### 7. Time table written as zlib when compression did not help

`write_time_table` (`src/io.rs`) used a strict `compressed.len() > time_table.len()` to decide
whether compression was worth it, so a zlib stream that came out *exactly* as long as the input was
written while recording `uncompressed_length == compressed_length` — which is precisely how readers
detect a raw block. They then decode deflate bytes as varint time deltas and fail. Fixed with `>=`,
matching the reference's strict-improvement rule (`fstapi.c:1669`).

Smallest trigger: eleven time steps one tick apart, i.e. eleven `0x01` deltas, which deflate to
exactly eleven bytes. Ten or twelve steps are fine. Pinned by
`write_read_time_table_equal_compressed_length`; full context in
`docs/time-table-equal-compressed-length.md`.

Found by the `fstapi_diff` fuzz target after it was switched from `wellen` to `fst-reader`: our file
failed to parse where the reference's did not.

### 8. Real valued signals started out as `x` instead of NaN

`SignalBuffer::new` filled the whole value buffer with `b'x'`, including the eight bytes of a real.
The reference initializes those to `strtod("NaN", NULL)` instead ("initialize doubles to NaN rather
than x", `fstapi.c:2605-2611`, value set at `fstapi.c:1133`). The symptom is a real signal whose
value before its first write reads back as `2.068428470140581e272` — eight `x` bytes reinterpreted
as a double — where the reference reports `NaN`. Fixed by seeding those slots with
`f64::NAN.to_le_bytes()`, using the new `FstSignalType::is_real`.

Found by the `fstapi_diff` fuzz target within a second of gaining real valued signals.

### 9. `flush()` cut sections the reference would not, losing time steps

`fstWriterFlushContext` drops the request outright unless `tchn_idx > 1` (`fstapi.c:1838`) — the
current section must already hold more than one time step past its first. `tchn_idx` is our
`time_table_index`, except that after a flush the reference re-records the closing time as the first
entry of the new chain (`fstapi.c:3140`) and counts it, so its index runs one ahead of ours in every
section but the first. Hence `SignalBuffer::can_flush`:

```rust
if self.first_buffer { self.time_table_index > 1 } else { self.time_table_index > 0 }
```

Without the gate we cut whenever there was anything to write, and the time steps that followed
landed in a section that might never receive a value change — which is not written out at all
(`fstapi.c:1259`), so they were lost with it. For `t=10, value; t=20, value; flush(); t=30` we
produced a time table of `[10, 20]` where the reference has `[10, 20, 30]`, while our own header
still said the file ended at 30.

Pinned by `write_read_flush_below_time_step_gate`, which compares against a reference-written file.
Two existing tests had to grow a third time step to keep exercising a flush at all:
`write_read_flush_then_finish` and `write_read_repeated_value_after_flush`. The flush in
`write_read_simple` is now held back, which changes nothing observable.

### 10. A `time_change` that did not advance the clock was collapsed

`fstWriterEmitTimeChange` never compares the new time to `curtime` (`fstapi.c:3143-3148`): it writes
the zero delta and advances `tchn_idx` like any other step. We returned early on
`Ordering::Equal`, which cost two things — the duplicate time table entry, and the `tchn_idx` count
that `SignalBuffer::can_flush` is built on (issue #9). Being one step behind there held back a flush
the reference honours, and the trailing time step then survived in our file and not in the
reference's.

Reduced by the fuzz target to: `t=0; t=37632; value; t=37632; flush; t=92449`. The reference records
`[0, 37632, 37632]`, we recorded `[0, 37632, 92449]`. Fixed by treating `Ordering::Equal` exactly
like `Greater`, which also means a `signal_change` after a flush plus a non-advancing time change no
longer reaches the `todo!()`. Pinned by `write_read_time_change_without_progress`, which asserts the
time table against a reference-written file with no dedup at all.

### 11. `flush()` now queues, as the reference's does

`fstWriterFlushContext` does not flush: it sets `flush_context_pending` (and only when
`tchn_idx > 1`), and the next `fstWriterEmitTimeChange` cuts the section, seeding the new time chain
with the time the old one closed at (`fstapi.c:1835-1842`, `3135-3140`). `Writer::flush` now does
the same — `SignalBuffer::request_flush` sets a flag, `take_pending_flush` is consulted at the top
of `Writer::time_change`, and `SignalBuffer::flush` opens the next section with the closing time.

Three things fell out of it:

- **The `todo!()` is gone.** A `signal_change` after a `flush` used to panic with "Currently we only
  support flushing right before a new time step", because the flush cut the section immediately and
  left no time step to attach to. A queued flush mutates nothing, so the value joins the still open
  section. The branch is now a `debug_assert!`, and `write_read_value_after_flush` pins it.
- **`can_flush` lost its second arm.** With the new section opening at the closing time, our
  `time_table_index` tracks `tchn_idx` in every section, not just the first, so the predicate is
  simply `time_table_index > 1`.
- **The fuzz target compares time tables exactly.** `dedup` is gone from
  `fuzz/fuzz_targets/fstapi_diff.rs`; 25k runs over five minutes and all 18 stored artifacts pass
  without it.

Deliberately not copied: when a queued flush finds nothing to write the reference still seeds the
new chain, corrupting every later time stamp in the section it did not close. We drop the request
and write nothing — see `docs/fstapi-flush-time-corruption.md` — which is the one remaining
difference in this area, and the reason the fuzz target only issues a flush when a value change is
pending.

`write_read_repeated_value_after_flush` grew a reference-written counterpart and now asserts the
duplicated boundary time stamp (`[10, 20, 145, 145, 290]`) rather than the single one it used to.

## Known issues, not fixed

1. **`FstInfo::start_time` is misleading.** It is written into the header's trailing `time_zero`
   field (`src/io.rs`), not into a start time; the section start time is derived from the first time
   change. Worth either renaming or wiring up.
2. **Upstream reports pending** for the two `fst-reader`/`wellen` panics; `repro/` is ready to
   attach.
3. **Variable length signals are broken.** `FstSignalType::bit_vec(0)` is reachable from the public
   API, but `signal_change` then only accepts an empty value (anything else hits the
   `expand_special_vector_cases` panic), and the section that comes out has no length prefix, so the
   reference reader walks off into uninitialized memory — it reported six bogus value changes with
   garbage contents and then glibc aborted the process with "corrupted size vs. prev_size". The
   reference encodes these with an explicit record length (`fstapi.c:1342-1367`). Either implement
   that or reject `bit_vec(0)` with an `FstWriteError`. Excluded from the fuzz target meanwhile.
4. **The reference corrupts its own time table on a no-op flush.** Not our bug, but the fuzz target
   has to avoid it. When a queued flush finds nothing to write, `fstWriterFlushContextPrivate`
   returns early on `vchg_siz <= 1` (`fstapi.c:1259`) so no new section starts, yet
   `fstWriterEmitTimeChange` still appends `curtime` to the *current* time chain
   (`fstapi.c:3136-3140`), where a reader decodes it as a delta. For `10, 20, 30, flush, 40` the
   reference reports the time table as `[10, 20, 30, 60, 70]` with the value at 60, while its own
   header says the file ends at 40; ours is the correct `[10, 20, 30, 40]`. The target only issues a
   flush when a value change is actually pending — and values written before the first time change
   do not count, since those reach the frame alone (`fstapi.c:2932-2935`). Written up for upstream in
   `docs/fstapi-flush-time-corruption.md`, with a self-contained C reproducer alongside it in
   `docs/flush_time_corruption.c`.
5. `fuzz/tests/adhoc.rs` (scratch differential harness) was deleted. The differential now lives as
   a permanent test in `tests/fstapi_read.rs`. The fuzz target `fuzz/fuzz_targets/fstapi_diff.rs`
   parses with `fst-reader` rather than `wellen`, which is how issue #7 was found.

## Current state

Everything below is green: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.

Tests and what each one pins:

| Test | Pins |
| --- | --- |
| `write_read_empty` | a file with no data at all is readable |
| `write_read_no_time_change` | values written without any time change survive at time 0 |
| `write_read_flush_then_finish` | `finish()` after `flush()` adds no redundant section |
| `write_read_first_time_change_not_zero` | first time change at t > 0 with no earlier value starts the file at t — no phantom time 0 sample |
| `write_read_first_time_change_at_zero` | first time change at 0 is recorded; a never-assigned signal has no data, as in a reference file |
| `write_read_value_before_time_change_at_zero` | a value written before a first `time_change(0)` is lost with the skipped frame, as in a reference file |
| `write_read_value_then_time_change_at_zero_writes_no_section` | that history alone produces no value change section |
| `write_read_only_time_changes_writes_no_section` | time changes without any value produce no section either |
| `write_read_simple` | the original end-to-end case, unchanged |
| `fstapi_read_first_time_change_not_zero` | header start time and earliest sample, seen by the reference reader (wellen cannot see the header field) |
| `fstapi_diff_first_time_change` | same history written by both writers is indistinguishable to the reference reader |
| `fstapi_diff_no_value_changes` | the two histories that produce no section are equally unreadable from both writers |
| `write_read_time_table_equal_compressed_length` | a time table whose zlib output is exactly as long as the input is stored raw |
| `write_read_time_change_without_progress` | a `time_change` that does not advance the clock is still a time step |
| `write_read_value_after_flush` | a value change may follow a `flush` with no time change in between |

The `fstapi_diff` fuzz target covers what the tests do not: scopes, aliases, bit vectors of any
width, reals, values before the first time change, repeated time stamps and `flush` calls, comparing
the time table — exactly, no dedup — the hierarchy and every value change against the reference.
See `fuzz/README.md`, including how to run many instances in parallel.
| `buffer::tests::*` | the three pre-existing unit tests for the value lists |

Documentation in the repo:

- `docs/wellen-fst-reader-quirks.md` — reader behaviour the writer has to satisfy.
- `docs/time-table-equal-compressed-length.md` — the time table compression fix (issue #7).
- `docs/fstapi-flush-time-corruption.md` — a bug in the reference, written up for an upstream
  report, with `docs/flush_time_corruption.c` as its reproducer.
- `docs/session-handoff.md` — this file.
- `repro/README.md` — the two frozen reader-crash artifacts.
- `explain.md` — how `frame` and `values` relate inside `SignalBuffer`.

Generated `tests/*.fst` files are gitignored; `repro/empty-time-table.fst` is deliberately tracked.
