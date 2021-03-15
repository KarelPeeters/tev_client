# tev_client

This Rust crate implements a IPC TCP client for [tev](https://github.com/Tom94/tev). 

Supports all existing commands:
* `OpenImage` open an existing image given the path
* `ReloadImage` reload an image from disk
* `CloseImage` close an opened image
* `CreateImage` create a new black image with given size and channels
* `UpdateImage` update part of the pixels of an opened image

## Example code:

```rust
use std::process::Command;
use tev_client::{PacketCreateImage, TevClient};

fn main() -> std::io::Result<()> {
    //spawn a tev instance, this command assumes tev is in PATH
    let mut client = TevClient::spawn_path_default()?;

    //send a command to tev
    client.send(PacketCreateImage {
        image_name: "test",
        grab_focus: false,
        width: 1920,
        height: 1080,
        channel_names: &["R", "G", "B"],
    })?;
    
    Ok(())
}
```