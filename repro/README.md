# Repro: FST file with an empty time table panics `fst-reader`

`empty-time-table.fst` is a small (452 byte) FST file produced by this crate
(`fst-writer` @ ebc4b35, see `tests/write_read.rs::write_read_empty`) that
crashes readers built on `fst-reader` 0.10.2 / `wellen` 0.13.12.

## What is wrong with the file

The file contains a value change data section (`FST_BL_VCDATA_DYN_ALIAS2`) whose
trailing time table has **zero entries** (`number_of_items == 0`), because the
writer emitted a section even though `time_change()` was never called.

GTKWave's `fstapi.c` never produces such a section: on close, if simulation time
never advanced (`is_initial_time`), it mocks up a time change at time 0 and
clones the current values into it (`fstWriterClose`, fstapi.c:1870-1883); and if
a section header was already started but carries no value changes, it truncates
the file back to before the header instead of finishing the section. So a
zero-entry time table is out-of-spec in practice — but it is exactly what a
fuzzer or a buggy writer will hand a reader.

## The panic

`fst-reader` reads the time table and then indexes it unconditionally:

```rust
// fst-reader-0.10.2/src/reader.rs:388, HeaderReader::read_data
let is_first_section = table.is_empty();
if is_first_section && time_chain[0] > start_time {   // <-- time_chain may be empty
    table.push(start_time);
}
```

```
thread 'main' panicked at fst-reader-0.10.2/src/reader.rs:388:46:
index out of bounds: the len is 0 but the index is 0
   fst_reader::reader::HeaderReader<R>::read_data
   fst_reader::reader::HeaderReader<R>::read
   fst_reader::reader::FstReader<R>::open_internal
   wellen::fst::read_header
   wellen::simple::read
```

## Reproducing

```rust
// needs the `wellen` dev-dependency of this crate
wellen::simple::read("repro/empty-time-table.fst").unwrap();
```

This panics instead of returning an `Err`. Reading an untrusted file should
never panic, so this is worth reporting upstream (`fst-reader` should treat an
empty time chain as either an error or an empty section).


# Repro: FST file with no value change section at all

`no-value-changes.fst` is a 380 byte file produced by this crate
(`tests/write_read.rs::write_read_value_then_time_change_at_zero_writes_no_section`) from the
history "write one value, then `time_change(0)`, then finish". It contains a header, a hierarchy
and a geometry block, and **no value change section**.

## Why the writer produces it

The value written before the first time change only ever reaches the frame, and readers skip the
frame when the first time step is at the section start time (`fstapi.c:5066`), so the section would
hold no value change. GTKWave's `fstapi.c` never finalizes such a section either:
`fstWriterFlushContextPrivate` returns on `vchg_siz <= 1` (fstapi.c:1259) before re-tagging the
block. This crate matches that, see `tests/fstapi_read.rs::fstapi_diff_no_value_changes`.

The reference's own file for the same history is *worse formed*: its unfinalized section header is
left behind as a zero-length `FST_BL_SKIP` block that readers cannot walk past. This file simply
ends after the geometry block.

## The two reader failures

1. **`fstReaderOpen` rejects the file** (`fstapi` crate 0.0.2 reports `Err(ContextCreate)`). The
   open path requires a non-zero section count:

   ```c
   if((rc) && (xc->vc_section_count) && (xc->maxhandle) && (...)) { xc->do_rewind = 1; }
   else { fstReaderClose(xc); xc = NULL; }
   ```

   Newer GTKWave relaxed this to `else if (!rc)`, so it returns a usable context instead — the
   copy in `../libfstwriter` (`980036d`) already has the fix.

2. **`wellen` 0.13.12 panics** once signals are loaded. `wellen::simple::read` succeeds and reports
   an empty time table, but `load_signals` unwraps the first time table entry:

   ```rust
   // wellen-0.13.12/src/fst.rs:69
   let mut index_and_time = time_table.next().unwrap();   // <-- empty time table
   ```

   ```
   thread '...' panicked at wellen-0.13.12/src/fst.rs:69:52:
   called `Option::unwrap()` on a `None` value
   ```

   Same family as the empty-time-table panic above: reading a file should not panic. Worth the same
   upstream report.

## Reproducing

```rust
let mut wave = wellen::simple::read("repro/no-value-changes.fst").unwrap();
assert!(wave.time_table().is_empty());
wave.load_signals(&[wellen::SignalRef::from_index(0).unwrap()]); // panics
```

