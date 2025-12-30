//! Optimized bitreader
//!
//! This module is extremely performance-critical. Generic bit-reading crates would not work they are zero-copy and heavily optimized.

// Note: Must handle Endianness dynamically (e.g., CR2 is LE, some old formats are BE).
