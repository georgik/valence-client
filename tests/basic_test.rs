
use valence::protocol::Decode;
use valence::protocol::Packet;

use valence::protocol::packets::login::{LoginCompressionS2c, LoginSuccessS2c};
use valence::protocol::PacketDecoder;
use valence::protocol as valence_protocol;
#[cfg(test)]
mod tests {

    #[test]
    fn test_flate2_to_miniz_oxide_long_string() {
        use flate2::{write::DeflateEncoder, Compression};
        use miniz_oxide::inflate::decompress_to_vec;
        use std::io::Write;

        // Generate a string with 5912 bytes
        let long_string: String = "Hello, ESP32-S3! ".repeat(5912 / 17); // "Hello, ESP32-S3! " is 17 bytes

        // Compress the string using flate2
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(long_string.as_bytes()).expect("Failed to write data");
        let compressed_data = encoder.finish().expect("Failed to finish compression");

        // Decompress the compressed data using miniz_oxide
        let decompressed_data = decompress_to_vec(&compressed_data).expect("Decompression failed");

        // Convert decompressed data to a string
        let decompressed_string = String::from_utf8(decompressed_data).expect("Failed to convert to UTF-8");

        // Assert that the decompressed string matches the original
        assert_eq!(
            decompressed_string, long_string,
            "Decompressed string does not match original!"
        );
    }


}