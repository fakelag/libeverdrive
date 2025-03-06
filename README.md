### libeverdrive

libeverdrive provides a rust interface for programming [EverDrive](https://krikzz.com/) devices through its USB development port.

#### Installing
```shell
cargo add libeverdrive
```

#### Usage example
```rust
use libeverdrive::Everdrive;

fn main() {
    let mut ed = match Everdrive::new(std::time::Duration::from_millis(100)) {
        Ok(ed) => {
            println!("Everdrive device found");
            ed
        },
        Err(err) => {
            println!("Failed to find Everdrive: {:?}", err);
            return;
        }
    };

    match ed.status() {
        Ok(_) => println!("ED status OK"),
        Err(err) => println!("ED status error: {:?}", err),
    }

    let mut buf = [0; 512];
    match ed.rom_read(0x10000000, &mut buf) {
        Ok(_) => println!("Rom content: {:?}", buf),
        Err(err) => println!("Read error: {:?}", err),
    }
}
```