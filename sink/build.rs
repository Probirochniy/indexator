fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../proto");

    tonic_prost_build::configure().compile_protos(
        &[
            "../proto/erc20.proto",
            "../proto/substreams.proto",
            "../proto/package.proto",
        ],
        &["../proto"],
    )?;
    Ok(())
}
