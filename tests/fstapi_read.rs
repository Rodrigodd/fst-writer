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
///
/// Note that the `fstapi` writer needs to be able to create temporary files with `tmpfile()`,
/// i.e. it needs write access to `/tmp`, otherwise it fails with `ContextCreate`.
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
