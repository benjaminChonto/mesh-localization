fn main() {
    linker_be_nice();
    println!("cargo:rerun-if-env-changed=ID");
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    generate_rssi_table();
}

fn generate_rssi_table() {
    use std::fmt::Write as _;

    // RSSI at 1 m reference distance (dBm). Measured value for this hardware.
    let rssi_ref: f64 = std::env::var("RSSI_REF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(-56.0);

    // Path-loss exponent n (unitless). Free-space = 2.0, indoors = 3.0-4.0.
    // Formula: dist = 10 ^ ((rssi_ref - rssi) / (10 * n))
    let n: f64 = std::env::var("PATH_LOSS_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2.5);

    println!("cargo:rerun-if-env-changed=RSSI_REF");
    println!("cargo:rerun-if-env-changed=PATH_LOSS_N");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:warning=RSSI model: ref={rssi_ref} dBm at 1 m, n={n}");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = std::path::Path::new(&out_dir).join("rssi_to_dist.rs");

    let mut code = String::new();
    let _ = writeln!(
        code,
        "// dist = 10^((rssi_ref - rssi) / (10*n)), rssi_ref={rssi_ref}, n={n}"
    );
    code.push_str("pub const RSSI_TO_DIST_BITS: [i32; 256] = [\n");
    for rssi in -128i32..=127 {
        let exponent = (rssi_ref - rssi as f64) / (10.0 * n);
        let dist = 10.0f64.powf(exponent).clamp(0.0, 32767.0);
        let bits = (dist * 65536.0).round() as i32;
        let _ = writeln!(code, "    {bits},");
    }
    code.push_str("];\n");

    std::fs::write(dest, code).unwrap();
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
