# Fixed: time table written as zlib when compression did not help

Applied. The one-line change below is in `write_time_table` (`src/io.rs`) and is
pinned by `tests/write_read.rs::write_read_time_table_equal_compressed_length`.
It is an independent bug, unrelated to the empty-time-table / `fst-reader` panic
issue.

Found by `fuzz/fuzz_targets/fstapi_diff.rs` once it parsed with `fst-reader`
instead of `wellen`: our file failed to parse (`Io(UnexpectedEof)`) where the
reference's parsed fine.

## The patch

```diff
--- a/src/io.rs
+++ b/src/io.rs
@@ -530,7 +530,7 @@ fn write_time_table(
     let compressed = miniz_oxide::deflate::compress_to_vec_zlib(time_table, ZLIB_LEVEL);

     // is compression worth it?
-    if compressed.len() > time_table.len() {
+    if compressed.len() >= time_table.len() {
         // it is more space efficient to stick with the uncompressed version
         output.write_all(time_table)?;
         write_u64(output, time_table.len() as u64)?;
         write_u64(output, time_table.len() as u64)?;
     } else {
         output.write_all(compressed.as_slice())?;
         write_u64(output, time_table.len() as u64)?;
         write_u64(output, compressed.len() as u64)?;
     }
```

(`write_time_table` in `src/io.rs`, called at the end of
`write_value_change_section`.)

## Why the `>=` is correct

The three u64s at the end of a value change section are
`uncompressed_length`, `compressed_length`, `number_of_items`. Readers use
`uncompressed_length == compressed_length` as the *signal* that the block is
stored raw — `fst-reader` 0.10.2, `src/io.rs:317`:

```rust
let bytes = if uncompressed_length == compressed_length && allow_uncompressed {
    read_bytes(input, compressed_length as usize)?      // treated as raw bytes
} else {
    // ... expects a 0x78 zlib header and inflates
};
```

With the original strict `>`, the case `compressed.len() == time_table.len()`
took the *else* branch: it wrote the zlib stream but recorded
`uncompressed_len == compressed_len`. A reader then hands the raw deflate bytes
(`78 01 ...`) to the delta-decoding loop as if they were varint time deltas —
silently corrupted time stamps, no error. `fst-reader` additionally has
`debug_assert!(is_zlib, ...)`, so in a debug build the mismatch shows up as an
assertion instead.

GTKWave's `fstapi.c` uses the same strict-improvement rule and falls back to raw
whenever compression does not shrink the block (`fstapi.c:1669`, time table):

```c
int rc = compress2(dmem, &destlen, tmem, tlen, 9);
if ((rc == Z_OK) && (((fst_off_t)destlen) < tlen)) {
    fstFwrite(dmem, destlen, 1, xc->handle);   /* compressed */
} else {
    fstFwrite(tmem, tlen, 1, xc->handle);      /* raw, destlen = tlen */
    destlen = tlen;
}
```

i.e. "compressed" is only used when strictly smaller — the same thing `>=`
achieves on our side.

## How to hit it

It needs a time table whose zlib encoding comes out exactly as long as the
input. The exact-equality window is narrow, but it is not exotic — the smallest
case is **eleven time steps one tick apart**:

```
n=10 raw=10 zlib=11
n=11 raw=11 zlib=11   <- equal: reader treats the block as stored raw
n=12 raw=12 zlib=11
```

Eleven `0x01` deltas deflate to exactly eleven bytes, so a plain
`for t in 1..=11 { time_change(t); signal_change(..) }` produced a file that
neither `fst-reader` 0.10.2 nor 0.17.0 can parse. Ten or twelve steps are fine,
which is what made this look rare.

## Status

Applied, with `tests/write_read.rs::write_read_time_table_equal_compressed_length`
as the regression test.
