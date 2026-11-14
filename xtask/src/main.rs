//! `cargo xtask layers` — a lower layer never depends on a higher one.
//! `cargo xtask ci` — what CI runs: fmt, clippy, tests, layers, the wasm build and its size budget.

use std::collections::HashMap;
use std::process::{Command, ExitCode};

/// Layer of every workspace crate. A crate may depend only on crates of a strictly lower layer.
const LAYERS: &[(&str, u8)] = &[
    ("sfdoc-geom", 0),
    ("sfdoc-model", 1),
    ("sfdoc-fonts", 1),
    ("sfdoc-layout", 2),
    ("sfdoc-raster", 3),
    ("sfdoc-pdf", 3),
    ("sfdoc-wasm", 4),
    ("sfdoc-cli", 4),
    // The speller stands apart from layout: it shares no crate with it and ships as its own wasm module.
    ("sfdoc-spell", 0),
    ("sfdoc-spell-wasm", 4),
];

/// Size budget of the release wasm, gzip-compressed (plan, section 8).
const WASM_GZIP_BUDGET: u64 = 1_500_000;

fn layers() -> Result<String, String> {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let list = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let string = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_owned();
    let layer: HashMap<&str, u8> = LAYERS.iter().copied().collect();
    let mut errors = Vec::new();
    let mut seen = 0;
    for pkg in list(&meta, "packages") {
        let name = string(&pkg, "name");
        let name = name.as_str();
        if name == "xtask" {
            continue;
        }
        let Some(own) = layer.get(name) else {
            errors.push(format!("{name} has no layer: add it to LAYERS in xtask/src/main.rs"));
            continue;
        };
        seen += 1;
        for dep in list(&pkg, "dependencies") {
            // Dev-dependencies are test scaffolding, not part of the crate's layering.
            if string(&dep, "kind") == "dev" {
                continue;
            }
            let dep_name = string(&dep, "name");
            if let Some(dep_layer) = layer.get(dep_name.as_str())
                && dep_layer >= own
            {
                errors.push(format!("{name} (layer {own}) depends on {dep_name} (layer {dep_layer})"));
            }
        }
    }
    if seen != LAYERS.len() {
        errors.push(format!("LAYERS lists {} crates, the workspace has {seen} of them", LAYERS.len()));
    }
    if errors.is_empty() { Ok(format!("layers: {seen} crates, no violations")) } else { Err(errors.join("\n")) }
}

fn step(name: &str, program: &str, args: &[&str]) -> Result<(), String> {
    eprintln!("── {name}: {program} {}", args.join(" "));
    let status = Command::new(program).args(args).status().map_err(|e| format!("{name}: cannot run {program}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{name} failed")) }
}

fn wasm_size() -> Result<String, String> {
    let path = "crates/wasm/pkg/sfdoc_wasm_bg.wasm";
    let out = Command::new("gzip").args(["-9", "-c", path]).output().map_err(|e| format!("gzip: {e}"))?;
    if !out.status.success() {
        return Err(format!("cannot read {path}"));
    }
    let size = out.stdout.len() as u64;
    if size > WASM_GZIP_BUDGET {
        return Err(format!("wasm is {size} bytes gzip, over the budget of {WASM_GZIP_BUDGET}"));
    }
    Ok(format!("wasm: {size} bytes gzip (budget {WASM_GZIP_BUDGET})"))
}

fn ci() -> Result<String, String> {
    let cargo = env!("CARGO");
    step("fmt", cargo, &["fmt", "--all", "--check"])?;
    step("clippy", cargo, &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    step("test", cargo, &["test", "--workspace", "--quiet"])?;
    let l = layers()?;
    step("wasm", "wasm-pack", &["build", "crates/wasm", "--release", "--target", "web", "--no-pack"])?;
    step("spell wasm", "wasm-pack", &["build", "crates/spell-wasm", "--release", "--target", "web", "--no-pack"])?;
    Ok(format!("{l}\n{}", wasm_size()?))
}

fn main() -> ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("layers") => layers(),
        Some("ci") => ci(),
        Some("wasm-size") => wasm_size(),
        _ => Err("usage: cargo xtask <layers|ci|wasm-size>".to_owned()),
    };
    match result {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
