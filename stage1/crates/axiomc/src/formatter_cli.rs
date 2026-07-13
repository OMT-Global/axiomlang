use super::formatter::{FormatRange, format_axiom_sources, format_axiom_stdin};
use super::print_error;
use axiomc::{diagnostics::Diagnostic, json_contract};
use std::io::{self, Read};
use std::path::Path;

pub(super) fn parse_format_range(value: &str) -> Result<FormatRange, String> {
    let (start, end) = value
        .split_once(':')
        .ok_or_else(|| "format range must use START:END UTF-8 byte offsets".to_string())?;
    let start_byte = start
        .parse::<usize>()
        .map_err(|_| "format range START must be a non-negative integer".to_string())?;
    let end_byte = end
        .parse::<usize>()
        .map_err(|_| "format range END must be a non-negative integer".to_string())?;
    if start_byte > end_byte {
        return Err("format range START must not exceed END".to_string());
    }
    Ok(FormatRange {
        start_byte,
        end_byte,
    })
}

pub(super) fn run(
    path: Option<&Path>,
    stdin: bool,
    range: Option<FormatRange>,
    check: bool,
    json: bool,
) -> i32 {
    let stdin_result = if stdin {
        let mut source = String::new();
        match io::stdin().read_to_string(&mut source) {
            Ok(_) => Some(format_axiom_stdin(&source, check, range)),
            Err(error) => Some(Err(Diagnostic::new(
                "fmt.stdin.read",
                format!("failed to read formatter input from stdin: {error}"),
            ))),
        }
    } else {
        None
    };
    let result = match stdin_result {
        Some(result) => result.map(|(formatted, report)| (Some(formatted), report)),
        None => format_axiom_sources(path.expect("clap requires fmt path"), check)
            .map(|report| (None, report)),
    };
    match result {
        Ok((formatted, report)) => {
            let serialization_error = if json {
                match json_contract::to_pretty_string(&report) {
                    Ok(output) => {
                        println!("{output}");
                        None
                    }
                    Err(error) => Some(error),
                }
            } else {
                if let Some(formatted) = formatted {
                    if !check {
                        print!("{formatted}");
                    }
                }
                None
            };
            if let Some(error) = serialization_error {
                print_error("fmt", error, true)
            } else {
                if !json {
                    for file in &report.files {
                        if file.changed {
                            eprintln!("formatted {}", file.path);
                        }
                    }
                    if check && report.changed > 0 {
                        eprintln!("{} file(s) need formatting", report.changed);
                    } else {
                        eprintln!("checked {} file(s)", report.files.len());
                    }
                }
                if check && report.changed > 0 { 1 } else { 0 }
            }
        }
        Err(error) => print_error("fmt", error, json),
    }
}
