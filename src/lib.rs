use std::{fs, io::Read};

pub const ROM_BASE_ADDR: u32 = 0x10000000;
pub const ROM_BASE_ADDR_EMU: u32 = 0x10200000;

pub enum EdCommand {
    Test,
    RamRead(u32, u32),
    RomRead(u32, u32),
    RomWrite(u32, u32),
    RomFill(u32, u32, u32),
    FpgaInit(u32),
    AppStart(bool),
}

#[repr(u8)]
pub enum EdSaveType {
    Eeprom4k = 0x10,
    Eeprom16k = 0x20,
    Sram = 0x30,
    Sram768k = 0x40,
    FlashRam = 0x50,
    Sram128k = 0x60,
}

#[repr(u8)]
pub enum EdRtcRegionType {
    Rtc = 0x01,
    NoRegion = 0x02,
    All = 0x03,
}

impl EdCommand {
    fn to_bytes(&self) -> std::io::Result<[u8; 16]> {
        const CMD_PREFIX: &[u8; 3] = b"cmd";

        let (cmd, addr, size, arg) = match self {
            EdCommand::Test => (b't', 0u32, 0u32, 0u32),
            EdCommand::RamRead(addr, size) => (b'r', *addr, *size, 0),
            EdCommand::RomRead(addr, size) => (b'R', *addr, *size, 0),
            EdCommand::RomWrite(addr, size) => (b'W', *addr, *size, 0),
            EdCommand::RomFill(addr, size, arg) => (b'c', *addr, *size, *arg),
            EdCommand::FpgaInit(size) => (b'f', 0, *size, 0),
            EdCommand::AppStart(save_path) => (b's', 0, 0, *save_path as u32),
        };

        let size = if size % 512 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Size must be a multiple of 512",
            ));
        } else {
            size / 512
        };

        let mut buf = [0; 16];
        buf[0..3].copy_from_slice(CMD_PREFIX);

        buf[3] = cmd;

        buf[4..8].copy_from_slice(&addr.to_be_bytes());
        buf[8..12].copy_from_slice(&size.to_be_bytes());
        buf[12..16].copy_from_slice(&arg.to_be_bytes());

        Ok(buf)
    }
}

#[derive(Debug)]
pub struct Everdrive {
    port: Box<dyn serialport::SerialPort>,
}

