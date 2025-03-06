use std::io::Read;

#[cfg(feature = "bitmap")]
use bmp::{Image, Pixel};

pub const ROM_BASE_ADDR: u32 = 0x10000000;
pub const ROM_BASE_ADDR_EMU: u32 = 0x10200000;
pub const KSEG0_BASE_ADDR: u32 = 0x80000000;

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
    /// Creates a new Everdrive instance and returns an error if the device is not found
    /// or if there is an error opening the USB serial port.
    ///
    /// `timeout` is configured for the serial port for future reads and writes
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = match Everdrive::new(std::time::Duration::from_millis(100)) {
    ///     Ok(ed) => ed,
    ///     Err(err) => {
    ///         eprintln!("Failed to find Everdrive: {:?}", err);
    ///         return;
    ///     }
    /// };
    /// ```
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

    /// Tests a handshake with the Everdrive device and returns an error if the handshake fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// match ed.status() {
    ///    Ok(_) => println!("ED status OK"),
    ///   Err(err) => eprintln!("ED status error: {:?}", err),
    /// }
    /// ```
    pub fn status(&mut self) -> std::io::Result<()> {
        self.tx(EdCommand::Test)?;
        self.rx(b'r')?;
        Ok(())
    }

    /// Takes a screenshot from the N64 device. The return value is the n64 framebuffer
    /// and the screen with and height.
    ///
    /// Optional `screen_size_wh` can be used to specify screen width and height. If
    /// None, an attempt is made to determine the screen size automatically via
    /// the console video interface.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let framebuffer = ed.screenshot(None).unwrap();
    /// println!("{:?}", framebuffer);
    /// ```
    pub fn screenshot(
        &mut self,
        screen_size_wh: Option<(u32, u32)>,
    ) -> std::io::Result<(Vec<u8>, u32, u32)> {
        let mut buf = [0; 512];
        self.ram_read(0xA4400004, &mut buf)?;

        let fb_ptr = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);

        let (screen_width, screen_height) = match screen_size_wh {
            Some((w, h)) => (w, h),
            None => {
                let screen_width = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
                match screen_width {
                    320 => (screen_width, 240),
                    640 => (screen_width, 480),
                    _ /* Default height to 240 */ => (screen_width, 240),
                }
            }
        };

        let mut buf = vec![0; (screen_width * screen_height * 2) as usize];
        self.ram_read(KSEG0_BASE_ADDR + fb_ptr, &mut buf)?;
        Ok((buf, screen_width, screen_height))
    }

    #[cfg(feature = "bitmap")]
    /// Takes a screenshot and converts it to a BMP image buffer.
    ///
    /// Optional `screen_size_wh` can be used to specify screen width and height. If
    /// None, an attempt is made to determine the screen size automatically via
    /// the console video interface.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let bmp_buf = ed.screenshot_bmp(None).unwrap();
    ///
    /// std::fs::write("screenshot.bmp", bmp_buf).unwrap();
    /// ```
    pub fn screenshot_bmp(
        &mut self,
        screen_size_wh: Option<(u32, u32)>,
    ) -> std::io::Result<Vec<u8>> {
        let (buf, width, height) = self.screenshot(screen_size_wh)?;
        Self::n64_fb_to_bitmap(&buf, width, height)
    }

    /// Fills a region of the rom with a value.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// ed.rom_fill(0x10000000, 0x1000, 0xFF).unwrap();
    /// ```
    pub fn rom_fill(&mut self, addr: u32, size: u32, val: u32) -> std::io::Result<()> {
        self.tx(EdCommand::RomFill(addr, size, val))
    }

    /// Reads a region of the rom into a buffer. Buffer size must be divisible by 512.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let mut buf = vec![0; 512];
    /// ed.rom_read(0x10000000, &mut buf).unwrap();
    ///
    /// println!("{:?}", buf);
    /// ```
    pub fn rom_read(&mut self, addr: u32, buf: &mut [u8]) -> std::io::Result<()> {
        self.tx(EdCommand::RomRead(addr, buf.len() as u32))?;
        self.read(buf)
    }

    /// Allocates a new buffer of size `S` and reads a region of the rom into it.
    /// Size `S` must be divisible by 512.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let buf = ed.rom_read_size::<512>(0x10000000).unwrap();
    /// println!("{:?}", buf);
    /// ```
    pub fn rom_read_size<const S: usize>(&mut self, addr: u32) -> std::io::Result<[u8; S]> {
        let mut buf = [0; S];
        self.rom_read(addr, &mut buf)?;
        Ok(buf)
    }

    /// Reads a region of the ram into a buffer. Buffer size must be divisible by 512.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let mut buf = vec![0; 512];
    /// ed.ram_read(0x10000000, &mut buf).unwrap();
    ///
    /// println!("{:?}", buf);
    /// ```
    pub fn ram_read(&mut self, addr: u32, buf: &mut [u8]) -> std::io::Result<()> {
        self.tx(EdCommand::RamRead(addr, buf.len() as u32))?;
        self.read(buf)
    }

    /// Allocates a new buffer of size `S` and reads a region of the ram into it.
    /// Size `S` must be divisible by 512.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let buf = ed.ram_read_size::<512>(0x10000000).unwrap();
    /// println!("{:?}", buf);
    /// ```
    pub fn ram_read_size<const S: usize>(&mut self, addr: u32) -> std::io::Result<[u8; S]> {
        let mut buf = [0; S];
        self.ram_read(addr, &mut buf)?;
        Ok(buf)
    }

    /// Writes a region of the rom with data. Data size must be divisible by 512.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let data = vec![0; 512];
    /// ed.rom_write(0x10000000, &data).unwrap();
    /// ```
    pub fn rom_write(&mut self, addr: u32, data: &[u8]) -> std::io::Result<()> {
        self.tx(EdCommand::RomWrite(addr, data.len() as u32))?;
        self.write(data)
    }

    /// Inits fpga with a RBF file. Data size must be divisible by 512.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let mut ed = Everdrive::new(std::time::Duration::from_millis(100)).unwrap();
    ///
    /// let fpga_data = vec![0; 0x100000];
    /// ed.fpga_init(0x100000, &fpga_data).unwrap();
    /// ```
    pub fn fpga_init(&mut self, size: u32, data: &[u8]) -> std::io::Result<()> {
        self.tx(EdCommand::FpgaInit(size))?;
        self.write(data)?;

        // @todo - Check that the second response byte is 0
        // non-zero are error codes
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

    /// Transmits an EdCommand to the Everdrive device
    /// and returns an error if sending the command fails.
    pub fn tx(&mut self, cmd: EdCommand) -> std::io::Result<()> {
        self.port.write_all(&cmd.to_bytes()?)
    }

    /// Receives a response from the Everdrive device
    /// and returns an error if reading from the device fails
    /// or if the response is invalid.
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

    /// Directly write a buffer to the serial port
    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.port.write_all(buf)
    }

    /// Directly read a buffer from the serial port
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

    #[cfg(feature = "bitmap")]
    fn n64_fb_to_bitmap(fb: &[u8], width: u32, height: u32) -> std::io::Result<Vec<u8>> {
        let mut img = Image::new(width, height);

        for y in 0..height {
            for x in 0..width {
                let b0 = fb[(y * width + x) as usize * 2];
                let b1 = fb[(y * width + x) as usize * 2 + 1];

                let r = b0 & 0xF8;
                let g = ((b0 & 0x07) << 5) | ((b1 & 0xC0) >> 3);
                let b = (b1 & 0x3E) << 2;

                img.set_pixel(x, y, Pixel::new(r, g, b));
            }
        }

        let mut img_buf: Vec<u8> = Vec::new();
        img.to_writer(&mut img_buf)?;

        Ok(img_buf)
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
