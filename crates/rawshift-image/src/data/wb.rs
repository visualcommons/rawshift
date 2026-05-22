//! Standard illuminant reference data.
//!
//! CIE standard illuminants used for white balance and color matrix
//! interpolation. Values are from CIE publications and the DNG specification.

/// Standard illuminant specification.
#[derive(Debug, Clone, Copy)]
pub struct Illuminant {
    /// Human-readable name
    pub name: &'static str,
    /// EXIF LightSource tag value
    pub exif_code: u16,
    /// Correlated Color Temperature in Kelvin
    pub cct: u32,
    /// CIE 1931 chromaticity x coordinate
    pub x: f64,
    /// CIE 1931 chromaticity y coordinate
    pub y: f64,
}

impl Illuminant {
    /// Compute the XYZ tristimulus values (Y=1 normalized).
    pub fn xyz(&self) -> [f64; 3] {
        let big_x = self.x / self.y;
        let big_z = (1.0 - self.x - self.y) / self.y;
        [big_x, 1.0, big_z]
    }
}

/// Standard Illuminant A — incandescent/tungsten (~2856K).
pub const ILLUMINANT_A: Illuminant = Illuminant {
    name: "Standard Illuminant A",
    exif_code: 17,
    cct: 2856,
    x: 0.44757,
    y: 0.40745,
};

/// CIE Illuminant D50 — ICC profile connection space (~5003K).
pub const D50: Illuminant = Illuminant {
    name: "D50",
    exif_code: 23,
    cct: 5003,
    x: 0.34567,
    y: 0.35850,
};

/// CIE Illuminant D55 (~5503K).
pub const D55: Illuminant = Illuminant {
    name: "D55",
    exif_code: 20,
    cct: 5503,
    x: 0.33242,
    y: 0.34743,
};

/// CIE Illuminant D65 — daylight reference (~6504K).
pub const D65: Illuminant = Illuminant {
    name: "D65",
    exif_code: 21,
    cct: 6504,
    x: 0.31271,
    y: 0.32902,
};

/// CIE Illuminant D75 (~7504K).
pub const D75: Illuminant = Illuminant {
    name: "D75",
    exif_code: 22,
    cct: 7504,
    x: 0.29902,
    y: 0.31485,
};

/// Cool White Fluorescent (~4150K).
pub const COOL_WHITE_FLUORESCENT: Illuminant = Illuminant {
    name: "Cool White Fluorescent",
    exif_code: 15,
    cct: 4150,
    x: 0.37510,
    y: 0.36714,
};

/// All standard illuminants in the database.
static ILLUMINANTS: &[Illuminant] = &[ILLUMINANT_A, COOL_WHITE_FLUORESCENT, D50, D55, D65, D75];

/// Look up an illuminant by its EXIF LightSource code.
pub fn from_exif_code(code: u16) -> Option<&'static Illuminant> {
    ILLUMINANTS.iter().find(|i| i.exif_code == code)
}

/// Look up an illuminant by name (case-insensitive).
pub fn from_name(name: &str) -> Option<&'static Illuminant> {
    let lower = name.to_lowercase();
    ILLUMINANTS.iter().find(|i| i.name.to_lowercase() == lower)
}

/// Find the closest standard illuminant to a given CCT.
pub fn nearest_by_cct(cct: u32) -> &'static Illuminant {
    ILLUMINANTS
        .iter()
        .min_by_key(|i| (i.cct as i64 - cct as i64).unsigned_abs())
        .expect("illuminant table is non-empty")
}

/// Returns all standard illuminants.
pub fn all_illuminants() -> &'static [Illuminant] {
    ILLUMINANTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d65_values() {
        assert_eq!(D65.cct, 6504);
        assert!((D65.x - 0.31271).abs() < 1e-5);
        assert!((D65.y - 0.32902).abs() < 1e-5);
    }

    #[test]
    fn test_d50_xyz() {
        let xyz = D50.xyz();
        // D50: X=0.9642, Y=1.0, Z=0.8251 (approximately)
        assert!((xyz[0] - 0.9642).abs() < 0.01);
        assert!((xyz[1] - 1.0).abs() < 1e-10);
        assert!((xyz[2] - 0.8251).abs() < 0.01);
    }

    #[test]
    fn test_exif_lookup() {
        let d65 = from_exif_code(21).unwrap();
        assert_eq!(d65.name, "D65");

        let std_a = from_exif_code(17).unwrap();
        assert_eq!(std_a.name, "Standard Illuminant A");
    }

    #[test]
    fn test_unknown_exif_code() {
        assert!(from_exif_code(255).is_none());
    }

    #[test]
    fn test_nearest_cct() {
        let nearest = nearest_by_cct(6500);
        assert_eq!(nearest.name, "D65");

        let nearest = nearest_by_cct(3000);
        assert_eq!(nearest.name, "Standard Illuminant A");

        let nearest = nearest_by_cct(5000);
        assert_eq!(nearest.name, "D50");
    }

    #[test]
    fn test_all_illuminants() {
        let all = all_illuminants();
        assert_eq!(all.len(), 6);
    }
}