impl Everdrive {
    pub fn new(timeout: std::time::Duration) -> std::io::Result<Self> {
        let ports = serialport::available_ports().expect("No available USB ports found");

        let usb_port = match ports.iter().find(|p| match &p.port_type {
            serialport::SerialPortType::UsbPort(info) => info.vid == 0x0403 && info.pid == 0x6001,
            _ => false,
        }) {
            Some(port) => port,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Everdrive USB device not found",
                ));
            }
        };

        let mut port = match serialport::new(&usb_port.port_name, 115_200).open() {
            Ok(port) => port,
            Err(err) => {
                return Err(err.into());
            }
        };

        match port.set_timeout(timeout) {
            Ok(_) => (),
            Err(err) => {
                return Err(err.into());
            }
        };

        let mut ed = Self { port };

        ed.status()?;

        Ok(ed)
    }

    pub fn status(&mut self) -> std::io::Result<()> {
        self.tx(EdCommand::Test)?;
        self.rx(b'r')?;
        Ok(())
    }

    pub fn rom_fill(&mut self, addr: u32, size: u32, val: u32) -> std::io::Result<()> {
        self.tx(EdCommand::RomFill(addr, size, val))
    }

    pub fn rom_read(&mut self, addr: u32, buf: &mut [u8]) -> std::io::Result<()> {
        self.tx(EdCommand::RomRead(addr, buf.len() as u32))?;
        self.read(buf)
    }

    pub fn rom_read_size<const S: usize>(&mut self, addr: u32) -> std::io::Result<[u8; S]> {
        let mut buf = [0; S];
        self.rom_read(addr, &mut buf)?;
        Ok(buf)
    }

    pub fn ram_read(&mut self, addr: u32, buf: &mut [u8]) -> std::io::Result<()> {
        self.tx(EdCommand::RamRead(addr, buf.len() as u32))?;
        self.read(buf)
    }

    pub fn ram_read_size<const S: usize>(&mut self, addr: u32) -> std::io::Result<[u8; S]> {
        let mut buf = [0; S];
        self.ram_read(addr, &mut buf)?;
        Ok(buf)
    }

    pub fn rom_write(&mut self, addr: u32, data: &[u8]) -> std::io::Result<()> {
        self.tx(EdCommand::RomWrite(addr, data.len() as u32))?;
        self.write(data)
    }

    pub fn fpga_init(&mut self, size: u32, data: &[u8]) -> std::io::Result<()> {
        self.tx(EdCommand::FpgaInit(size))?;
        self.write(data)?;
        self.rx(b'r')
    }

    /// Starts a rom file. The rom file must be loaded first using `load_rom`
    ///
    /// Optional `file_name` is used for specifying save file on the SD card
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    /// use std::fs;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let rom_data = fs::read("your_rom.z64").unwrap();
    ///
    /// ed.load_rom(rom_data, None, None, None).unwrap();
    /// ed.app_start(Some("your_rom.z64")).unwrap();
    ///
    /// ```
    pub fn app_start(&mut self, file_name: Option<&str>) -> std::io::Result<()> {
        let file_name_buf = if let Some(file_name) = file_name {
        if file_name.len() >= 256 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "File name is too long",
            ));
        }

        let mut buf = [0; 256];
        buf[0..file_name.len()].copy_from_slice(file_name.as_bytes());

            Some(buf)
        } else {
            None
        };

        self.tx(EdCommand::AppStart(file_name_buf.is_some()))?;

        if let Some(buf) = file_name_buf {
            self.write(&buf)?;
    }

        Ok(())
    }

    /// Loads a rom file into the specified base address
    ///
    /// `rom_file` should contain the rom file as data. The base address is optional and defaults to ROM_BASE_ADDR.
    /// `save_type` and `rtc_region_type` are optional and are used to specify the save type and RTC region type respectively.
    /// Additional checks are done to determine the endianness of the rom file and swap bytes accordingly, and
    /// to set the save type and RTC region type in the rom file header.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    /// use std::fs;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let rom_data = fs::read("your_rom.z64").unwrap();
    ///
    /// ed.load_rom(rom_data, None, None, None).unwrap();
    /// ed.app_start(Some("your_rom.z64")).unwrap();
    ///
    /// ```
    pub fn load_rom(
        &mut self,
        rom_file: Vec<u8>,
        base_address: Option<u32>,
        save_type: Option<EdSaveType>,
        rtc_region_type: Option<EdRtcRegionType>,
    ) -> std::io::Result<()> {
        // reference https://github.com/krikzz/ED64/blob/master/usb64/usb64/CommandProcessor.cs#L125
        let mut rom_file = rom_file.clone();

        let header_word_be =
            u32::from_be_bytes([rom_file[0], rom_file[1], rom_file[2], rom_file[3]]);

        let mut base_address = base_address.unwrap_or(ROM_BASE_ADDR);

        match header_word_be {
            0x80371240 /* Big-endian native */ => { /* No need to do anything */}
            0x37804012 /* Byte-swapped, swap every 2 bytes */=> {
                for i in (0..rom_file.len()).step_by(2) {
                    rom_file.swap(i, i + 1);
                }
            }
            0x40123780 /* Little-endian, swap every 4 bytes */ => {
                for i in (0..rom_file.len()).step_by(4) {
                    rom_file.swap(i, i + 3);
                    rom_file.swap(i + 1, i + 2);
                }
            }
            _ => {
                // Don't swap and assume emulator rom
                base_address = ROM_BASE_ADDR_EMU;
            }
        };

        if let Some(st) = save_type {
            let region_type = rtc_region_type.map(|val| val as u8).unwrap_or(0);
            rom_file[0x3C] = 0x45;
            rom_file[0x3D] = 0x44;
            rom_file[0x3F] = ((st as u8) << 4) | region_type;
        }

        self.load_rom_force(rom_file, base_address)?;

        Ok(())
    }

    /// Loads a rom file into the specified base address. But does not do checks for
    /// endianness or base_address.
    pub fn load_rom_force(&mut self, data: Vec<u8>, base_address: u32) -> std::io::Result<()> {
        const CRC_AREA_SIZE: usize = 0x101000;

        if data.len() < CRC_AREA_SIZE {
            let val = if Self::is_ed_bootloader(&data) {
                0xFFFFFFFF
            } else {
                0
            };

            self.rom_fill(base_address, CRC_AREA_SIZE as u32, val)?;
        }

        self.tx(EdCommand::RomWrite(base_address, data.len() as u32))?;
        self.write(&data)
    }

    pub fn tx(&mut self, cmd: EdCommand) -> std::io::Result<()> {
        self.port.write_all(&cmd.to_bytes()?)
    }

    pub fn rx(&mut self, resp: u8) -> std::io::Result<()> {
        let mut recv_buf = vec![0; 16];

        match self.read(&mut recv_buf) {
            Ok(_) => {
                if recv_buf[0..4] == [b'c', b'm', b'd', resp] {
                    Ok(())
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Invalid response from Everdrive device",
                    ))
                }
            }
            Err(err) => return Err(err),
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.port.write_all(buf)
    }

    pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.port.read_exact(buf)
    }

    fn is_ed_bootloader(data: &[u8]) -> bool {
        const ED_HEADER: &[u8] = b"EverDrive bootloader";
        ED_HEADER
            .iter()
            .zip(data.iter().skip(0x20))
            .all(|(a, b)| a == b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let mut ed = match Everdrive::new(std::time::Duration::from_millis(100)) {
            Ok(ed) => {
                println!("Everdrive device found");
                ed
            }
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
}
