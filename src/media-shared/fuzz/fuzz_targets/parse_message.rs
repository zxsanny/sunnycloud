#![no_main]

use libfuzzer_sys::fuzz_target;
use media_shared::decoder_protocol::{
    FrameReader, MAX_PAYLOAD, Polled, encode_message, parse_message,
};

fuzz_target!(|data: &[u8]| {
    if let Ok(message) = parse_message(data) {
        let encoded = encode_message(&message).expect("an accepted message encodes");
        assert_eq!(parse_message(&encoded), Ok(message));
    }

    let mut source = data;
    let mut reader = FrameReader::new();
    loop {
        let polled = reader.poll_reply(&mut source);
        assert!(reader.buffered_capacity() <= MAX_PAYLOAD);
        match polled {
            Ok(Polled::Ready(_)) => {}
            Ok(Polled::Pending | Polled::Closed) | Err(_) => break,
        }
    }
});
