use clap::Parser;
use rawshift::formats::RawFile;
use rawshift::processing::{BayerAlgorithm, DemosaicMethod, ProcessingOptions};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input ARW file
    #[arg(required = true)]
    input: PathBuf,

    /// Output image path (extension determines format: png, jpg, tiff)
    #[arg(required = true)]
    output: PathBuf,

    /// Demosaic algorithm (for Bayer sensors)
    #[arg(short, long, value_enum, default_value_t = DemosaicAlgoArg::Bilinear)]
    demosaic: DemosaicAlgoArg,

    /// White balance multipliers (red, green, blue).
    /// If not provided, a default Daylight preset is used.
    #[arg(short, long, number_of_values = 3)]
    white_balance: Option<Vec<f32>>,

    /// Gamma correction value (default 2.2 for sRGB)
    #[arg(short, long, default_value_t = 2.2)]
    gamma: f32,

    /// Disable default color matrix (use identity)
    #[arg(long)]
    no_matrix: bool,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum DemosaicAlgoArg {
    Bilinear,
    Amaze,
    Lmmse,
    Rcd,
}

impl From<DemosaicAlgoArg> for DemosaicMethod {
    fn from(arg: DemosaicAlgoArg) -> Self {
        match arg {
            DemosaicAlgoArg::Bilinear => DemosaicMethod::Bayer(BayerAlgorithm::Bilinear),
            DemosaicAlgoArg::Amaze => DemosaicMethod::Bayer(BayerAlgorithm::Amaze),
            DemosaicAlgoArg::Lmmse => DemosaicMethod::Bayer(BayerAlgorithm::Lmmse),
            DemosaicAlgoArg::Rcd => DemosaicMethod::Bayer(BayerAlgorithm::Rcd),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("Opening {:?}", args.input);
    let file = File::open(&args.input)?;
    let reader = BufReader::new(file);
    let mut raw_file = RawFile::open(reader)?;

    let mut options = ProcessingOptions::new()
        .demosaic(args.demosaic.into())
        .gamma(args.gamma);

    // TODO: Remove white balance arg and just use what is extracted from metadata
    if let Some(wb) = args.white_balance {
        if wb.len() == 3 {
            println!(
                "Using custom White Balance: {:.2}, {:.2}, {:.2}",
                wb[0], wb[1], wb[2]
            );
            options = options.white_balance(wb[0], wb[1], wb[2]);
        }
    } else {
        println!("Using default Daylight White Balance");
        options = options.white_balance(2.35, 1.0, 1.65);
    }

    if !args.no_matrix {
        // Default generic "Neutral" matrix
        println!("Applying Color Matrix");
        #[rustfmt::skip]
        let matrix = [
            1.6, -0.4, -0.2,
            -0.2, 1.4, -0.2,
            -0.1, -0.3, 1.4,
        ];
        options = options.color_matrix(matrix);
    } else {
        println!("Color Matrix disabled");
    }

    println!("Exporting to {:?}", args.output);
    raw_file.export(&args.output, &options)?;

    println!("Done!");
    Ok(())
}
