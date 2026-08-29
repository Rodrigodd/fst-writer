// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>
//
// write FST files with fst-writer and read them again with the wellen library
// (using fst-native as the backend)

use fst_writer::*;
use wellen::{SignalRef, Time};

#[test]
fn write_read_empty() {
    let filename = "tests/empty.fst";
    let version = "test 0.2.3";
    let date = "2034-10-10";

    ///////// write
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: version.to_string(),
        date: date.to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();

    let _var = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let writer = writer.finish().unwrap();

    // writer.time_change(0).unwrap();
    // writer.signal_change(var, b"0").unwrap();

    writer.finish().unwrap();

    drop(wellen::simple::read(filename).unwrap());
}

/// The reference implementation mocks up a time zero step if the time never advances,
/// instead of dropping the values (see `fstWriterClose` in `fstapi.c`).
#[test]
fn write_read_no_time_change() {
    let filename = "tests/no_time_change.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let b = writer
        .var(
            "b",
            FstSignalType::bit_vec(16),
            FstVarType::Port,
            FstVarDirection::Input,
            None,
        )
        .unwrap();
    // c never receives a value and thus stays at its default
    let _c = writer
        .var(
            "c",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // values are provided, but the time never advances
    writer.signal_change(a, b"1").unwrap();
    writer.signal_change(b, b"1010101010101010").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0]);

    // a, b and c are the first three signals in the file
    let refs = (0..3)
        .map(|ii| SignalRef::from_index(ii).unwrap())
        .collect::<Vec<_>>();
    wave.load_signals(&refs);
    let values = refs
        .iter()
        .map(|r| signal_values_to_string(wave.get_signal(*r).unwrap(), wave.time_table()))
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        ["(0: 1)", "(0: 1010101010101010)", "(0: x)"],
        "the values written before the first time change need to be preserved"
    );
}

/// Calling `finish` right after a `flush` must not write out a second, redundant section.
///
/// The history has to reach three time steps before the flush, or the flush is held back outright
/// (`SignalBuffer::can_flush`, mirroring `fstapi.c:1838`) and there is nothing to test.
#[test]
fn write_read_flush_then_finish() {
    let filename = "tests/flush_then_finish.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(2).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.time_change(3).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.flush().unwrap();
    // no more value changes, and flushing again should be a no-op
    writer.flush().unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0, 1, 2, 3]);
    let a_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[a_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table()),
        "(0: 0), (1: 1), (2: 0), (3: 1)"
    );
}

/// If no value is written before the first time change, the first value change section starts at
/// that first time (`firsttime = vc_emitted ? 0 : tim` in `fstWriterEmitTimeChange`, `fstapi.c`).
/// Otherwise readers re-create a time 0 entry from the section start time (`fst-reader`:
/// `if is_first_section && time_chain[0] > start_time`), which would show up as an extra all-`x`
/// sample that a file written by `fstapi.c` does not have.
#[test]
fn write_read_first_time_change_not_zero() {
    let filename = "tests/first_time_change_not_zero.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // no values before the first time change, and the first time change is not at 0
    writer.time_change(10).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(20).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    let a_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[a_ref]);
    let values = signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table());

    // this is what a file written by `fstapi.c` looks like
    assert_eq!(
        wave.time_table(),
        [10, 20],
        "no time 0 entry expected, values are: {values}"
    );
    assert_eq!(values, "(10: 1), (20: 0)");
}

/// A first time change at 0 is recorded in the time table, just like in `fstapi.c`, which means
/// that the frame is not read back by readers (`fst-reader` only reads it if the first time table
/// entry is greater than the start time of the section). Signals without a value change thus have
/// no data at all, which is also what a file written by `fstapi.c` looks like.
#[test]
fn write_read_first_time_change_at_zero() {
    let filename = "tests/first_time_change_at_zero.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    // b never receives a value
    let _b = writer
        .var(
            "b",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // the first time change is at 0, and no value is written before it
    writer.time_change(0).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0, 1]);
    let (a_ref, b_ref) = (
        SignalRef::from_index(0).unwrap(),
        SignalRef::from_index(1).unwrap(),
    );
    wave.load_signals(&[a_ref, b_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table()),
        "(0: 1), (1: 0)"
    );
    assert_eq!(
        wave.get_signal(b_ref).unwrap().get_first_time_idx(),
        None,
        "a signal without any value change has no data"
    );
}

