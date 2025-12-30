// TODO: Lazy static maps (Model -> ColorMatrix)

// use phf::phf_map;

// pub struct CameraSpec {
//     pub black_level: u16,
//     pub white_level: u16,
//     pub color_matrix: [f32; 9],
// }

// // Static lookup. No file loading, no mutexes.
// static CAMERA_DB: phf::Map<&'static str, CameraSpec> = phf_map! {
//     "Sony ILCE-7M3" => CameraSpec {
//         black_level: 512,
//         white_level: 16383,
//         color_matrix: [ /* ... */ ],
//     },
//     // ...
// };

// pub fn get_camera_constants(model: &str) -> Option<&'static CameraSpec> {
//     CAMERA_DB.get(model)
// }
