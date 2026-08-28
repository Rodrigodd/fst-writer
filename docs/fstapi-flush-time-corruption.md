# `fstWriterFlushContext()` corrupts the time table when the queued flush has nothing to write

`fstWriterFlushContext()` does not flush; it sets `flush_context_pending` and lets the next
`fstWriterEmitTimeChange()` act on it. When that moment arrives, the time change unconditionally
appends `xc->curtime` to the time chain on the assumption that a new section has just been started.
If no value change was pending, `fstWriterFlushContextPrivate()` bails out and *no* new section is
started — but the extra chain entry is written anyway, into the section that is still current, where
a reader decodes it as a delta. Every time stamp after that point in the section is shifted by
`curtime`, and one spurious step appears.

The damage is contained in the writer: the file it produces is internally inconsistent, and
`fstapi`'s own reader reports value changes at times the header says the file does not reach.

## Reproducer

Self-contained: it writes a file and reads it back through `fstReaderIterBlocks2()`, so no external
tool is needed.

```c
/* Reproducer: fstWriterFlushContext() corrupts the time table when the queued
 * flush turns out to have nothing to write.
 *
 * Build (adjust the include/source paths to your gtkwave checkout):
 *   cc -I. flush_time_corruption.c fstapi.c fastlz.c lz4.c -lz -o repro && ./repro
 */
#include <inttypes.h>
#include <stdio.h>
#include "fstapi.h"

static void on_value_change(void *user, uint64_t time, fstHandle facidx,
                            const unsigned char *value)
{
    (void)user;
    printf("  value change: t=%" PRIu64 " handle=%u value=%s\n", time, facidx, value);
}

int main(void)
{
    const char *path = "flush_time_corruption.fst";

    void *w = fstWriterCreate(path, 1);
    if (!w) { fprintf(stderr, "fstWriterCreate failed\n"); return 1; }
    fstWriterSetTimescaleFromString(w, "1ns");
    fstHandle a = fstWriterCreateVar(w, FST_VT_VCD_REG, FST_VD_OUTPUT, 8, "a", 0);

    /* three time steps, so tchn_idx reaches 2 and the flush below is queued */
    fstWriterEmitTimeChange(w, 10);
    fstWriterEmitTimeChange(w, 20);
    fstWriterEmitTimeChange(w, 30);

    /* no value change has been emitted, so there is nothing for the flush to write */
    fstWriterFlushContext(w);

    fstWriterEmitTimeChange(w, 40);
    fstWriterEmitValueChange(w, a, "00000001");
    fstWriterClose(w);

    void *r = fstReaderOpen(path);
    if (!r) { fprintf(stderr, "fstReaderOpen failed\n"); return 1; }
    printf("header start_time=%" PRIu64 " end_time=%" PRIu64 "\n",
           fstReaderGetStartTime(r), fstReaderGetEndTime(r));
    fstReaderSetFacProcessMaskAll(r);
    fstReaderIterBlocks2(r, on_value_change, NULL, NULL, NULL);
    fstReaderClose(r);

    printf("expected: the only value change is at t=40\n");
    return 0;
}
```

Build it against a gtkwave source tree, e.g.

```sh
cc -w -I. flush_time_corruption.c fstapi.c fastlz.c lz4.c -lz -o repro && ./repro
```

Verified two ways: against gtkwave's current `fstapi.c` as written above, and against the older copy
bundled in the `fstapi` 0.0.3 crate, where two signatures differ — `fstWriterEmitValueChange()` takes
a trailing length, and the `fstReaderIterBlocks2()` callback takes a trailing `uint32_t len`. Both
print the same thing.

### Observed

```
header start_time=10 end_time=40
  value change: t=60 handle=1 value=00000001
expected: the only value change is at t=40
```

The single value change is reported at **t=60**, a time the header itself says the file never
reaches (`end_time=40`).

### Expected

```
header start_time=10 end_time=40
  value change: t=40 handle=1 value=00000001
```

### As seen by an independent reader

