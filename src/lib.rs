mod edos;
mod unf;

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
    /// let usb_ports = Everdrive::find_usb_devices();
    /// assert!(!usb_ports.is_empty());
    ///
    /// let mut ed = Everdrive::new(&usb_ports[0]).unwrap();
    ///
    /// assert!(ed.ed_status().is_ok());
    ///  ```
    pub fn new(port_name: &str) -> std::io::Result<Self> {
        let port = match serialport::new(port_name, 115_200).open() {
            Ok(port) => port,
            Err(err) => {
                return Err(err.into());
            }
        };

        let mut ed = Self { port };
        ed.set_timeout(std::time::Duration::from_millis(100))?;
        Ok(ed)
    }

    pub fn set_timeout(&mut self, timeout: std::time::Duration) -> std::io::Result<()> {
        match self.port.set_timeout(timeout) {
            Ok(_) => Ok(()),
            Err(err) => {
                return Err(err.into());
            }
        }
    }

    pub fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.port.write_all(buf)
    }

    pub fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.port.read_exact(buf)
    }

    pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.port.read(buf)
    }

    pub fn read_word_be(&mut self) -> std::io::Result<u32> {
        let mut buf = [0; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_be_bytes(buf))
    }

    pub fn read_byte(&mut self) -> std::io::Result<u8> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    /// Find available USB ports with everdrive devices and returns a list of port names
    /// matching Everdrive VID and PID. The port name can be used to create a new Everdrive instance.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use libeverdrive::Everdrive;
    ///
    /// let usb_ports = Everdrive::find_usb_devices();
    ///
    /// println!("Found devices: {:?}", usb_ports);
    /// ```
    pub fn find_usb_devices() -> Vec<String> {
        let ports = serialport::available_ports().expect("No available USB ports found");

        let ed_device_ports = ports.iter().filter_map(|p| match &p.port_type {
            serialport::SerialPortType::UsbPort(info) => {
                if info.vid == 0x0403 && info.pid == 0x6001 {
                    Some(p.port_name.clone())
                } else {
                    None
                }
            }
            _ => None,
        });

        ed_device_ports.collect()
    }
}
