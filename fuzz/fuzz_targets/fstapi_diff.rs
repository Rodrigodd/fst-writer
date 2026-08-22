#![no_main]

use fst_reader::{FstHierarchyEntry, FstReader, ReaderError};
use fst_writer::{open_fst, FstFileType, FstInfo, FstSignalType, FstVarDirection, FstVarType};
use fstapi::{var_dir, var_type, Writer};
use libfuzzer_sys::fuzz_target;
use std::io::BufReader;
use std::path::Path;
use tempfile::tempdir;

/// Everything we compare between the two files, as parsed by `fst-reader`.
#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    time_table: Vec<u64>,
    vars: Vec<String>,
}

fn parse(path: &Path) -> Result<Parsed, ReaderError> {
    let input = BufReader::new(std::fs::File::open(path).unwrap());
    let mut reader = FstReader::open_and_read_time_table(input)?;

    // Sometimes the time tables differ in how the times are being duplicated.
    let mut time_table = reader.get_time_table().unwrap_or_default().to_vec();
    time_table.dedup();

    let mut vars = vec![];
    reader.read_hierarchy(|entry| {
        if let FstHierarchyEntry::Var { name, .. } = entry {
            vars.push(name);
        }
    })?;

    Ok(Parsed { time_table, vars })
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    let outdir = tempdir().unwrap();

    // Create the waveform.
    let fstfile = outdir.path().join("fstapi.fst");
    let mut fstapi = Writer::create(&fstfile, true)
        .unwrap()
        .comment("FST waveform example")
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();

    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "0.0.0".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let writerfile = outdir.path().join("fst-writer.fst");
    let mut writer = open_fst(&writerfile, &info).unwrap();

    let vars = (0..data[0] as usize)
        .map(|i| {
            let name = &format!("s{}", i);
            let width = 8;
            (
                fstapi
                    .create_var(var_type::VCD_REG, var_dir::OUTPUT, width, name, None)
                    .unwrap(),
                writer
                    .var(
                        name,
                        FstSignalType::bit_vec(8),
                        FstVarType::Logic,
                        FstVarDirection::Output,
                        None,
                    )
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();

    if vars.is_empty() {
        return;
    }

    let mut writer = writer.finish().unwrap();

    let mut timestamp: u64 = 0;

    // fstapi.emit_time_change(0).unwrap();
    // writer.time_change(0).unwrap();

    for chunk in data[1..].chunks(4) {
        let &[flush, dt, signal, value] = chunk else {
            break;
        };

        // Flush on odd `flush` bytes, i.e. roughly every other step, and only right before a time
        // step that actually advances. `signal_change` after a flush that is not followed by a new
        // time step hits a `todo!()` in the writer ("Currently we only support flushing right
        // before a new time step", `src/buffer.rs`), which this target would otherwise spend all
        // its time rediscovering.
        //
        // The reference only queues the flush (`fstWriterFlushContext` sets
        // `flush_context_pending`, and only once `tchn_idx > 1`, `fstapi.c:1835`) and acts on it at
        // the next time change, while our `flush` writes the section out right away. The two
        // therefore cut sections at different points, which the comparison below tolerates: the
        // time table is the concatenation of every section's table, so it does not depend on where
        // the boundaries fall.
        if flush % 2 != 0 && dt > 0 {
            fstapi.flush();
            writer.flush().unwrap();
        }

        timestamp += dt as u64;
        let signal = vars[signal as usize % vars.len()];
        let value = format!("{:08b}", value);

        // println!("{} {} {}", timestamp, signal.0, value);

        fstapi.emit_time_change(timestamp).unwrap();
        fstapi
            .emit_value_change(signal.0, value.as_bytes())
            .unwrap();

        writer.time_change(timestamp).unwrap();
        writer.signal_change(signal.1, value.as_bytes()).unwrap();
    }

    writer.finish().unwrap();

    drop(fstapi);

    // println!("Files: {:?} {:?}", fstfile, writerfile);

    // read

    // Ignore cases where it fail even in the reference writer
    let Ok(reference) = parse(&fstfile) else {
        return;
    };
    let ours = parse(&writerfile).unwrap();

    assert_eq!(reference, ours);
});
