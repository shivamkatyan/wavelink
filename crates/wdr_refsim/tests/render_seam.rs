//! Render-seam test (WS3): the receiver must deliver decoded canonical PCM
//! bytes through the injectable `RenderSink` seam — not just the built-in null
//! device — proving a real desktop/mobile output sink can plug into the same
//! pipeline the golden tests verify.

use std::cell::RefCell;
use std::rc::Rc;

use wdr_codec::{CodecAdapter, FlacAdapter};
use wdr_proto::{ChannelLayout, Codec, Frame, FrameFlags, SampleRepr};
use wdr_refsim::emitter::BufferMeta;
use wdr_refsim::receiver::{BufferProfile, ClockHandle, Receiver};
use wdr_refsim::sink::{RenderSink, SinkError, SinkFormat};

/// A `RenderSink` that records everything it is fed, so the test can assert
/// not just the final hash but the exact `on_format`/`on_block`/`finish`
/// contract a real output device would rely on.
#[derive(Default)]
struct RenderRecorder {
    formats: Vec<SinkFormat>,
    bytes: Vec<u8>,
    finish_count: usize,
}

struct RecorderSink(Rc<RefCell<RenderRecorder>>);

impl RenderSink for RecorderSink {
    fn on_format(&mut self, fmt: SinkFormat) -> Result<(), SinkError> {
        self.0.borrow_mut().formats.push(fmt);
        Ok(())
    }

    fn on_block(&mut self, bytes: &[u8]) -> Result<(), SinkError> {
        self.0.borrow_mut().bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), SinkError> {
        self.0.borrow_mut().finish_count += 1;
        Ok(())
    }
}

#[test]
fn receiver_renders_decoded_canonical_bytes_through_render_sink_seam() {
    // One canonical lossless FLAC frame (512 samples/channel = 1024 values).
    let meta = BufferMeta::canonical_lossless(Codec::Flac);
    let mut adapter = FlacAdapter::new(meta.sample_rate, meta.channels, 16).unwrap();
    let samples: Vec<i16> = (0..1024).map(|i| (i as i16).wrapping_mul(37)).collect();
    let payload = adapter.encode(&samples).unwrap();
    let frame = Frame::new_lossless(
        meta.stream_id(),
        0,
        0,
        Codec::Flac,
        meta.sample_rate,
        SampleRepr::I16,
        ChannelLayout::Stereo,
        meta.frame_samples as u32,
        FrameFlags::default(),
        payload.into(),
    );
    let packed = frame.pack().expect("frame packs");

    let recorder = Rc::new(RefCell::new(RenderRecorder::default()));
    let mut receiver =
        Receiver::for_stream(meta, BufferProfile::Balanced, ClockHandle::system(), 0)
            .expect("receiver builds");
    receiver.set_render_sink(Box::new(RecorderSink(recorder.clone())));

    receiver.ingest_bytes(&packed).expect("frame ingests");
    receiver.ingest_end_marker(1);
    let outcome = receiver.finalize();

    let rec = recorder.borrow();
    // The format is announced exactly once, with the stream metadata.
    assert_eq!(rec.formats, vec![SinkFormat::canonical()]);
    // The decoded canonical bytes exactly match the source i16-le stream.
    let expected: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    assert_eq!(
        rec.bytes, expected,
        "render seam must see decoded canonical bytes"
    );
    // End-of-stream flushes the sink once.
    assert_eq!(rec.finish_count, 1);
    // A non-verifying sink reports no integrity hash; the outcome carries the
    // documented empty-input hash (hash of nothing verified). The recorder's
    // *bytes* are asserted above to exactly match the source, which is the
    // lossless proof at this level of the seam.
    assert_eq!(outcome.hash_hex(), blake3::hash(&[]).to_hex().to_string());
    assert_eq!(outcome.metrics.underruns, 0);
    assert_eq!(outcome.metrics.malformed, 0);
    assert_eq!(outcome.metrics.packets_recv, 1);
}