Reading the same file with the Rust [`fst-reader`](https://crates.io/crates/fst-reader) crate 0.10.2
(through `wellen` 0.13.12) gives the time table

```
[10, 20, 30, 60, 70]
```

where `[10, 20, 30, 40]` is correct: `30` is duplicated as `60` and the real last step `40` lands at
`70`. So the corruption is in the file, not in how one particular reader interprets it.

## Root cause

Three pieces of `fstapi.c`. Line numbers are given for two vendored copies, since the file moves
around: **A** = gtkwave's current `fstapi.c` (as vendored in `libfstwriter`,
`integration_test/verilator_share/gtkwave/fstapi.c`), **B** = the older copy bundled in the
[`fstapi` 0.0.3](https://crates.io/crates/fstapi) crate. The logic is identical in both, and the
reproducer was run against each.

**1. The request is queued, not performed** (A: 1835-1842, B: 1906-1914):

```c
void fstWriterFlushContext(fstWriterContext *xc)
{
    if (xc) {
        if (xc->tchn_idx > 1) {
            xc->flush_context_pending = 1;
        }
    }
}
```

**2. The private flush declines to do anything when the pending buffer is empty** (A: 1259,
B: 1318). `vchg_siz` is 1 when only the leading `'!'` is in the buffer, i.e. no value change has
been emitted since the section began:

```c
if ((xc->vchg_siz <= 1) || (xc->already_in_flush))
    return;
```

**3. `fstWriterEmitTimeChange()` acts on the pending flag and assumes it succeeded** (A: 3135-3140,
B: 3243-3249):

```c
if (fstWriterGetFlushContextPendingInternal(xc)) {
    xc->flush_context_pending = 0;
    fstWriterFlushContextPrivate(xc);   /* may have returned without doing anything */
    xc->tchn_cnt++;
    fstWriterVarint(xc->tchn_handle, xc->curtime);
}
```

The last two lines are only correct if a new section was started, because they seed the *new*
section's time chain with the time the old one closed at — the chain is delta encoded from zero, so
the first entry is an absolute time. When step 2 returned early there is no new chain: `curtime`
lands in the current one, where it is a delta.

Walking the reproducer, with the chain shown as the deltas actually written:

| call | `tchn_idx` | chain | decoded |
| --- | --- | --- | --- |
| `EmitTimeChange(10)` | 0 | `10` | 10 |
| `EmitTimeChange(20)` | 1 | `10, 10` | 10, 20 |
| `EmitTimeChange(30)` | 2 | `10, 10, 10` | 10, 20, 30 |
| `FlushContext()` | 2 | — | request queued, `tchn_idx > 1` holds |
| `EmitTimeChange(40)` | 3 | `10, 10, 10, `**`30`**`, 10` | 10, 20, 30, **60**, **70** |

The `30` is the injected `curtime`; the following `10` is the genuine `40 - 30` delta, now applied
on top of a base that is 30 too large.

Note that `fstWriterGetFlushContextPendingInternal()` (A: 2503, B: nearby) is

```c
return (xc->vchg_siz >= xc->fst_break_size) || (xc->flush_context_pending);
```

so the same block also runs for the automatic size-triggered flush. That path is safe: it is reached
because the buffer is *large*, which implies `vchg_siz > 1`, so step 2 always does its work. Only the
explicit `fstWriterFlushContext()` route can reach step 3 with an empty buffer.

## Why it is not seen more often

`fstWriterFlushContext()` drops the request unless `tchn_idx > 1`, i.e. unless the current section
already holds more than one time step past its first. A caller that flushes eagerly on every step
therefore has most of its requests discarded, and a caller that flushes after emitting values has a
non-empty buffer, so step 2 succeeds and the extra chain entry is correct. It needs a flush that is
both accepted (three or more time steps in the section) and pointless (no value change pending) —
which is exactly what a writer does when it flushes on a timer, or on a period of the simulation
where nothing changed.

## Suggested fix

Do nothing when there is nothing to flush:

```diff
--- a/fstapi.c
+++ b/fstapi.c
@@ fstWriterEmitTimeChange
             if (fstWriterGetFlushContextPendingInternal(xc)) {
                 xc->flush_context_pending = 0;
-                fstWriterFlushContextPrivate(xc);
-                xc->tchn_cnt++;
-                fstWriterVarint(xc->tchn_handle, xc->curtime);
+                if (xc->vchg_siz > 1) {
+                    fstWriterFlushContextPrivate(xc);
+                    xc->tchn_cnt++;
+                    fstWriterVarint(xc->tchn_handle, xc->curtime);
+                }
             }
```

The condition duplicates the guard inside `fstWriterFlushContextPrivate()`; having that function
report whether it flushed would be tidier, and would also cover the `already_in_flush` case:

```c
if (fstWriterFlushContextPrivate(xc)) {   /* nonzero when a section was written */
    xc->tchn_cnt++;
    fstWriterVarint(xc->tchn_handle, xc->curtime);
}
```

One design question for whoever fixes it: whether a request that cannot be honoured should be
dropped, as `xc->flush_context_pending = 0` above does, or stay queued until a value change gives it
something to write. Dropping it is the smaller change and matches the current behaviour of
`fstWriterFlushContext()` itself, which discards a request outright when `tchn_idx <= 1`.

## How this was found

By differential fuzzing: the same waveform written with `fstapi` and with a third-party FST writer,
both files then parsed and compared. The reference's time table diverged by exactly one duplicated,
doubled time stamp, which led back to the code above. The header/`end_time` contradiction in the
reproducer's output is a self-contained confirmation that needs no second writer.