/// Values written before a first time change at 0 are lost, and that is what the reference does
/// too: they only ever reach the frame (`fstWriterEmitValueChange` takes the `curval_mem`-only path
/// while `is_initial_time`, `fstapi.c:2932-2935`), and readers skip the frame when the first time
/// step is at the start time of the section (`fstapi.c:5066`; `fst-reader` only reads it if
/// `time_table[0] > start_time`). A signal that never received a value has no data either.
#[test]
fn write_read_value_before_time_change_at_zero() {
    let filename = "tests/value_before_time_change_at_zero.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let b = writer
        .var(
            "b",
            FstSignalType::bit_vec(16),
            FstVarType::Port,
            FstVarDirection::Input,
            None,
        )
        .unwrap();
    // c never receives a value
    let _c = writer
        .var(
            "c",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // initial values, followed by a first time change at 0
    writer.signal_change(a, b"1").unwrap();
    writer.signal_change(b, b"1010101010101010").unwrap();
    writer.time_change(0).unwrap();
    writer.time_change(5).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0, 5]);
    let refs = (0..3)
        .map(|ii| SignalRef::from_index(ii).unwrap())
        .collect::<Vec<_>>();
    wave.load_signals(&refs);
    // only the change written after the first time step survives; the `1` written before it went
    // into the skipped frame
    assert_eq!(
        signal_values_to_string(wave.get_signal(refs[0]).unwrap(), wave.time_table()),
        "(5: 0)"
    );
    for (r, name) in refs[1..].iter().zip(["b", "c"]) {
        assert_eq!(
            wave.get_signal(*r).unwrap().get_first_time_idx(),
            None,
            "{name} has no value change of its own, so it has no data"
        );
    }
}

/// A section that holds no value change at all is never written out: the reference bails out of
/// `fstWriterFlushContextPrivate` on `vchg_siz <= 1` (`fstapi.c:1259`) before it re-tags the block,
/// leaving the header tagged `FST_BL_SKIP`, which readers treat like EOF (`fstapi.c:4917`).
/// Here the only value written lands in the frame, which is then skipped, so nothing is left.
///
/// A file with no value change section is barely usable — `fstReaderOpen` rejects it outright and
/// wellen panics as soon as signals are loaded — but it is what the reference produces, see
/// `fstapi_diff_no_value_changes` in `tests/fstapi_read.rs` and `repro/no-value-changes.fst`.
#[test]
fn write_read_value_then_time_change_at_zero_writes_no_section() {
    let filename = "tests/value_then_time_change_at_zero.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(0).unwrap();
    writer.finish().unwrap();

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    assert!(
        wave.time_table().is_empty(),
        "no value change section was written: {:?}",
        wave.time_table()
    );
    // no `load_signals` here: wellen 0.13.12 panics on an empty time table
    // (`wellen-0.13.12/src/fst.rs:69`), see `repro/no-value-changes.fst`
}

/// Time changes on their own do not make a section either (`fstapi.c:1259`, see above). The time
/// steps are simply absent from the file, while the header still records the end time, like
/// `fstWriterClose` writing out `curtime` (`fstapi.c:2099`).
#[test]
fn write_read_only_time_changes_writes_no_section() {
    let filename = "tests/only_time_changes.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let _a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    writer.time_change(0).unwrap();
    writer.time_change(5).unwrap();
    writer.finish().unwrap();

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    assert!(
        wave.time_table().is_empty(),
        "no value change section was written: {:?}",
        wave.time_table()
    );
    // no `load_signals` here: wellen 0.13.12 panics on an empty time table
    // (`wellen-0.13.12/src/fst.rs:69`), see `repro/no-value-changes.fst`
}

/// A time table whose zlib encoding comes out exactly as long as the input must be stored raw.
/// Readers use `uncompressed_length == compressed_length` as the signal that the block is stored
/// raw (`fst-reader` 0.10.2, `src/io.rs:317`), so writing the zlib stream while recording equal
/// lengths makes them decode deflate bytes as varint time deltas. The reference only uses the
/// compressed form when it is strictly smaller (`fstapi.c:1669`).
///
/// Eleven time steps one tick apart is the smallest history that hits it: the time table is eleven
/// `0x01` deltas, and zlib turns those into exactly eleven bytes. See
/// `docs/time-table-equal-compressed-length.md`.
#[test]
fn write_read_time_table_equal_compressed_length() {
    let filename = "tests/time_table_equal_compressed_length.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    for time in 1..=11u64 {
        writer.time_change(time).unwrap();
        writer
            .signal_change(a, if time % 2 == 0 { b"0" } else { b"1" })
            .unwrap();
    }
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    let a_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[a_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table()),
        "(1: 1), (2: 0), (3: 1), (4: 0), (5: 1), (6: 0), (7: 1), (8: 0), (9: 1), (10: 0), (11: 1)"
    );
}

