mod command;
mod connection;
mod detect;
mod transfer;
mod tx;

use super::TextEncoding;

struct TextStreamDecoder {
    decoder: encoding_rs::Decoder,
}

impl TextStreamDecoder {
    fn new(encoding: TextEncoding) -> Self {
        Self {
            decoder: encoding.decoder(),
        }
    }

    fn decode(&mut self, chunk: &[u8]) -> String {
        self.decode_inner(chunk, false)
    }

    fn finish(&mut self) -> String {
        self.decode_inner(&[], true)
    }

    fn decode_inner(&mut self, input: &[u8], last: bool) -> String {
        let initial_capacity = self
            .decoder
            .max_utf8_buffer_length(input.len())
            .unwrap_or_else(|| input.len().saturating_mul(3).saturating_add(3));
        let mut output = String::with_capacity(initial_capacity);
        let mut read = 0;

        loop {
            let (result, consumed, _) =
                self.decoder
                    .decode_to_string(&input[read..], &mut output, last);
            read += consumed;
            match result {
                encoding_rs::CoderResult::InputEmpty => return output,
                encoding_rs::CoderResult::OutputFull => {
                    let additional = self
                        .decoder
                        .max_utf8_buffer_length(input.len().saturating_sub(read))
                        .unwrap_or(4)
                        .max(4);
                    output.reserve(additional);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TextStreamDecoder;
    use crate::session::TextEncoding;

    #[test]
    fn utf8_decoder_preserves_characters_split_across_chunks() {
        let mut decoder = TextStreamDecoder::new(TextEncoding::Utf8);

        assert_eq!(decoder.decode(&[b'a', 0xe4]), "a");
        assert_eq!(decoder.decode(&[0xb8]), "");
        assert_eq!(decoder.decode(&[0xad, b'b']), "中b");
        assert_eq!(decoder.finish(), "");
    }

    #[test]
    fn utf8_decoder_replaces_invalid_bytes_without_dropping_valid_text() {
        let mut decoder = TextStreamDecoder::new(TextEncoding::Utf8);

        assert_eq!(
            decoder.decode(b"before\xffconfiguration\xfeafter"),
            "before\u{fffd}configuration\u{fffd}after"
        );
        assert_eq!(decoder.finish(), "");
    }

    #[test]
    fn utf8_decoder_output_does_not_depend_on_ssh_chunk_boundaries() {
        let input = b"prefix\xb5\xe7configuration\xe4\xb8\xadsuffix";
        let expected = String::from_utf8_lossy(input);

        for chunk_size in 1..=input.len() {
            let mut decoder = TextStreamDecoder::new(TextEncoding::Utf8);
            let mut actual = String::new();
            for chunk in input.chunks(chunk_size) {
                actual.push_str(&decoder.decode(chunk));
            }
            actual.push_str(&decoder.finish());
            assert_eq!(actual, expected, "chunk size {chunk_size}");
        }
    }

    #[test]
    fn utf8_decoder_flushes_an_incomplete_trailing_character() {
        let mut decoder = TextStreamDecoder::new(TextEncoding::Utf8);

        assert_eq!(decoder.decode(&[b'a', 0xe4]), "a");
        assert_eq!(decoder.finish(), "\u{fffd}");
    }

    #[test]
    fn chinese_encoding_variants_decode_through_gb18030() {
        let encoded = [0xd6, 0xd0, 0xce, 0xc4];

        for encoding in [
            TextEncoding::Gb2312,
            TextEncoding::Gbk,
            TextEncoding::Gb18030,
        ] {
            let mut decoder = TextStreamDecoder::new(encoding);
            assert_eq!(decoder.decode(&encoded[..1]), "");
            assert_eq!(decoder.decode(&encoded[1..3]), "中");
            assert_eq!(decoder.decode(&encoded[3..]), "文");
            assert_eq!(decoder.finish(), "");
        }
    }

    #[test]
    fn gb18030_decoder_supports_four_byte_characters_across_chunks() {
        let (encoded, _, had_errors) = encoding_rs::GB18030.encode("\u{20000}");
        assert!(!had_errors);

        let mut decoder = TextStreamDecoder::new(TextEncoding::Gb18030);
        let mut decoded = String::new();
        for byte in encoded.iter() {
            decoded.push_str(&decoder.decode(std::slice::from_ref(byte)));
        }
        decoded.push_str(&decoder.finish());

        assert_eq!(decoded, "\u{20000}");
    }
}
