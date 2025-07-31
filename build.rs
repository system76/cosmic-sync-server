fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .extern_path(".google.protobuf.Timestamp", "::prost_types::Timestamp")
        .compile(&["proto/sync.proto"], &["proto"])?;
    Ok(())
}