/// Writing the same value a signal already holds still has to be recorded. The reference only
/// removes duplicate value changes under `FST_REMOVE_DUPLICATE_VC` (`fstapi.c:2868-2924`), which no
/// build here defines, so `fstWriterEmitValueChange` always records the write.
///
/// Suppressing it cost more than the value change: the section left behind holds only a time step,
/// and a section without any value change is not written out at all (`fstapi.c:1259`), so the second
/// time step vanished with it and the time table came back as `[145]`.
///
/// Also covers the section boundary a queued flush produces, which repeats the time the closed
/// section ended at.
#[test]
fn write_read_repeated_value_after_flush() {
    let filename = "tests/repeated_value_after_flush.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // three time steps first, so the flush below is not held back by `SignalBuffer::can_flush`
    writer.time_change(10).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(20).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.time_change(145).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.flush().unwrap();
    // the same value again, in a section of its own
    writer.time_change(290).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.finish().unwrap();

    //// the same history through the reference writer
    let reference_filename = "tests/repeated_value_after_flush_fstapi.fst";
    let mut reference = fstapi::Writer::create(reference_filename, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let ref_a = reference
        .create_var(
            fstapi::var_type::VCD_REG,
            fstapi::var_dir::OUTPUT,
            1,
            "a",
            None,
        )
        .unwrap();
    for (time, value) in [(10u64, b"1"), (20, b"0"), (145, b"1")] {
        reference.emit_time_change(time).unwrap();
        reference.emit_value_change(ref_a, value).unwrap();
    }
    reference.flush();
    reference.emit_time_change(290).unwrap();
    reference.emit_value_change(ref_a, b"1").unwrap();
    drop(reference);

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    // The flush is acted on at the next time change, which opens the new section with the time the
    // old one closed at, so 145 goes into the time table twice. `wellen` collapses the repeat on
    // read, so the duplicate itself is only pinned by the comparison against the reference below.
    assert_eq!(
        wave.time_table(),
        [10, 20, 145, 290],
        "the second time step survives"
    );
    let reference_wave = wellen::simple::read(reference_filename).unwrap();
    assert_eq!(
        wave.time_table(),
        reference_wave.time_table(),
        "the section boundary has to fall exactly where the reference puts it"
    );

    // `wellen` collapses a value change that repeats the value a signal already holds, so the
    // change itself is only visible through the reference reader.
    let mut reader = fstapi::Reader::open(filename).unwrap();
    reader.set_mask_all();
    let mut changes = vec![];
    reader
        .for_each_block(|time, _handle, value, _var_len| {
            changes.push((time, String::from_utf8_lossy(value).to_string()))
        })
        .unwrap();
    assert_eq!(
        changes,
        [
            (10, "1".to_string()),
            (20, "0".to_string()),
            (145, "1".to_string()),
            (290, "1".to_string())
        ]
    );
}

/// A flush is held back until the section holds more than one time step past its first, mirroring
/// `tchn_idx > 1` in `fstWriterFlushContext` (`fstapi.c:1838`), where a request below that is
/// dropped outright rather than deferred.
///
/// Cutting the section anyway strands whatever follows: the time step at 30 lands in a section that
/// never receives a value change, and such a section is not written out (`fstapi.c:1259`), so the
/// time step disappears with it and the time table comes back as `[10, 20]`.
#[test]
fn write_read_flush_below_time_step_gate() {
    let filename = "tests/flush_below_time_step_gate.fst";
    let reference_filename = "tests/flush_below_time_step_gate_fstapi.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(8),
            FstVarType::Reg,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    // only two time steps when the flush comes in, so it must not cut the section
    writer.time_change(10).unwrap();
    writer.signal_change(a, b"00000001").unwrap();
    writer.time_change(20).unwrap();
    writer.signal_change(a, b"00000010").unwrap();
    writer.flush().unwrap();
    // no value change follows, so a section cut above would take this time step down with it
    writer.time_change(30).unwrap();
    writer.finish().unwrap();

    //// the same history through the reference writer
    let mut reference = fstapi::Writer::create(reference_filename, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let a = reference
        .create_var(
            fstapi::var_type::VCD_REG,
            fstapi::var_dir::OUTPUT,
            8,
            "a",
            None,
        )
        .unwrap();
    reference.emit_time_change(10).unwrap();
    reference.emit_value_change(a, b"00000001").unwrap();
    reference.emit_time_change(20).unwrap();
    reference.emit_value_change(a, b"00000010").unwrap();
    reference.flush();
    reference.emit_time_change(30).unwrap();
    drop(reference);

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [10, 20, 30]);
    let reference_wave = wellen::simple::read(reference_filename).unwrap();
    assert_eq!(
        wave.time_table(),
        reference_wave.time_table(),
        "the reference drops the flush request here, so nothing may be lost"
    );
}

/// A `time_change` that does not advance the clock is still a time step. The reference never
/// compares the new time to `curtime` (`fstWriterEmitTimeChange`, `fstapi.c:3143-3148`): it writes
/// the zero delta and advances `tchn_idx`. Collapsing it left our time table an entry short, and
/// left `tchn_idx` behind, so a flush the reference honours was held back here — after which the
/// trailing time step at 92449 survived in our file and not in the reference's.
///
/// This is the history the `fstapi_diff` fuzz target reduced it to.
#[test]
fn write_read_time_change_without_progress() {
    let filename = "tests/time_change_without_progress.fst";
    let reference_filename = "tests/time_change_without_progress_fstapi.fst";
    let value = f64::from_bits(3315782124548).to_le_bytes();
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let r = writer
        .var(
            "r",
            FstSignalType::real(),
            FstVarType::Real,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    writer.time_change(0).unwrap();
    writer.time_change(37632).unwrap();
    writer.signal_change(r, &value).unwrap();
    // does not advance the clock, but still counts as a step
    writer.time_change(37632).unwrap();
    writer.flush().unwrap();
    writer.time_change(92449).unwrap();
    writer.finish().unwrap();

    //// the same history through the reference writer
    let mut reference = fstapi::Writer::create(reference_filename, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let r = reference
        .create_var(
            fstapi::var_type::VCD_REAL,
            fstapi::var_dir::OUTPUT,
            8,
            "r",
            None,
        )
        .unwrap();
    reference.emit_time_change(0).unwrap();
    reference.emit_time_change(37632).unwrap();
    reference.emit_value_change(r, &value).unwrap();
    reference.emit_time_change(37632).unwrap();
    reference.flush();
    reference.emit_time_change(92449).unwrap();
    drop(reference);

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    // 37632 goes into the time table twice, and the flush is honoured, so the trailing 92449 goes
    // down with the section that never receives a value change. `wellen` collapses the repeated
    // time stamp on read, so the comparison against the reference below is what pins it.
    assert_eq!(wave.time_table(), [0, 37632]);
    let reference_wave = wellen::simple::read(reference_filename).unwrap();
    assert_eq!(
        wave.time_table(),
        reference_wave.time_table(),
        "the repeated time stamp has to be recorded, exactly as the reference records it"
    );
}

/// A value change may follow a `flush` with no time change in between. It used to reach a
/// `todo!()`, because the flush cut the section immediately and left no time step for the value to
/// attach to. Now the flush is only queued, so the value joins the still open section and the cut
/// happens at the next time change — or, as here, never.
#[test]
fn write_read_value_after_flush() {
    let filename = "tests/value_after_flush.fst";
    let reference_filename = "tests/value_after_flush_fstapi.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(8),
            FstVarType::Reg,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    for time in [10u64, 20, 30] {
        writer.time_change(time).unwrap();
        writer.signal_change(a, b"00000001").unwrap();
    }
    // enough time steps for the reference to queue the flush
    writer.flush().unwrap();
    writer.signal_change(a, b"00000010").unwrap();
    writer.finish().unwrap();

    //// the same history through the reference writer
    let mut reference = fstapi::Writer::create(reference_filename, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let ref_a = reference
        .create_var(
            fstapi::var_type::VCD_REG,
            fstapi::var_dir::OUTPUT,
            8,
            "a",
            None,
        )
        .unwrap();
    for time in [10u64, 20, 30] {
        reference.emit_time_change(time).unwrap();
        reference.emit_value_change(ref_a, b"00000001").unwrap();
    }
    reference.flush();
    reference.emit_value_change(ref_a, b"00000010").unwrap();
    drop(reference);

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [10, 20, 30]);
    let reference_wave = wellen::simple::read(reference_filename).unwrap();
    assert_eq!(wave.time_table(), reference_wave.time_table());

    // the value written after the flush belongs to the last time step
    let read = |f: &str| {
        let mut reader = fstapi::Reader::open(f).unwrap();
        reader.set_mask_all();
        let mut changes = vec![];
        reader
            .for_each_block(|time, _handle, value, _var_len| {
                changes.push((time, String::from_utf8_lossy(value).to_string()))
            })
            .unwrap();
        changes
    };
    assert_eq!(
        *read(filename).last().unwrap(),
        (30, "00000010".to_string())
    );
    assert_eq!(read(filename), read(reference_filename));
}

#[test]
fn write_read_simple() {
    let filename = "tests/simple.fst";
    let version = "test 0.2.3";
    let date = "2034-10-10";

    ///////// write
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: version.to_string(),
        date: date.to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    writer
        .scope("simple", "Simple", FstScopeType::Module)
        .unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let b = writer
        .var(
            "b",
            FstSignalType::bit_vec(16),
            FstVarType::Port,
            FstVarDirection::Input,
            None,
        )
        .unwrap();
    let _ = writer
        .var(
            "a_alias",
            FstSignalType::bit_vec(1),
            FstVarType::Port,
            FstVarDirection::Output,
            Some(a),
        )
        .unwrap();
    writer.up_scope().unwrap();

    let mut writer = writer.finish().unwrap();
    // provide an initial value for a
    writer.signal_change(a, b"0").unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.signal_change(b, b"1010101010101010").unwrap();
    writer.time_change(5).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.signal_change(b, b"101010XX10101010").unwrap();

    // held back by `SignalBuffer::can_flush`: the section has two time steps so far, one short of
    // what the reference acts on (`fstapi.c:1838`)
    writer.flush().unwrap();

    writer.time_change(7).unwrap();
    writer.signal_change(a, b"X").unwrap();
    writer.signal_change(b, b"0").unwrap();

    writer.time_change(8).unwrap();
    writer.signal_change(a, b"Z").unwrap();

    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();

    // time table
    assert_eq!(wave.time_table(), [0, 1, 5, 7, 8]);

    // hierarchy
    assert_eq!(wave.hierarchy().date(), date);
    assert_eq!(wave.hierarchy().version(), version);
    {
        let h = wave.hierarchy();
        let top = h.first_scope().unwrap();
        assert_eq!(top.full_name(h), "simple");
        let vars = top.vars(h).map(|r| &h[r]).collect::<Vec<_>>();
        let var_names = vars.iter().map(|v| v.full_name(h)).collect::<Vec<_>>();
        assert_eq!(var_names, ["simple.a", "simple.b", "simple.a_alias"]);
        let signal_ids = vars
            .iter()
            .map(|v| v.signal_ref().index())
            .collect::<Vec<_>>();
        assert_eq!(signal_ids, [0, 1, 0]);
    }

    // signal values
    let (a_ref, b_ref) = (
        SignalRef::from_index(0).unwrap(),
        SignalRef::from_index(1).unwrap(),
    );
    wave.load_signals(&[a_ref, b_ref]);
    let signal_a = wave.get_signal(a_ref).unwrap();
    assert_eq!(signal_a.get_first_time_idx(), Some(0));
    assert_eq!(signal_a.time_indices(), [0, 1, 2, 3, 4]);
    assert_eq!(
        signal_values_to_string(signal_a, wave.time_table()),
        "(0: 0), (1: 1), (5: 0), (7: x), (8: z)"
    );
    let signal_b = wave.get_signal(b_ref).unwrap();
    assert_eq!(
        signal_values_to_string(signal_b, wave.time_table()),
        "(0: xxxxxxxxxxxxxxxx), (1: 1010101010101010), (5: 101010xx10101010), (7: 0000000000000000)"
    );
}

use std::fmt::Write;
fn signal_values_to_string(signal: &wellen::Signal, time_table: &[Time]) -> String {
    let mut out = String::new();
    for (time, value) in signal.iter_changes() {
        write!(
            out,
            "({}: {}), ",
            time_table[time as usize],
            value.to_bit_string().unwrap()
        )
        .unwrap();
    }
    out.pop().unwrap();
    out.pop().unwrap();
    out
}

fn test_info(version: &str) -> FstInfo {
    FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: version.to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    }
}

/// A character that is not one of the nine `std_logic` states is rejected rather than silently
/// written.
///
/// `?` is the interesting one: the format has a code for it, `FST_RCV_Q`, but `fstapi.h` reserves
/// that code for a future escape mechanism and no reader can decode it as a value.
#[test]
fn write_invalid_bit_vector_character() {
    let filename = "tests/invalid_character.fst";
    let info = test_info("test 0.2.3");
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();

    writer.time_change(0).unwrap();
    for bad in ['q', '?'] {
        let err = writer
            .signal_change(a, bad.to_string().as_bytes())
            .unwrap_err();
        assert!(
            matches!(err, FstWriteError::InvalidCharacter(c) if c == bad),
            "expected {bad:?} to be rejected, got: {err:?}"
        );
    }
}
