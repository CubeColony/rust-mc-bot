use crate::packet_utils::Buf;
use flate2::write::{ZlibEncoder, ZlibDecoder};
use flate2::Compression;
use std::io::Write;
use std::collections::HashMap;
use crate::BotInfo;
use crate::login;
use crate::play;
use std::sync::Arc;
use futures_locks::{RwLock, RwLockWriteGuard};
use futures::io::ErrorKind;
use std::ops::Deref;

pub type Packet = fn(buffer: &mut Buf, bot: RwLockWriteGuard<BotInfo>);

pub struct PacketFramer {}

pub struct PacketCompressor {}

pub struct PacketProcessor {
    packets: HashMap<u8, HashMap<u8, Packet>>
}

impl PacketProcessor {
    pub fn new() -> Self {
        let mut map = HashMap::with_capacity(4);

        //Define packets here
        let mut login: HashMap<u8, Packet> = HashMap::new();

        login.insert(0x02, login::process_login_success_packet);
        login.insert(0x03, login::process_set_compression_packet);

        map.insert(0, login);


        let mut play: HashMap<u8, Packet> = HashMap::new();

        play.insert(0x1f, play::process_keep_alive_packet);

        map.insert(1, play);

        PacketProcessor { packets: map }
    }
}

impl PacketFramer {
    //extra data, not enough data, size
    pub fn process_read(buffer: &mut Buf) -> (bool, u32, u32) {
        let size = buffer.read_var_u32();
        if size == 0 {
            panic!("empty packet");
        }

        let length = buffer.buffer.len() as u32 - buffer.get_reader_index();
        if size != length {
            return if size > length {
                (false, size-length, size)
            } else {
                (true, 0, size)
            }
        }
        (false, 0, size)
    }

    pub fn process_write(buffer: Buf) -> Buf {
        let size = buffer.buffer.len();
        let header_size = Buf::get_var_u32_size(size as u32);
        if header_size > 3 {
            panic!("header_size > 3")
        }
        let mut target = Buf::with_length(size as u32 + header_size);
        target.write_var_u32(size as u32);
        target.append(buffer);
        target
    }
}

impl PacketCompressor {
    pub fn process_read(buffer: &mut Buf, length : u32) -> Option<Buf> {
        let real_length = buffer.read_var_u32();
        //buffer is not compressed
        if real_length == 0 {
            return None;
        }
        let mut output = Buf::with_capacity(real_length);
        {
            let mut decompressor = ZlibDecoder::new(&mut output.buffer);
            let r = decompressor.write_all(&buffer.buffer[buffer.get_reader_index() as usize..(length - Buf::get_var_u32_size(real_length) + buffer.get_reader_index()) as usize]);
            if r.is_err() {
                match r.as_ref().unwrap_err().kind() {
                    ErrorKind::WriteZero => (),
                    _ => r.unwrap()
                }
            }
        }
        buffer.set_reader_index(buffer.get_reader_index() + length);
        Some(output)
    }

    pub fn process_write<D: Deref<Target=BotInfo>>(buffer: Buf, bot: Arc<D>) -> Buf {
        if buffer.buffer.len() as i32 > bot.compression_threshold {
            let mut buf = Buf::new();
            buf.write_var_u32(buffer.buffer.len() as u32);
            let mut compressor = ZlibEncoder::new(&mut buf.buffer, Compression::fast());
            compressor.write_all(&buffer.buffer[buffer.get_writer_index() as usize..]).unwrap();
            compressor.flush_finish().unwrap();
            buf
        } else {
            let mut buf = Buf::new();
            buf.write_var_u32(0);
            buf.append(buffer);
            buf
        }
    }
}

impl PacketProcessor {
    pub async fn process_decode(&self, buffer: &mut Buf, bot: Arc<RwLock<BotInfo>>) -> Option<()> {
        let bot = bot.write().await;
        let packet_id = buffer.read_var_u32() as u8;
        (self.packets.get(&bot.state)?.get(&packet_id)?)(buffer, bot);
        Some(())
    }
}