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
