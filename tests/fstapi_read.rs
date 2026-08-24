// read files written by fst-writer with the reference implementation (GTKWave's fstapi),
// in order to check the things that the wellen reader does not look at

use fst_writer::*;

/// `fstapi.c` starts the first value change section, and thus the file, at the first time change,
/// if no value was written before it (`firsttime = vc_emitted ? 0 : tim` in
/// `fstWriterEmitTimeChange`, `xc->firsttime` written to the header in `fstWriterClose`).
#[test]
fn fstapi_read_first_time_change_not_zero() {
    let filename = "tests/first_time_change_fstapi.fst";
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

    //// read with the reference implementation
    let mut reader = fstapi::Reader::open(filename).unwrap();
    assert_eq!(reader.end_time(), 20);
    // the header start time is not read by wellen, only by the reference implementation
    assert_eq!(reader.start_time(), 10);

    // collect all value changes; the reference reader reports the frame at the section start time,
    // thus we only look at the earliest time and not at the exact sequence
    reader.set_mask_all();
    let mut times = vec![];
    reader
        .for_each_block(|time, _handle, value, _var_len| {
            times.push((time, String::from_utf8_lossy(value).to_string()))
        })
        .unwrap();
    let first_time = times.iter().map(|(t, _)| *t).min();
    assert_eq!(first_time, Some(10), "no sample before time 10: {times:?}");
}

/// Writes the same signal history with the reference implementation and with `fst-writer` and
/// compares what the reference reader makes of both files.
#[test]
fn fstapi_diff_first_time_change() {
    let steps: [(u64, &[u8]); 3] = [(10, b"00000001"), (20, b"00000000"), (30, b"00001111")];

    //// write with the reference implementation
    let fstapi_file = "tests/first_time_change_diff_fstapi.fst";
    let mut writer = fstapi::Writer::create(fstapi_file, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let var = writer
        .create_var(
            fstapi::var_type::VCD_REG,
            fstapi::var_dir::OUTPUT,
            8,
            "s",
            None,
        )
        .unwrap();
    for (time, value) in steps {
        writer.emit_time_change(time).unwrap();
        writer.emit_value_change(var, value).unwrap();
    }
    drop(writer);

    //// write the same thing with fst-writer
    let our_file = "tests/first_time_change_diff.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(our_file, &info).unwrap();
    let var = writer
        .var(
            "s",
            FstSignalType::bit_vec(8),
            FstVarType::Logic,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    for (time, value) in steps {
        writer.time_change(time).unwrap();
        writer.signal_change(var, value).unwrap();
    }
    writer.finish().unwrap();

    //// both files need to look the same to the reference reader
    assert_eq!(read_with_fstapi(our_file), read_with_fstapi(fstapi_file));
}

/// Returns the start time, the end time and all value changes, as seen by the reference reader.
fn read_with_fstapi(filename: &str) -> (u64, u64, Vec<(u64, String)>) {
    let mut reader = fstapi::Reader::open(filename).unwrap();
    reader.set_mask_all();
    let mut changes = vec![];
    reader
        .for_each_block(|time, _handle, value, _var_len| {
            changes.push((time, String::from_utf8_lossy(value).to_string()))
        })
        .unwrap();
    (reader.start_time(), reader.end_time(), changes)
}

/// A history that leaves the section without any value change produces a file with no value change
/// section at all, in both writers: the reference bails out of `fstWriterFlushContextPrivate` on
/// `vchg_siz <= 1` (`fstapi.c:1259`) without ever finalizing the block. Neither file is usable —
/// `fstReaderOpen` rejects a file whose `vc_section_count` is 0 (the `else` branch of the
/// `rc && vc_section_count && maxhandle && ...` check in `fstapi.c`) — but they have to be
/// *equally* unusable, which is what this pins.
///
/// Our file is the better formed of the two: the reference leaves its unfinalized section header
/// behind as a zero-length `FST_BL_SKIP` block, which readers cannot even walk past, whereas we
/// write nothing at all.
#[test]
fn fstapi_diff_no_value_changes() {
    for (name, history) in [
        // a value that only ever reaches the frame, and a first time change at 0 that makes
        // readers skip that frame
        (
            "value_then_time_change",
            &[Step::Value(b"1"), Step::Time(0)][..],
        ),
        // time changes without a single value
        ("only_time_changes", &[Step::Time(0), Step::Time(5)][..]),
    ] {
        let fstapi_file = format!("tests/no_vc_{name}_fstapi.fst");
        let our_file = format!("tests/no_vc_{name}.fst");
        write_with_fstapi(&fstapi_file, history);
        write_with_fst_writer(&our_file, history);

        // `Reader::open` fails for both, and it has to fail the same way
        let outcome = |f: &str| {
            fstapi::Reader::open(f)
                .map(|_| ())
                .map_err(|e| format!("{e:?}"))
        };
        assert_eq!(
            outcome(&our_file),
            outcome(&fstapi_file),
            "{name}: the reference reader must make the same thing of both files"
        );
    }
}

/// A real valued signal starts out as NaN, not as `x`. The reference seeds the eight bytes of its
/// current value with `strtod("NaN", NULL)` ("initialize doubles to NaN rather than x",
/// `fstWriterCreateVar`, `fstapi.c:2605-2611`), where every other signal gets `x`. Filling those
/// bytes with `x` instead makes the value read back as 2.068428470140581e272.
///
/// Nothing is written to the signal here, so the value that reaches the file is purely the
/// initializer, cloned into a mocked time zero step on close (`fstapi.c:1870-1883`).
#[test]
fn fstapi_diff_real_initial_value() {
    let fstapi_file = "tests/real_initial_value_fstapi.fst";
    let mut writer = fstapi::Writer::create(fstapi_file, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    writer
        .create_var(
            fstapi::var_type::VCD_REAL,
            fstapi::var_dir::OUTPUT,
            8,
            "r",
            None,
        )
        .unwrap();
    drop(writer);

    let our_file = "tests/real_initial_value.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(our_file, &info).unwrap();
    writer
        .var(
            "r",
            FstSignalType::real(),
            FstVarType::Real,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    writer.finish().unwrap().finish().unwrap();

    let (_, _, changes) = read_with_fstapi(our_file);
    assert_eq!(
        changes,
        [(0, "nan".to_string())],
        "an untouched real reads back as NaN, not as eight `x` bytes"
    );
    assert_eq!(read_with_fstapi(our_file), read_with_fstapi(fstapi_file));
}

/// One call of the writer API, so the same history can be replayed into both writers.
enum Step<'a> {
    Time(u64),
    Value(&'a [u8]),
}

fn write_with_fstapi(filename: &str, history: &[Step]) {
    let mut writer = fstapi::Writer::create(filename, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let var = writer
        .create_var(
            fstapi::var_type::VCD_REG,
            fstapi::var_dir::OUTPUT,
            1,
            "a",
            None,
        )
        .unwrap();
    for step in history {
        match step {
            Step::Time(time) => writer.emit_time_change(*time).unwrap(),
            Step::Value(value) => writer.emit_value_change(var, value).unwrap(),
        }
    }
}

fn write_with_fst_writer(filename: &str, history: &[Step]) {
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let var = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    for step in history {
        match step {
            Step::Time(time) => writer.time_change(*time).unwrap(),
            Step::Value(value) => writer.signal_change(var, value).unwrap(),
        }
    }
    writer.finish().unwrap();
}
