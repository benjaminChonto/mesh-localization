fn main() {
    linker_be_nice();
    println!("cargo:rerun-if-env-changed=ID");
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    generate_rssi_table();
}

fn generate_rssi_table() {
    use std::fmt::Write as _;
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = std::path::Path::new(&out_dir).join("rssi_to_dist.rs");

    // TODO env factor and RSSI at one meter should be changed here
    let mut code = String::new();
    // 10^((-56 - rssi) / 25.0) as I16F16 bits (i32), for rssi = -128..=127
    code.push_str("pub const RSSI_TO_DIST_BITS: [i32; 256] = [\n");
    for rssi in -128i32..=127 {
        let exponent = (-56.0f64 - rssi as f64) / 25.0;
        let dist = 10.0f64.powf(exponent).clamp(0.0, 32767.0);
        let bits = (dist * 65536.0).round() as i32;
        let _ = writeln!(code, "    {bits},");
    }
    code.push_str("];\n");

    std::fs::write(dest, code).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                "free"
                | "malloc"
                | "calloc"
                | "get_free_internal_heap_size"
                | "malloc_internal"
                | "realloc_internal"
                | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}
