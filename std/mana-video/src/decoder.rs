use anyhow::Context;
use ffmpeg_next::util::frame::Video;

pub struct SoftwareDecoder {
    decoder: ffmpeg_next::decoder::Video,
}

impl SoftwareDecoder {
    pub fn new(codec_id: ffmpeg_next::codec::Id) -> anyhow::Result<Self> {
        let codec = ffmpeg_next::codec::decoder::find(codec_id)
            .with_context(|| format!("{codec_id:?} codec not found"))?;
        let ctx = ffmpeg_next::codec::context::Context::new();
        let decoder = ctx
            .decoder()
            .open_as(codec)
            .with_context(|| format!("open {codec_id:?} decoder"))?
            .video()
            .context("not a video decoder")?;
        Ok(Self { decoder })
    }

    pub fn decode<F>(&mut self, data: &[u8], mut on_frame: F) -> anyhow::Result<()>
    where
        F: FnMut(&Video) -> anyhow::Result<()>,
    {
        let pkt = ffmpeg_next::codec::packet::Packet::copy(data);
        self.decoder.send_packet(&pkt).context("send_packet")?;
        let mut decoded = Video::empty();
        loop {
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    on_frame(&decoded)?;
                    decoded = Video::empty();
                }
                Err(ffmpeg_next::Error::Other { errno }) if errno == ffmpeg_next::error::EAGAIN => {
                    break;
                }
                Err(e) => return Err(anyhow::anyhow!("decode: {e}")),
            }
        }
        Ok(())
    }
}
