use assert_cmd::prelude::*;
use std::process::Command;
use std::str;

#[test]
fn echo_hello() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::cargo_bin("procstar")?
        .arg("--print")
        .arg("tests/int/specs/echo.json")
        .output()?;

    let mut lines = str::from_utf8(&output.stdout)?.lines();

    // First line is output from program.
    assert_eq!(lines.next().unwrap(), "Hello, world.");

    // Second line is JSON result.
    let res_jso: serde_json::Value = serde_json::from_str(lines.next().unwrap())?;
    let jso = &res_jso["test0"];
    eprintln!("jso: {res_jso}");
    assert_eq!(jso["status"]["status"], 0);
    assert!(jso["rusage"]["utime"].as_f64().unwrap() >= 0.);

    Ok(())
}
