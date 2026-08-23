use std::{env, fs, path::PathBuf};

use cainome::rs::ExecutionVersion;

/// Contracts to generate bindings for: (ABI name, module name).
const STARKNET_DEPLOYMENTS: [(&str, &str); 1] = [("Liquidate", "liquidate")];

fn main() {
    println!("cargo::rerun-if-changed=abis");
    println!("cargo::rerun-if-changed=build.rs");

    let abi_base =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("abis");
    // Generated code lives in `OUT_DIR` so that `cargo fmt --check` never sees it;
    // `src/bindings.rs` includes it.
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    for (abi_file, module) in STARKNET_DEPLOYMENTS {
        let contract_class =
            abi_base.join(format!("vesu_v2_periphery_{abi_file}.contract_class.json"));
        let contract_class = contract_class
            .to_str()
            .expect("contract class path is not valid utf8");

        let bindings = cainome::rs::Abigen::new(abi_file, contract_class)
            .with_execution_version(ExecutionVersion::V3)
            .with_derives(vec![
                "Debug".into(),
                "Clone".into(),
                "serde::Deserialize".into(),
                "serde::Serialize".into(),
            ])
            .generate()
            .unwrap_or_else(|e| panic!("could not generate bindings for {contract_class}: {e:?}"));

        let destination = out_dir.join(format!("{module}.rs"));
        fs::write(&destination, bindings.to_string())
            .unwrap_or_else(|e| panic!("could not write bindings to {destination:?}: {e}"));
    }
}
