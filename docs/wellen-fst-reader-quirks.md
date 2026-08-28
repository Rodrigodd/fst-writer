# Reader behaviour we have to write for: `wellen` 0.13.12 / `fst-reader` 0.10.2

Notes collected while chasing the empty-time-table panic and the first-time-change divergence from
the reference implementation (`fstapi.c`, at
`../libfstwriter/integration_test/verilator_share/gtkwave/fstapi.c`; all `fstapi.c:NNN` references
in this repo point there). These behaviours are why the writer emits what it emits, so they are worth keeping
around even though they live in someone else's crate. Paths are relative to
`~/.cargo/registry/src/index.crates.io-*/fst-reader-0.10.2/`.

## 1. A section with an empty time table panics the reader

`HeaderReader::read_data` indexes the time chain with no length check (`src/reader.rs:388`):

```rust
let is_first_section = table.is_empty();
if is_first_section && time_chain[0] > start_time {   // <-- time_chain may be empty
    table.push(start_time);
}
```

A value change section whose time table has `number_of_items == 0` therefore aborts the process
(`index out of bounds: the len is 0 but the index is 0`) instead of producing an `Err`. Reading an
untrusted file should never panic, so this is worth reporting upstream.

Frozen repro: `repro/empty-time-table.fst`, details in `repro/README.md`. Our writer no longer
produces such files (`FstBodyWriter::flush` / `finish` in `src/writer.rs`), and `fstapi.c` never
did — see `fstWriterClose` (fstapi.c:1847), which either truncates the empty section header or mocks
up a time-zero step.

## 2. The reader invents a sample from the section start time

Same code, working case: for the **first** section, when `time_chain[0] > start_time`, the reader
pushes `start_time` into the time table as an extra entry. The leading `0` in `write_read_simple`'s
expected `[0, 1, 5, 7, 8]` does not come from us — we never write time 0 into the chain there; the
reader adds it.

## 3. The frame is read only in that same case

`DataReader::read`, `src/reader.rs:659`:

```rust
// only read frame if this is the first section and there is no other data for the start time
if is_first_section && (time_table.is_empty() || time_table[0] > start_time) {
    read_frame(...)?;
} else {
    skip_frame(...)?;
}
```

So the frame — the block holding "the values at the start of the section" — is silently dropped for
every section after the first, and for a first section whose first chain entry equals its start
time. Consequence for writers: a value left in the frame alone is only visible if the first chain
entry is *later* than the section start time. `fstWriterClose` duplicates the values as value
changes when simulation time never advanced ("mock up the changes as time zero ones"), which
`SignalBuffer::mock_initial_time_step` mirrors — but on every other path the reference accepts the
loss, and so do we. A value written before a first `time_change(0)` is therefore gone, and if it was
the only one the file ends up with no value change section at all
(`repro/no-value-changes.fst`).

## 4. `wellen` ignores the header start time

`wellen-0.13.12/src/fst.rs` never reads `Header::start_time`; the time table is rebuilt from the
sections. A wrong start time in the file header is therefore invisible through `wellen`, which is
why `tests/fstapi_read.rs` uses GTKWave's own reader (`fstapi::Reader::start_time`) to check it.
`fstapi.c` writes `firsttime` there on close (fstapi.c:2099).

Related field mix-up on our side: `src/io.rs:150` writes `FstInfo::start_time` into the header's
trailing `time_zero` field (`fst-reader/src/io.rs:440`), not into the start-time field, which is
hardcoded to 0 at `src/io.rs:138`.

## 5. Equal compressed/uncompressed length means "stored raw"

`read_zlib_compressed_bytes` (`src/io.rs:317`) treats `uncompressed_length == compressed_length` as
an uncompressed block. A writer must therefore only claim compression when it strictly shrinks the
data. See `docs/time-table-equal-compressed-length.md`.
