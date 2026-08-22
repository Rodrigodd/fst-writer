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
    writer.flush().unwrap();
    // no more value changes, and flushing again should be a no-op
    writer.flush().unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0, 1]);
    let a_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[a_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table()),
        "(0: 0), (1: 1)"
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

    // flush the buffer, creating a new value change section
    writer.flush().unwrap();

    writer.time_change(7).unwrap();
    writer.signal_change(a, b"X").unwrap();
    writer.signal_change(b, b"0").unwrap();

    writer.time_change(8).unwrap();
    writer.signal_change(a, b"Z").unwrap();

    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();

    // timetable
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
