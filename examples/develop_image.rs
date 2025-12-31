//! Pipeline example of developing RAW image into sRGB JPEG

use rawshift::formats::RawFile;
use std::fs::File;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let file = File::open("image.arw")?;
    let raw_file = RawFile::open(file)?;

    match raw_file {
        RawFile::Arw(_arw) => {
            println!("Opened ARW files");
        }
        RawFile::Dng => {
            println!("Opened DNG file");
        }
    }

    todo!()

    // Ok(())
}
