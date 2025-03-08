use crate::Everdrive;

const UNF_MAGIC: u32 = 0x444d4140;
const UNF_FOOTER: u32 = 0x434d5048;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum UnfDataType {
    DataTypeText,
    DataTypeBinary,
    DataTypeHeader,
    DataTypeScreenshot,
    DataTypeHeartbeat,
    DataTypeRdbPacket,
    DataTypeUnknown,
}

impl From<u8> for UnfDataType {
    fn from(byte: u8) -> Self {
        match byte {
            0x01 => UnfDataType::DataTypeText,
            0x02 => UnfDataType::DataTypeBinary,
            0x03 => UnfDataType::DataTypeHeader,
            0x04 => UnfDataType::DataTypeScreenshot,
            0x05 => UnfDataType::DataTypeHeartbeat,
            0x06 => UnfDataType::DataTypeRdbPacket,
            _ => UnfDataType::DataTypeUnknown,
        }
    }
}

impl Into<u8> for UnfDataType {
    fn into(self) -> u8 {
        match self {
            UnfDataType::DataTypeText => 0x01,
            UnfDataType::DataTypeBinary => 0x02,
            UnfDataType::DataTypeHeader => 0x03,
            UnfDataType::DataTypeScreenshot => 0x04,
            UnfDataType::DataTypeHeartbeat => 0x05,
            UnfDataType::DataTypeRdbPacket => 0x06,
            UnfDataType::DataTypeUnknown => 0xFF,
        }
    }
}

struct PacketReader<'a> {
    buf: &'a [u8],
    offset: usize,
}

impl<'a> PacketReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    fn consume_byte(&mut self) -> u8 {
        let byte = self.buf[self.offset];
        self.offset += 1;
        byte
    }

    fn consume_word(&mut self) -> u32 {
        let word = u32::from_be_bytes([
            self.buf[self.offset],
            self.buf[self.offset + 1],
            self.buf[self.offset + 2],
            self.buf[self.offset + 3],
        ]);
        self.offset += 4;
        word
    }

    fn get_offset(&self) -> usize {
        self.offset
    }

    fn skip(&mut self, n: usize) {
        self.offset += n;
    }
}

#[derive(Debug)]
pub struct UnfRecvPacket {
    datatype: UnfDataType,
    data: Vec<u8>,
}

impl UnfRecvPacket {
    pub fn get_data(&self) -> &[u8] {
        &self.data
    }

    pub fn get_datatype(&self) -> UnfDataType {
        self.datatype
    }
}

#[derive(Debug)]
pub struct UnfSendPacket {
    data_size: u32,
    backing: Vec<u8>,
}

impl UnfSendPacket {
    pub fn new(data_type: UnfDataType, data_size: usize) -> std::io::Result<Self> {
        if data_size > 0x00FFFFFF {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Data size must be less than 0x00FFFFFF",
            ));
        }

        let align_bytes = data_size & 1;

        let mut data = vec![0; data_size + 12 + align_bytes];

        data[0..8].copy_from_slice(&[
            (UNF_MAGIC >> 24) as u8,
            (UNF_MAGIC >> 16) as u8,
            (UNF_MAGIC >> 8) as u8,
            UNF_MAGIC as u8,
            data_type.into(),
            (data_size >> 16) as u8,
            (data_size >> 8) as u8,
            data_size as u8,
        ]);

        data[data_size + 8..data_size + 8 + align_bytes].fill(0xFF);

        data[data_size + 8 + align_bytes..].copy_from_slice(&[
            (UNF_FOOTER >> 24) as u8,
            (UNF_FOOTER >> 16) as u8,
            (UNF_FOOTER >> 8) as u8,
            UNF_FOOTER as u8,
        ]);

        Ok(Self {
            backing: data,
            data_size: data_size as u32,
        })
    }

    pub fn get_data(&mut self) -> &mut [u8] {
        &mut self.backing[8..8 + self.data_size as usize]
    }
}

impl Everdrive {
    pub fn unf_tx(&mut self, packet: &UnfSendPacket) -> std::io::Result<()> {
        self.write_all(&packet.backing)
    }

    pub fn unf_rx(&mut self) -> std::io::Result<UnfRecvPacket> {
        let magic = self.read_word_be().map_err(|e| {
            std::io::Error::new(e.kind(), format!("Failed to read UNF packet magic {}", e))
        })?;

        if magic != /* "DMA@" */ UNF_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid UNF packet magic {}, expected {}", magic, UNF_MAGIC),
            ));
        }

        let header = self.read_word_be().map_err(|e| {
            std::io::Error::new(e.kind(), format!("Failed to read UNF packet header {}", e))
        })?;

        let dsize = header & 0x00FFFFFF;
        let dtype = (header >> 24) as u8;

        let datatype = dtype.try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid packet UnfDataType {}", dtype),
            )
        })?;

        let mut data = vec![0; dsize as usize];

        self.read_exact(&mut data).map_err(|e| {
            std::io::Error::new(e.kind(), format!("Failed to read UNF packet data {}", e))
        })?;

        let cmp = self.read_word_be().map_err(|e| {
            std::io::Error::new(e.kind(), format!("Failed to read UNF packet footer {}", e))
        })?;

        if cmp != /* "CMPH" */ UNF_FOOTER {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid UNF packet footer {}, expected {}", cmp, UNF_FOOTER),
            ));
        }

        Ok(UnfRecvPacket { datatype, data })
    }
}
