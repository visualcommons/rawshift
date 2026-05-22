use clap::Parser;
use rawshift_image::formats::RawFile;
use rawshift_image::formats::export::EncodeOptions;
use rawshift_image::processing::{
    BayerAlgorithm, DemosaicMethod, ProcessingOptions, XTransAlgorithm,
};
use rawshift_image::transforms::BadPixelCorrectionMode;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input image file
    #[arg(required = true)]
    input: PathBuf,

    /// Output image path (extension determines format: png, jpg, webp, avif, jxl, dng)
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

    /// Enable bad pixel correction (median or average)
    #[arg(long, value_enum)]
    bad_pixel_correction: Option<BadPixelArg>,

    /// Bilateral denoising spatial sigma (e.g. 2.0). Omit to disable.
    #[arg(long)]
    denoise_sigma: Option<f32>,

    /// Chromatic aberration correction: red scale and blue scale (e.g. 0.999 1.001)
    #[arg(long, number_of_values = 2)]
    ca_correction: Option<Vec<f32>>,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum DemosaicAlgoArg {
    Bilinear,
    Amaze,
    Lmmse,
    Rcd,
    Markesteijn,
    Markesteijn3Pass,
    XTransFast,
}

impl From<DemosaicAlgoArg> for DemosaicMethod {
    fn from(arg: DemosaicAlgoArg) -> Self {
        match arg {
            DemosaicAlgoArg::Bilinear => DemosaicMethod::Bayer(BayerAlgorithm::Bilinear),
            DemosaicAlgoArg::Amaze => DemosaicMethod::Bayer(BayerAlgorithm::Amaze),
            DemosaicAlgoArg::Lmmse => DemosaicMethod::Bayer(BayerAlgorithm::Lmmse),
            DemosaicAlgoArg::Rcd => DemosaicMethod::Bayer(BayerAlgorithm::Rcd),
            DemosaicAlgoArg::Markesteijn => DemosaicMethod::XTrans(XTransAlgorithm::Markesteijn),
            DemosaicAlgoArg::Markesteijn3Pass => {
                DemosaicMethod::XTrans(XTransAlgorithm::Markesteijn3Pass)
            }
            DemosaicAlgoArg::XTransFast => DemosaicMethod::XTrans(XTransAlgorithm::Fast),
        }
    }
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum BadPixelArg {
    Median,
    Average,
}

impl From<BadPixelArg> for BadPixelCorrectionMode {
    fn from(arg: BadPixelArg) -> Self {
        match arg {
            BadPixelArg::Median => BadPixelCorrectionMode::Median,
            BadPixelArg::Average => BadPixelCorrectionMode::Average,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::TRACE)
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let Args {
        input,
        output,
        demosaic,
        white_balance,
        gamma,
        no_matrix,
        bad_pixel_correction,
        denoise_sigma,
        ca_correction,
    } = Args::parse();

    let encode_options = match output.extension().and_then(|ext| ext.to_str()) {
        Some("png") => EncodeOptions::png(),
        Some("jpg") | Some("jpeg") => EncodeOptions::jpeg(),
        Some("webp") => EncodeOptions::webp_lossy(),
        #[cfg(feature = "avif-encode")]
        Some("avif") => EncodeOptions::avif(),
        #[cfg(feature = "jxl-encode")]
        Some("jxl") => EncodeOptions::jxl(),

        #[cfg(not(feature = "jxl-encode"))]
        Some("jxl") => panic!(
            "JXL support requires the 'jxl-encode' feature. Compile with --features jxl-encode"
        ),
        _ => panic!("Unsupported/unknown output format: {}", output.display()),
    };

    println!("Opening {:?}", input);
    let file = File::open(&input)?;
    let reader = BufReader::new(file);
    let mut raw_file = RawFile::open(reader)?;
    // let meta = raw.metadata();
    // println!("Metadata: BitDepth={}", meta.image.bit_depth);

    let mut options = ProcessingOptions::new()
        .demosaic(demosaic.into())
        .gamma(gamma);

    // TODO: Remove white balance arg and just use what is extracted from metadata
    if let Some(wb) = white_balance
        && wb.len() == 3
    {
        println!(
            "Using custom White Balance: {:.2}, {:.2}, {:.2}",
            wb[0], wb[1], wb[2]
        );
        options = options.white_balance(wb[0], wb[1], wb[2]);
    }

    if !no_matrix {
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

    if let Some(bpc) = bad_pixel_correction {
        println!("Enabling bad pixel correction: {:?}", bpc);
        options = options.bad_pixel_correction(bpc.into());
    }

    if let Some(sigma) = denoise_sigma {
        println!("Enabling bilateral denoising: sigma={}", sigma);
        options = options.denoise(sigma);
    }

    if let Some(ca) = ca_correction
        && ca.len() == 2
    {
        println!(
            "Enabling CA correction: red_scale={}, blue_scale={}",
            ca[0], ca[1]
        );
        options = options.ca_correction(ca[0], ca[1]);
    }

    println!("Exporting to {:?}", output);
    raw_file.export(&output, &options, &encode_options)?;

    println!("Done!");
    Ok(())
}
