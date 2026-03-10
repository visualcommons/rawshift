//! Pipeline example of developing RAW image into sRGB JPEG

use rawshift::formats::RawFile;
use std::fs::File;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let file = File::open("image.arw")?;
    let raw_file = RawFile::open(file)?;

    match raw_file {
        RawFile::Arw(_arw) => {
            println!("Opened ARW file");
        }
        RawFile::Cr2(_cr2) => {
            println!("Opened CR2 file");
        }
        RawFile::Dng(_dng) => {
            println!("Opened DNG file");
        }
        RawFile::Nef(_nef) => {
            println!("Opened NEF file");
        }
    }

    todo!()

    // Ok(())
}
