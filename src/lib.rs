use std::io;
use std::io::Write;
use std::net::TcpStream;

/// A connection to a Tev instance. Constructed using [TevClient::new]. Use [TevClient::send] to send commands.
#[derive(Debug)]
pub struct TevClient {
    socket: TcpStream,
}

impl TevClient {
    /// Create a [TevClient] from an existing [TcpStream].
    /// # Example
    /// If `tev` was started with the default hostname use something like
    /// ```no_run
    /// # use tev_client::TevClient;
    /// # use std::net::TcpStream;
    /// # fn main() -> std::io::Result<()> {
    /// let mut client = TevClient::new(TcpStream::connect("127.0.0.1:14158")?);
    /// # }
    /// ```
    pub fn new(socket: TcpStream) -> Self {
        TevClient { socket }
    }

    /// Send a command to `tev`. A command is any struct in this crate that implements [TevPacket].
    /// # Example
    /// ```no_run
    /// # use tev_client::{TevClient, PacketOpenImage};
    /// # fn main() -> std::io::Result<()> {
    /// # use tev_client::PacketCloseImage;
    /// # let mut client: TevClient = unimplemented!();
    /// client.send(PacketCloseImage { image_name: "test.exf" })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn send(&mut self, packet: impl TevPacket) -> io::Result<()> {
        //reserve space for the packet length
        let vec = vec![0, 0, 0, 0];

        //append the packet
        let mut target = TevWriter { target: vec };
        packet.write_to(&mut target);
        let mut vec = target.target;

        //actually fill in the packet length
        let packet_length = vec.len() as u32;
        vec[0..4].copy_from_slice(&packet_length.to_le_bytes());

        println!("Sending {:?}", vec);
        self.socket.write_all(&vec)
    }
}

/// Opens a new image where [image_name] is the path.
#[derive(Debug)]
pub struct PacketOpenImage<'a> {
    pub image_name: &'a str,
    pub grab_focus: bool,
    pub channel_selector: &'a str,
}

impl TevPacket for PacketOpenImage<'_> {
    fn write_to(&self, writer: &mut TevWriter) {
        writer.write(PacketType::OpenImageV2);
        writer.write(self.grab_focus);
        writer.write(self.image_name);
        writer.write(self.channel_selector);
    }
}

/// Reload an existing image with name or path [image_name] from disk.
#[derive(Debug)]
pub struct PacketReloadImage<'a> {
    pub image_name: &'a str,
    pub grab_focus: bool,
}

impl TevPacket for PacketReloadImage<'_> {
    fn write_to(&self, writer: &mut TevWriter) {
        writer.write(PacketType::ReloadImage);
        writer.write(self.grab_focus);
        writer.write(self.image_name);
    }
}

/// Update part of an existing image with new pixel data.
#[derive(Debug)]
pub struct PacketUpdateImage<'a, S: AsRef<str> + 'a> {
    pub image_name: &'a str,
    pub grab_focus: bool,
    pub channel_names: &'a [S],
    pub channel_offsets: &'a [u32],
    pub channel_strides: &'a [u32],
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: &'a [f32],
}

impl<'a, S: AsRef<str> + 'a> TevPacket for PacketUpdateImage<'a, S> {
    fn write_to(&self, writer: &mut TevWriter) {
        let channel_count = self.channel_names.len();

        assert_ne!(channel_count, 0, "Must update at least one channel");
        assert_eq!(channel_count, self.channel_offsets.len(), "Channel count must be consistent");
        assert_eq!(channel_count, self.channel_strides.len(), "Channel count must be consistent");

        let pixel_count = self.width * self.height;
        assert_ne!(pixel_count, 0, "Must update at least one pixel");

        let max_data_index_used = self.channel_offsets.iter().zip(self.channel_strides)
            .map(|(&o, &s)| (o as u64) + (pixel_count as u64 - 1) * (s as u64))
            .max().unwrap();
        assert_eq!(max_data_index_used + 1, self.data.len() as u64, "Data size does not match actually used data range");

        writer.write(PacketType::UpdateImageV3);
        writer.write(self.grab_focus);
        writer.write(self.image_name);
        writer.write(channel_count as u32);
        writer.write_all(self.channel_names.iter().map(AsRef::as_ref));
        writer.write(self.x);
        writer.write(self.y);
        writer.write(self.width);
        writer.write(self.height);
        writer.write_all(self.channel_offsets.iter().map(|&x| x as u64));
        writer.write_all(self.channel_strides.iter().map(|&x| x as u64));

        writer.write_all(self.data)
    }
}

/// Close an image.
#[derive(Debug)]
pub struct PacketCloseImage<'a> {
    pub image_name: &'a str,
}

impl TevPacket for PacketCloseImage<'_> {
    fn write_to(&self, writer: &mut TevWriter) {
        writer.write(PacketType::CloseImage);
        writer.write(self.image_name);
    }
}

/// Create a new image with name [image_name], size ([width], [height]) and channels [channel_names].
#[derive(Debug)]
pub struct PacketCreateImage<'a, S: AsRef<str> + 'a> {
    pub image_name: &'a str,
    pub grab_focus: bool,
    pub width: u32,
    pub height: u32,
    pub channel_names: &'a [S],
}

impl<'a, S: AsRef<str> + 'a> TevPacket for PacketCreateImage<'a, S> {
    fn write_to(&self, writer: &mut TevWriter) {
        writer.write(PacketType::CreateImage);
        writer.write(self.grab_focus);
        writer.write(self.image_name);
        writer.write(self.width);
        writer.write(self.height);
        writer.write(self.channel_names.len() as u32);
        writer.write_all(self.channel_names.iter().map(AsRef::as_ref));
    }
}

pub struct TevWriter {
    target: Vec<u8>
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
enum PacketType {
    ReloadImage = 1,
    CloseImage = 2,
    CreateImage = 4,
    UpdateImageV3 = 6,
    OpenImageV2 = 7,
}

impl TevWriter {
    fn write(&mut self, value: impl TevWritable) {
        value.write_to(self);
    }

    fn write_all(&mut self, values: impl IntoIterator<Item=impl TevWritable>) {
        for value in values {
            value.write_to(self);
        }
    }
}

pub trait TevPacket {
    fn write_to(&self, writer: &mut TevWriter);
}

trait TevWritable {
    fn write_to(self, writer: &mut TevWriter);
}

impl<T: TevWritable + Copy> TevWritable for &T {
    fn write_to(self, writer: &mut TevWriter) {
        (*self).write_to(writer);
    }
}

impl TevWritable for bool {
    fn write_to(self, writer: &mut TevWriter) {
        writer.target.push(self as u8);
    }
}

impl TevWritable for PacketType {
    fn write_to(self, writer: &mut TevWriter) {
        writer.target.push(self as u8);
    }
}

impl TevWritable for u32 {
    fn write_to(self, writer: &mut TevWriter) {
        writer.target.extend_from_slice(&self.to_le_bytes());
    }
}

impl TevWritable for u64 {
    fn write_to(self, writer: &mut TevWriter) {
        writer.target.extend_from_slice(&self.to_le_bytes());
    }
}

impl TevWritable for f32 {
    fn write_to(self, writer: &mut TevWriter) {
        writer.target.extend_from_slice(&self.to_le_bytes());
    }
}

impl TevWritable for &'_ str {
    fn write_to(self, writer: &mut TevWriter) {
        assert!(!self.contains('\0'), "cannot send strings containing '\\0'");
        writer.target.extend_from_slice(self.as_bytes());
        writer.target.push(0);
    }
}
