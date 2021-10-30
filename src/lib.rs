//! This Rust crate implements a IPC TCP client for [tev](https://github.com/Tom94/tev).
//! It enables programmatic control of the images displayed by _tev_ using a convenient and safe Rust api.
//!
//! Supports all existing _tev_ commands:
//! * [PacketOpenImage] open an existing image given the path
//! * [PacketReloadImage] reload an image from disk
//! * [PacketCloseImage] close an opened image
//! * [PacketCreateImage] create a new black image with given size and channels
//! * [PacketUpdateImage] update part of the pixels of an opened image
//!
//! ## Example code:
//!
//! ```rust
//! use tev_client::{TevClient, TevError, PacketCreateImage};
//!
//! fn main() -> Result<(), TevError> {
//!     // Spawn a tev instance, this command assumes tev is on the PATH.
//!     // There are other constructors available too, see TevClient::spawn and TevClient::wrap.
//!     let mut client = TevClient::spawn_path_default()?;
//!
//!     // Create a new image
//!     client.send(PacketCreateImage {
//!         image_name: "test",
//!         grab_focus: false,
//!         width: 1920,
//!         height: 1080,
//!         channel_names: &["R", "G", "B"],
//!     })?;
//!
//!     Ok(())
//! }
//! ```

use std::io;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

/// A connection to a Tev instance.
/// Constructed using [TevClient::wrap], [TevClient::spawn] or [TevClient::spawn_path_default].
/// Use [TevClient::send] to send commands.
#[derive(Debug)]
pub struct TevClient {
    socket: TcpStream,
}

/// The error type returned by [TevClient::spawn] in case of an error.
///
/// For convenience, this type implements `From<std::io::Error>` so the errors returned by [TevClient::send]
/// can be wrapped into this type by the `?` operator.
#[derive(Debug)]
pub enum TevError {
    /// Error during command execution.
    Command { io: std::io::Error },
    /// Error while reading from stdout of the spawned process.
    Stdout { io: std::io::Error },
    /// Tev didn't respond with an address to connect to on stdout.
    /// `read` is the data that was read before stdout closed.
    NoSocketResponse { read: String },
    /// There was an error opening or writing to the TCP connection.
    /// `host` is the address received from _tev_ we're trying to connect to.
    TcpConnect { host: String, io: std::io::Error },
    /// There was some other IO error.
    IO { io: std::io::Error },
}

impl TevClient {
    /// Create a [TevClient] from an existing [TcpStream] that's connected to _tev_. If _tev_ may not be running yet use
    /// [TevClient::spawn] or [TevClient::spawn_path_default] instead.
    ///
    /// For example, if _tev_ is already running on the default hostname:
    /// ```no_run
    /// # use tev_client::{TevClient, TevError};
    /// # use std::net::TcpStream;
    /// # fn main() -> std::io::Result<()> {
    /// let mut client = TevClient::wrap(TcpStream::connect("127.0.0.1:14158")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn wrap(socket: TcpStream) -> Self {
        TevClient { socket }
    }

    /// Create a new [TevClient] by spawning _tev_ assuming it is in `PATH` with the default hostname.
    pub fn spawn_path_default() -> Result<TevClient, TevError> {
        TevClient::spawn(Command::new("tev"))
    }

    /// Crate a [TevClient] from a command that spawns _tev_.
    /// If _tev_ is in `PATH` and the default hostname should be used use [TevClient::spawn_path_default] instead.
    ///
    /// ```no_run
    /// # use tev_client::{TevClient, TevError};
    /// # use std::process::Command;
    /// # fn main() -> Result<(), TevError> {
    /// let mut command = Command::new("path/to/tev");
    /// command.arg("--hostname=127.0.0.1:14159");
    /// let mut client = TevClient::spawn(command)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn spawn(mut command: Command) -> Result<TevClient, TevError> {
        const PATTERNS: &[&str] = &[
            "Initialized IPC, listening on ",
            "Connected to primary instance at ",
        ];

        let mut child = command.stdout(Stdio::piped()).spawn()
            .map_err(|io| TevError::Command { io })?;
        let reader = BufReader::new(child.stdout.take().unwrap());

        let mut read = String::new();
        for line in reader.lines() {
            let line = line.map_err(|io| TevError::Stdout { io })?;

            for pattern in PATTERNS {
                if let Some(start) = line.find(pattern) {
                    let rest = &line[start + pattern.len()..];

                    // cut of any trailing terminal escape codes
                    let end = rest.find('\u{1b}').unwrap_or(rest.len());
                    let host = &rest[..end];

                    let socket = TcpStream::connect(host)
                        .map_err(|io| TevError::TcpConnect { host: host.to_string(), io })?;
                    return Ok(TevClient::wrap(socket));
                }
            }

            read.push_str(&line);
            read.push('\n');
        }

        return Err(TevError::NoSocketResponse { read });
    }

    /// Send a command to _tev_. A command is any struct in this crate that implements [TevPacket].
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

        self.socket.write_all(&vec)
    }
}

/// Opens a new image where `image_name` is the path.
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

/// Reload an existing image with name or path `image_name` from disk.
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
    pub channel_offsets: &'a [u64],
    pub channel_strides: &'a [u64],
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

        let pixel_count = (self.width as u64) * (self.height as u64);
        assert_ne!(pixel_count, 0, "Must update at least one pixel");

        let max_data_index_used = self.channel_offsets.iter().zip(self.channel_strides)
            .map(|(&o, &s)| o + (pixel_count - 1) * s)
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
        writer.write_all(self.channel_offsets);
        writer.write_all(self.channel_strides);

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

/// Create a new image with name `image_name`, size (`width`, `height`) and channels `channel_names`.
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

/// A buffer used to construct TCP packets. For internal use only.
#[doc(hidden)]
pub struct TevWriter {
    target: Vec<u8>,
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

/// The trait implemented by all packets.
#[doc(hidden)]
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

impl From<std::io::Error> for TevError {
    fn from(io: std::io::Error) -> Self {
        TevError::IO { io }
    }
}