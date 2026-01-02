//! Parse all metadata from image

use std::env;
use std::fs::File;
use std::io::BufReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args().nth(1).expect("Usage: metadata_dump <file>");

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let raw = rawshift::formats::RawFile::open(reader)?;

    // Extract metadata
    let metadata = raw.metadata();
    println!("Metadata: {:#?}", metadata);

    Ok(())
}
