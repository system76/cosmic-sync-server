fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/sync.proto");
    tonic_build::configure()
        .extern_path(".google.protobuf.Timestamp", "::prost_types::Timestamp")
        .compile(&["proto/sync.proto"], &["proto"])?;
    Ok(())
}
